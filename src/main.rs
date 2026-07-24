mod img_atlas;
mod img_cut;
mod img_resize;

use std::path::PathBuf;

use clap::Parser;
use image::codecs::png::CompressionType;
use img_atlas::AtlasConfig;
use img_resize::{collect_files, parse_size, resize_image};
use rayon::prelude::*;

/// image tool — resize & atlas packing.
///
/// A tiny Rust CLI tool for batch image resizing and sprite-atlas packing,
/// powered by `image` + `rayon` multithreading.
#[derive(Parser, Debug)]
#[command(name = "img_tool", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Batch resize images with aspect-ratio control (keep/center or stretch), multi-threaded
    #[command(
        after_help = "Examples:\n  img_tool resize --input ./images --output ./out --size 256x256 --aspect keep"
    )]
    Resize(ResizeArgs),
    /// Pack images into a single sprite atlas with tight packing or uniform-grid layout
    #[command(
        after_help = "Examples:\n  Tight packing (preserves original dims):\n    img_tool atlas -i ./images -o atlas.png\n    img_tool atlas -i ./images -o atlas.png --frames 4x3\n    img_tool atlas -i ./images -o atlas.png --atlas-size 4096x4096\n\n  Uniform grid (all images resized to same cell size):\n    img_tool atlas -i ./images -o atlas.png --resize -s 256x256 --spacing 10x10\n    img_tool atlas -i ./images -o atlas.png --resize -s 128x128 --frames 8x8 --spacing 4x4\n\n  Auto-sizing: atlas dimensions are rounded up to the nearest multiple of 512, always square."
    )]
    Atlas(AtlasArgs),
    /// Cut a single image into a grid of smaller segments
    #[command(
        after_help = "Examples:\n  img_tool cut -i input.png -o ./out --segments 4x4\n  img_tool cut -i input.png -o ./out --segments 3x2 -c default\n  img_tool cut -i input.png -o ./out -s 2x2 --name tile_$col-$row --index-start 1"
    )]
    Cut(CutArgs),
}

// ---------- resize ----------

/// Batch resize images with aspect-ratio control, multi-threaded via rayon.
///
/// Supports keep (center + transparent pad) and stretch modes.
/// Output preserves the original filename, converting non-PNG formats to PNG.
#[derive(clap::Args, Debug)]
struct ResizeArgs {
    /// Input image file or directory. If a directory, all files matching --filter are processed.
    #[arg(long, short, default_value = ".", verbatim_doc_comment)]
    input: PathBuf,

    /// Glob pattern for scanning input directory (e.g. "*.png", "*.jpg").
    #[arg(long, short, default_value = "*.png", verbatim_doc_comment)]
    filter: String,

    /// Output directory. Created if it doesn't exist. Original filenames are preserved.
    #[arg(long, short, verbatim_doc_comment)]
    output: PathBuf,

    /// Target size in WxH format (e.g. "256x256", "512x512").
    #[arg(long, short, default_value = "256x256", verbatim_doc_comment)]
    size: String,

    /// Aspect ratio handling: "keep" centers the image with transparent padding,
    /// "stretch" deforms the image to exactly fill the target size.
    #[arg(long, short, default_value = "keep", verbatim_doc_comment)]
    aspect: String,

    /// PNG compression level: "best", "default", "fast", or a number 0–9.
    #[arg(long, short, default_value = "best", verbatim_doc_comment)]
    compression: String,
}

fn run_resize(args: ResizeArgs) -> Result<(), Box<dyn std::error::Error>> {
    let size = parse_size(&args.size)?;
    let compression = parse_compression(&args.compression)?;
    let files = collect_files(&args.input, &args.filter)?;

    if files.is_empty() {
        eprintln!(
            "No files matching '{}' found in '{}'",
            args.filter,
            args.input.display()
        );
        return Ok(());
    }

    println!(
        "Resizing {} file(s) to {}x{}, aspect={}, output='{}'",
        files.len(),
        size.0,
        size.1,
        args.aspect,
        args.output.display()
    );

    let results: Vec<_> = files
        .par_iter()
        .map(|file| {
            let status = match resize_image(file, &args.output, size, &args.aspect, compression) {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("FAILED: {}", e),
            };
            (file.display().to_string(), status)
        })
        .collect();

    for (name, status) in &results {
        println!("  {} ... {}", name, status);
    }

    println!("Done.");
    Ok(())
}

// ---------- atlas ----------

/// Pack images into a single sprite atlas.
///
/// Two modes:
/// - Tight packing (no --resize): preserves original dimensions, packs tightly row-by-row.
/// - Uniform grid (with --resize): all images resized to same cell size first.
///
/// Auto-sizing: atlas dimensions are rounded up to the nearest multiple of 512, always square.
#[derive(clap::Args, Debug)]
struct AtlasArgs {
    /// Input directory containing source images (must be a directory).
    #[arg(long, short, default_value = ".", verbatim_doc_comment)]
    input: PathBuf,

    /// Output atlas image path (e.g. "atlas.png").
    #[arg(long, short, verbatim_doc_comment)]
    output: PathBuf,

    /// Glob pattern for scanning input directory (e.g. "*.png").
    #[arg(long, short, default_value = "*.png", verbatim_doc_comment)]
    filter: String,

    /// Enable per-image resize before packing (uniform grid mode).
    /// Without this flag, tight packing is used, preserving original dimensions.
    #[arg(long, short, default_value_t = false, verbatim_doc_comment)]
    resize: bool,

    /// Cell size WxH for uniform grid mode (e.g. "256x256"). Requires --resize.
    #[arg(long, short, default_value = "256x256", verbatim_doc_comment)]
    size: String,

    /// Aspect ratio for resize: "keep" (center + transparent pad) or "stretch". Requires --resize.
    #[arg(long, short, default_value = "keep", verbatim_doc_comment)]
    aspect: String,

    /// Gap between cells in WxH format (e.g. "10x10"). Requires --resize.
    #[arg(long, default_value = "0x0", verbatim_doc_comment)]
    spacing: String,

    /// Fixed grid layout as "cols x rows" (e.g. "8x8", "4x3").
    /// When omitted, the grid is computed automatically.
    #[arg(long, verbatim_doc_comment)]
    frames: Option<String>,

    /// Force final atlas dimensions WxH (e.g. "4096x4096").
    /// When omitted, dimensions are auto-calculated.
    #[arg(long, verbatim_doc_comment)]
    atlas_size: Option<String>,

    /// PNG compression: "best", "default", "fast", or a number 0–9.
    #[arg(long, short, default_value = "best", verbatim_doc_comment)]
    compression: String,
}

fn run_atlas(args: AtlasArgs) -> Result<(), Box<dyn std::error::Error>> {
    // warn about resize-dependent params without --resize
    let mut warnings: Vec<&str> = Vec::new();
    if !args.resize {
        if args.size != "256x256" {
            warnings.push("--size");
        }
        if args.aspect != "keep" {
            warnings.push("--aspect");
        }
        if args.spacing != "0x0" {
            warnings.push("--spacing");
        }
    }
    for name in &warnings {
        eprintln!("Warning: {} is set but --resize is off, ignoring", name);
    }

    let config = AtlasConfig {
        input: args.input,
        output: args.output,
        filter: args.filter,
        do_resize: args.resize,
        resize_size: parse_size(&args.size)?,
        resize_aspect: args.aspect,
        spacing: parse_size(&args.spacing)?,
        frames: args.frames.as_deref().map(parse_size).transpose()?,
        atlas_size: args.atlas_size.as_deref().map(parse_size).transpose()?,
        compression: parse_compression(&args.compression)?,
    };

    img_atlas::run_atlas(config)
}

// ---------- cut ----------

/// Cut a single image into a grid of smaller segments.
///
/// Splits the input image into `segments.0` columns × `segments.1` rows.
///
/// Output naming is controlled by `--name` with placeholders:
/// - `$col` — column index (0-based)
/// - `$row` — row index (0-based)
/// - `$index` — sequential index starting at `--index-start`
///
/// If `--name` does not contain any placeholder, defaults to `{stem}-$index`.
/// Characters illegal on Windows/macOS are rejected at startup.
#[derive(clap::Args, Debug)]
struct CutArgs {
    /// Input image file (must be a single file).
    #[arg(long, short, default_value = ".", verbatim_doc_comment)]
    input: PathBuf,

    /// Output directory. Created if it doesn't exist.
    #[arg(long, short, verbatim_doc_comment)]
    output: PathBuf,

    /// Grid size in ColsxRows format (e.g. "4x4", "3x2").
    #[arg(long, short, default_value = "4x4", verbatim_doc_comment)]
    segments: String,

    /// Output filename template. Supports $col, $row, $index placeholders.
    /// If none of these appear, falls back to "{stem}-$index".
    /// Example: "tile_$col-$row"  →  tile_0-0.png, tile_1-0.png, …
    /// Example: "slice-$index"     →  slice-0.png, slice-1.png, …
    /// The ".png" extension is appended automatically.
    #[arg(long, verbatim_doc_comment)]
    name: Option<String>,

    /// Starting value for the $index placeholder.
    #[arg(long, default_value = "0", verbatim_doc_comment)]
    index_start: u32,

    /// PNG compression level: "best", "default", "fast", or a number 0–9.
    #[arg(long, short, default_value = "best", verbatim_doc_comment)]
    compression: String,
}

fn run_cut(args: CutArgs) -> Result<(), Box<dyn std::error::Error>> {
    if !args.input.is_file() {
        return Err(format!(
            "Cut --input must be a single file, got: {}",
            args.input.display()
        )
        .into());
    }

    let segments = parse_size(&args.segments)?;
    let compression = parse_compression(&args.compression)?;

    println!(
        "Cutting '{}' into {}x{} segments, output='{}'",
        args.input.display(),
        segments.0,
        segments.1,
        args.output.display()
    );

    img_cut::cut_image(
        &args.input,
        &args.output,
        segments,
        compression,
        args.name.as_deref(),
        args.index_start,
    )?;

    println!("Done.");
    Ok(())
}

// ---------- main ----------

fn parse_compression(s: &str) -> Result<CompressionType, Box<dyn std::error::Error>> {
    match s.to_lowercase().as_str() {
        "best" => Ok(CompressionType::Best),
        "default" => Ok(CompressionType::Default),
        "fast" => Ok(CompressionType::Fast),
        _ => {
            // Try parsing as a numeric level (0-9)
            if let Ok(level) = s.parse::<u8>() {
                Ok(CompressionType::Level(level))
            } else {
                Err(format!(
                    "Unknown compression '{}', expected: best, default, fast, or a number 0-9",
                    s
                )
                .into())
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Resize(args) => run_resize(args),
        Command::Atlas(args) => run_atlas(args),
        Command::Cut(args) => run_cut(args),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

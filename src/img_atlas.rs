use std::path::PathBuf;

use glob::glob;
use image::{
    DynamicImage, ImageBuffer, ImageFormat, Rgba, RgbaImage, codecs::png::CompressionType,
};
use rayon::prelude::*;

use crate::img_resize;

// ---------- natural sort helpers ----------

mod natord {
    use std::cmp::Ordering;

    /// Natural-order comparison: splits strings into numeric & non-numeric chunks,
    /// compares numeric chunks by value (so "2" < "10").
    pub fn compare(a: &str, b: &str) -> Ordering {
        let a_chunks = split_chunks(a);
        let b_chunks = split_chunks(b);
        for (ca, cb) in a_chunks.iter().zip(b_chunks.iter()) {
            let ord = match (ca, cb) {
                (Chunk::Num(na), Chunk::Num(nb)) => na.cmp(nb),
                (Chunk::Num(_), Chunk::Str(_)) => Ordering::Less,
                (Chunk::Str(_), Chunk::Num(_)) => Ordering::Greater,
                (Chunk::Str(sa), Chunk::Str(sb)) => sa.cmp(sb),
            };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        a_chunks.len().cmp(&b_chunks.len())
    }

    enum Chunk<'a> {
        Num(u64),
        Str(&'a str),
    }

    fn split_chunks(s: &str) -> Vec<Chunk<'_>> {
        let mut chunks = Vec::new();
        let mut rest = s;
        while !rest.is_empty() {
            if let Some(pos) = rest.find(|c: char| c.is_ascii_digit()) {
                if pos > 0 {
                    chunks.push(Chunk::Str(&rest[..pos]));
                    rest = &rest[pos..];
                }
                let end = rest
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(rest.len());
                let num: u64 = rest[..end].parse().unwrap_or(0);
                chunks.push(Chunk::Num(num));
                rest = &rest[end..];
            } else {
                chunks.push(Chunk::Str(rest));
                break;
            }
        }
        chunks
    }
}

// ---------- public types ----------

/// Configuration for atlas generation.
pub struct AtlasConfig {
    pub input: PathBuf,
    pub output: PathBuf,
    pub filter: String,
    pub do_resize: bool,
    pub resize_size: (u32, u32),
    pub resize_aspect: String,
    pub spacing: (u32, u32),
    pub frames: Option<(u32, u32)>,
    pub atlas_size: Option<(u32, u32)>,
    pub compression: CompressionType,
}

// ---------- internal types ----------

/// A loaded (and optionally already resized) image ready to be placed.
struct ImageItem {
    image: DynamicImage,
    name: String,
}

// ---------- public entry point ----------

pub fn run_atlas(config: AtlasConfig) -> Result<(), Box<dyn std::error::Error>> {
    // 1. validate
    if !config.input.is_dir() {
        return Err("Atlas --input must be a directory".into());
    }

    // 2. collect files (natural sorted order)
    let pattern = format!("{}/{}", config.input.display(), config.filter);
    let mut paths: Vec<PathBuf> = glob(&pattern)?
        .filter_map(|e| e.ok())
        .filter(|p| p.is_file())
        .collect();
    paths.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));

    if paths.is_empty() {
        eprintln!(
            "No files matching '{}' found in '{}'",
            config.filter,
            config.input.display()
        );
        return Ok(());
    }

    println!("Found {} image(s), loading...", paths.len());

    // 3. load images sequentially in natural order
    let items: Vec<ImageItem> = paths
        .iter()
        .map(|p| {
            let img = image::open(p).expect("Failed to open image");
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            ImageItem { image: img, name }
        })
        .collect();

    if config.do_resize {
        run_atlas_with_resize(items, config)
    } else {
        run_atlas_tight(items, config)
    }
}

// ==================== resize path (uniform cells) ====================

fn run_atlas_with_resize(
    items: Vec<ImageItem>,
    config: AtlasConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Resizing to {}x{} (aspect={})...",
        config.resize_size.0, config.resize_size.1, config.resize_aspect
    );

    let pairs: Vec<(usize, ImageItem)> = items
        .into_par_iter()
        .enumerate()
        .map(|(i, it)| {
            let resized = img_resize::resize_dynamic_image(
                &it.image,
                config.resize_size,
                &config.resize_aspect,
                "png",
            )
            .expect("Failed to resize");
            (
                i,
                ImageItem {
                    image: resized,
                    ..it
                },
            )
        })
        .collect();
    // restore original order after parallel processing
    let items: Vec<ImageItem> = {
        let mut pairs = pairs;
        pairs.sort_by_key(|(i, _)| *i);
        pairs.into_iter().map(|(_, item)| item).collect()
    };

    let item_size = config.resize_size;
    let spacing = config.spacing;
    let n = items.len() as u32;
    let cell_w = item_size.0 + spacing.0;
    let cell_h = item_size.1 + spacing.1;

    let (cols, rows) = compute_grid(n, config.frames);
    let (atlas_w, atlas_h) =
        compute_atlas_dims(cols, rows, cell_w, cell_h, spacing, config.atlas_size);

    println!(
        "Atlas {}x{}  grid {}x{}  cell {}x{}  spacing {}x{}",
        atlas_w, atlas_h, cols, rows, item_size.0, item_size.1, spacing.0, spacing.1,
    );

    // Compute per-item positions from grid
    let positions: Vec<(u32, u32)> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let col = i as u32 % cols;
            let row = i as u32 / cols;
            let x = spacing.0 + col * cell_w + (item_size.0.saturating_sub(item.image.width())) / 2;
            let y =
                spacing.1 + row * cell_h + (item_size.1.saturating_sub(item.image.height())) / 2;
            (x, y)
        })
        .collect();

    let atlas_img = stitch_at_positions(&items, &positions, atlas_w, atlas_h);
    save_atlas(atlas_img, &config.output, config.compression)
}

// ==================== tight-packing path (no resize) ====================

fn run_atlas_tight(
    mut items: Vec<ImageItem>,
    config: AtlasConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    // Sort by height descending for better space utilisation
    items.sort_by_key(|it| std::cmp::Reverse(it.image.height()));

    let n = items.len() as u32;
    let cols = match config.frames {
        Some((fc, _)) => fc,
        None => (n as f64).sqrt().ceil() as u32,
    };

    // Build rows: each row contains indices into items, plus the row's height (tallest image)
    struct Row {
        indices: Vec<usize>,
        height: u32,
        width: u32,
    }
    let mut rows: Vec<Row> = Vec::new();

    for chunk in (0..items.len()).collect::<Vec<_>>().chunks(cols as usize) {
        let height = chunk
            .iter()
            .map(|&i| items[i].image.height())
            .max()
            .unwrap_or(0);
        let width: u32 = chunk.iter().map(|&i| items[i].image.width()).sum();
        rows.push(Row {
            indices: chunk.to_vec(),
            height,
            width,
        });
    }

    let content_w = rows.iter().map(|r| r.width).max().unwrap_or(1);
    let content_h: u32 = rows.iter().map(|r| r.height).sum();

    let (atlas_w, atlas_h) = if let Some(size) = config.atlas_size {
        size
    } else {
        round_to_512_square(content_w.max(content_h))
    };

    let rows_count = rows.len();
    println!(
        "Atlas {}x{}  rows {}  cols {}  tight={}x{}",
        atlas_w, atlas_h, rows_count, cols, content_w, content_h,
    );

    // Compute per-item positions
    let mut positions: Vec<(u32, u32)> = vec![(0, 0); items.len()];
    let mut y: u32 = 0;
    for row in &rows {
        let mut x: u32 = 0;
        for &idx in &row.indices {
            let h = items[idx].image.height();
            // Center vertically within the row
            positions[idx] = (x, y + (row.height - h) / 2);
            x += items[idx].image.width();
        }
        y += row.height;
    }

    let atlas_img = stitch_at_positions(&items, &positions, atlas_w, atlas_h);
    save_atlas(atlas_img, &config.output, config.compression)
}

// ---------- layout helpers ----------

fn compute_grid(n: u32, frames: Option<(u32, u32)>) -> (u32, u32) {
    match frames {
        Some((fc, fr)) => {
            let needed_rows = (n + fc - 1) / fc;
            let rows = needed_rows.max(fr);
            (fc, rows)
        }
        None => {
            let side = (n as f64).sqrt().ceil() as u32;
            (side, side)
        }
    }
}

/// Compute atlas dimensions for uniform-cell mode.
fn compute_atlas_dims(
    cols: u32,
    rows: u32,
    cell_w: u32,
    cell_h: u32,
    spacing: (u32, u32),
    forced: Option<(u32, u32)>,
) -> (u32, u32) {
    if let Some(size) = forced {
        return size;
    }
    let raw_w = spacing.0 + cols * cell_w;
    let raw_h = spacing.1 + rows * cell_h;
    let max_dim = raw_w.max(raw_h);
    let rounded = ((max_dim + 511) / 512) * 512;
    (rounded, rounded)
}

/// Round a dimension up to the nearest multiple of 512 → square atlas.
fn round_to_512_square(dim: u32) -> (u32, u32) {
    let rounded = ((dim + 511) / 512) * 512;
    (rounded, rounded)
}

// ---------- stitching ----------

/// Stitch items into a canvas using pre-computed (x, y) positions.
fn stitch_at_positions(
    items: &[ImageItem],
    positions: &[(u32, u32)],
    atlas_w: u32,
    atlas_h: u32,
) -> DynamicImage {
    let fill = Rgba([0, 0, 0, 0]);
    let mut canvas: RgbaImage = ImageBuffer::from_pixel(atlas_w, atlas_h, fill);

    for (item, &(x, y)) in items.iter().zip(positions.iter()) {
        if x >= atlas_w || y >= atlas_h {
            eprintln!("Warning: '{}' outside atlas bounds, skipped", item.name);
            continue;
        }
        let rgba = item.image.to_rgba8();
        let copy_w = item.image.width().min(atlas_w - x);
        let copy_h = item.image.height().min(atlas_h - y);
        for py in 0..copy_h {
            for px in 0..copy_w {
                canvas.put_pixel(x + px, y + py, *rgba.get_pixel(px, py));
            }
        }
    }

    DynamicImage::ImageRgba8(canvas)
}

// ---------- output ----------

fn save_atlas(
    atlas_img: DynamicImage,
    output: &PathBuf,
    compression: CompressionType,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("png");
    match ext {
        "jpg" | "jpeg" => {
            atlas_img.save_with_format(output, ImageFormat::Jpeg)?;
        }
        _ => {
            let file = std::fs::File::create(output)?;
            let writer = std::io::BufWriter::new(file);
            let encoder = image::codecs::png::PngEncoder::new_with_quality(
                writer,
                compression,
                image::codecs::png::FilterType::Adaptive,
            );
            atlas_img.write_with_encoder(encoder)?;
        }
    }
    println!("Saved → {}", output.display());
    Ok(())
}

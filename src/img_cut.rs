use std::path::Path;

use image::{DynamicImage, GenericImageView, codecs::png::CompressionType};

/// Cut a single image into a grid of segments and save them to the output directory.
///
/// The image is divided into `segments.0` columns × `segments.1` rows.
///
/// `name_template` supports three variables:
/// - `$col`  — column index (0-based)
/// - `$row`  — row index (0-based)
/// - `$index` — sequential index starting at `index_start`
///
/// If the template does not contain any of these variables,
/// `{file_stem}-$index` is used as the fallback.
///
/// After resolving the template, a trial substitution is done with zeros;
/// if the resulting string contains characters illegal on Windows/macOS
/// (`/`, `\`, `:`, `*`, `?`, `"`, `<`, `>`, `|`, or ASCII control chars),
/// an error is returned.
pub fn cut_image(
    input_path: &Path,
    output_dir: &Path,
    segments: (u32, u32),
    compression: CompressionType,
    name_template: Option<&str>,
    index_start: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let img = image::open(input_path)?;
    let (img_w, img_h) = img.dimensions();
    let (cols, rows) = segments;

    if cols == 0 || rows == 0 {
        return Err("Segments must be at least 1x1".into());
    }
    if img_w < cols || img_h < rows {
        return Err(format!(
            "Image dimensions {}x{} are smaller than segment count {}x{}",
            img_w, img_h, cols, rows
        )
        .into());
    }

    let file_stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("cut");

    // Resolve the effective name template
    let template = resolve_name_template(name_template, file_stem);

    // Trial-substitute with (col=0, row=0, index=0) and validate
    let trial_name = substitute(&template, 0, 0, index_start);
    validate_filename(&trial_name)?;

    std::fs::create_dir_all(output_dir)?;

    let seg_w = img_w / cols;
    let seg_h = img_h / rows;

    let mut index = index_start;

    for row in 0..rows {
        for col in 0..cols {
            let x = col * seg_w;
            let y = row * seg_h;
            // Last column/row absorbs the remainder
            let w = if col == cols - 1 { img_w - x } else { seg_w };
            let h = if row == rows - 1 { img_h - y } else { seg_h };

            let sub = img.crop_imm(x, y, w, h);
            let out_name = format!("{}.png", substitute(&template, col, row, index));
            let out_path = output_dir.join(out_name);

            save_png(
                &DynamicImage::ImageRgba8(sub.to_rgba8()),
                &out_path,
                compression,
            )?;

            index += 1;
        }
    }

    Ok(())
}

// ---------- name template helpers ----------

/// Resolve the effective template string.
///
/// If `name_template` is `Some(t)` and `t` contains at least one of
/// `$col`, `$row`, `$index`, use `t` verbatim. Otherwise fall back
/// to `"{stem}-$index"`.
fn resolve_name_template(name_template: Option<&str>, file_stem: &str) -> String {
    if let Some(tmpl) = name_template {
        if tmpl.contains("$col") || tmpl.contains("$row") || tmpl.contains("$index") {
            return tmpl.to_string();
        }
    }
    format!("{}-$index", file_stem)
}

/// Substitute `$col`, `$row`, `$index` placeholders in the template.
fn substitute(template: &str, col: u32, row: u32, index: u32) -> String {
    template
        .replace("$col", &col.to_string())
        .replace("$row", &row.to_string())
        .replace("$index", &index.to_string())
}

/// Characters forbidden in filenames on Windows and/or macOS.
const ILLEGAL_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Validate that `name` contains no platform-illegal characters or
/// ASCII control characters.
fn validate_filename(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if name.is_empty() {
        return Err("Filename is empty after template substitution".into());
    }
    for ch in name.chars() {
        if ILLEGAL_CHARS.contains(&ch) {
            return Err(format!("Illegal character '{}' in filename: {}", ch, name).into());
        }
        if ch.is_ascii_control() {
            return Err(format!(
                "Control character U+{:04X} in filename: {}",
                ch as u32, name
            )
            .into());
        }
    }
    Ok(())
}

/// Save a DynamicImage as PNG with the given compression level.
fn save_png(
    img: &DynamicImage,
    path: &Path,
    compression: CompressionType,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        writer,
        compression,
        image::codecs::png::FilterType::Adaptive,
    );
    img.write_with_encoder(encoder)?;
    Ok(())
}

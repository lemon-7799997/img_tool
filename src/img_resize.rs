use std::path::{Path, PathBuf};

use glob::glob;
use image::{
    DynamicImage, GenericImageView, ImageBuffer, ImageFormat, Rgba, codecs::png::CompressionType,
};

/// Resize a single image file and save to output directory.
pub fn resize_image(
    input_path: &Path,
    output_dir: &Path,
    size: (u32, u32),
    aspect: &str,
    compression: CompressionType,
) -> Result<(), Box<dyn std::error::Error>> {
    let img = image::open(input_path)?;
    let (w, h) = size;
    let ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();

    let resized = match aspect {
        "keep" => resize_keep(&img, w, h, &ext),
        "stretch" => resize_stretch(&img, w, h),
        _ => return Err(format!("Unknown aspect mode: {}", aspect).into()),
    };

    // Build output path preserving filename
    let file_stem = input_path.file_stem().unwrap().to_str().unwrap();
    let out_ext = if ext == "jpg" || ext == "jpeg" {
        "jpg"
    } else {
        "png"
    };
    let out_name = format!("{}.{}", file_stem, out_ext);
    let out_path = output_dir.join(out_name);

    // Ensure output directory exists
    std::fs::create_dir_all(output_dir)?;

    match out_ext {
        "jpg" | "jpeg" => {
            resized.save_with_format(out_path, ImageFormat::Jpeg)?;
        }
        _ => {
            save_png(&resized, &out_path, compression)?;
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

/// Resize to exact dimensions (stretch).
pub fn resize_stretch(img: &DynamicImage, w: u32, h: u32) -> DynamicImage {
    img.resize_exact(w, h, image::imageops::FilterType::Lanczos3)
}

/// Resize keeping aspect ratio, center on a transparent/solid canvas.
/// For PNG output, uses transparent background; for others, white background.
pub fn resize_keep(img: &DynamicImage, target_w: u32, target_h: u32, ext: &str) -> DynamicImage {
    let (iw, ih) = img.dimensions();
    let scale = (target_w as f64 / iw as f64).min(target_h as f64 / ih as f64);
    let new_w = (iw as f64 * scale).round() as u32;
    let new_h = (ih as f64 * scale).round() as u32;

    let scaled = img.resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3);

    let offset_x = (target_w - new_w) / 2;
    let offset_y = (target_h - new_h) / 2;

    let is_png = ext == "png";
    let fill_color: Rgba<u8> = if is_png {
        Rgba([0, 0, 0, 0])
    } else {
        Rgba([255, 255, 255, 255])
    };

    let mut canvas: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(target_w, target_h, fill_color);

    for y in 0..new_h {
        for x in 0..new_w {
            let pixel = scaled.get_pixel(x, y);
            canvas.put_pixel(offset_x + x, offset_y + y, pixel);
        }
    }

    DynamicImage::ImageRgba8(canvas)
}

/// Resize a DynamicImage in memory (for atlas pipeline reuse).
pub fn resize_dynamic_image(
    img: &DynamicImage,
    size: (u32, u32),
    aspect: &str,
    ext: &str,
) -> Result<DynamicImage, Box<dyn std::error::Error>> {
    match aspect {
        "keep" => Ok(resize_keep(img, size.0, size.1, ext)),
        "stretch" => Ok(resize_stretch(img, size.0, size.1)),
        _ => Err(format!("Unknown aspect mode: {}", aspect).into()),
    }
}

/// Collect all matching image files from input path.
/// If `input` is a file, returns it directly (if it passes the filter).
/// If `input` is a directory, scans with the given glob pattern.
pub fn collect_files(
    input: &Path,
    filter: &str,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    if input.is_file() {
        // For a single file, check if it matches the filter extension
        let file_name = input.file_name().unwrap().to_str().unwrap();
        if glob_match(filter, file_name) {
            return Ok(vec![input.to_path_buf()]);
        }
        return Ok(vec![]);
    }

    let pattern = format!("{}/{}", input.display(), filter);
    let paths: Vec<PathBuf> = glob(&pattern)?
        .filter_map(|entry| entry.ok())
        .filter(|p| p.is_file())
        .collect();

    Ok(paths)
}

/// Simple glob-style matching for a single filename against a pattern.
fn glob_match(pattern: &str, name: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let name = name.to_lowercase();
    if pattern == "*.*" {
        return name.contains('.');
    }
    if let Some(ext) = pattern.strip_prefix("*.") {
        return name.ends_with(&format!(".{}", ext));
    }
    if pattern == "*" {
        return true;
    }
    name == pattern
}

/// Parse a "WxH" size string into (width, height).
pub fn parse_size(s: &str) -> Result<(u32, u32), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = s.split(|c| c == 'x' || c == 'X').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid size format '{}', expected WxH (e.g. 256x256)", s).into());
    }
    let w: u32 = parts[0].trim().parse()?;
    let h: u32 = parts[1].trim().parse()?;
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("256x256").unwrap(), (256, 256));
        assert_eq!(parse_size("128x64").unwrap(), (128, 64));
        assert_eq!(parse_size("512X384").unwrap(), (512, 384));
        assert!(parse_size("abc").is_err());
        assert!(parse_size("128x").is_err());
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.png", "image.png"));
        assert!(glob_match("*.png", "image.PNG"));
        assert!(!glob_match("*.png", "image.jpg"));
        assert!(glob_match("*.*", "image.png"));
        assert!(glob_match("*", "anything"));
    }
}

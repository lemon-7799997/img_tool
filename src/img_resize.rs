use std::path::{Path, PathBuf};

use glob::glob;
use image::{
    DynamicImage, GenericImageView, ImageBuffer, ImageFormat, Rgba, codecs::png::CompressionType,
    imageops::FilterType,
};

/// How the source image is scaled to fit the requested WxH canvas.
///
/// In every mode the output canvas is exactly the requested WxH; any space
/// the image does not cover is filled (transparent for PNG, black for JPG).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleMode {
    /// Deform the image to exactly fill the canvas (aspect ratio lost).
    Scale,
    /// Never upscale: when the source fits inside the canvas (small → large)
    /// its original pixel size is kept and the leftover is filled; when the
    /// source is larger than the canvas it behaves like `KeepAspect`.
    Keep,
    /// Scale to fit inside the canvas preserving the aspect ratio,
    /// both when enlarging and when shrinking.
    KeepAspect,
}

impl ScaleMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ScaleMode::Scale => "scale",
            ScaleMode::Keep => "keep",
            ScaleMode::KeepAspect => "keep-aspect",
        }
    }
}

/// Horizontal placement of the image within the leftover canvas area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

/// Vertical placement of the image within the leftover canvas area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

/// 9-direction placement of the image within the leftover canvas area,
/// represented as an independent horizontal + vertical component pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Align {
    pub h: HAlign,
    pub v: VAlign,
}

impl Align {
    /// Centered alignment (used by the test suite and callers that
    /// construct an `Align` directly).
    #[allow(dead_code)]
    pub const CENTER: Align = Align {
        h: HAlign::Center,
        v: VAlign::Center,
    };

    /// Convenience constructor (used by the test suite and future callers).
    #[allow(dead_code)]
    pub const fn new(h: HAlign, v: VAlign) -> Align {
        Align { h, v }
    }

    /// Compute the top-left placement offset inside a `pad_x x pad_y`
    /// leftover area.
    fn offset(self, pad_x: u32, pad_y: u32) -> (u32, u32) {
        let ox = match self.h {
            HAlign::Left => 0,
            HAlign::Center => pad_x / 2,
            HAlign::Right => pad_x,
        };
        let oy = match self.v {
            VAlign::Top => 0,
            VAlign::Center => pad_y / 2,
            VAlign::Bottom => pad_y,
        };
        (ox, oy)
    }

    /// Canonical hyphenated name, e.g. `top-left`. Centered axes are
    /// omitted: `top`, `left`, `center`.
    pub fn name(self) -> String {
        let h = match self.h {
            HAlign::Left => "left",
            HAlign::Center => "",
            HAlign::Right => "right",
        };
        let v = match self.v {
            VAlign::Top => "top",
            VAlign::Center => "",
            VAlign::Bottom => "bottom",
        };
        match (h.is_empty(), v.is_empty()) {
            (true, true) => "center".to_string(),
            (true, false) => v.to_string(),
            (false, true) => h.to_string(),
            (false, false) => format!("{}-{}", v, h),
        }
    }
}

/// Merge a horizontal direction into the accumulated parse result.
/// A `Center` placeholder is treated as "not yet chosen" so a later
/// concrete direction (e.g. "center-left") overrides it.
fn merge_h(h: &mut Option<HAlign>, dir: HAlign, what: &str) -> Result<(), Box<dyn std::error::Error>> {
    match *h {
        None | Some(HAlign::Center) => {
            *h = Some(dir);
            Ok(())
        }
        Some(cur) if cur == dir => Ok(()),
        Some(_) => Err(format!(
            "Conflicting horizontal directions in align '{}'",
            what
        )
        .into()),
    }
}

/// Merge a vertical direction into the accumulated parse result.
/// See [`merge_h`].
fn merge_v(v: &mut Option<VAlign>, dir: VAlign, what: &str) -> Result<(), Box<dyn std::error::Error>> {
    match *v {
        None | Some(VAlign::Center) => {
            *v = Some(dir);
            Ok(())
        }
        Some(cur) if cur == dir => Ok(()),
        Some(_) => Err(format!(
            "Conflicting vertical directions in align '{}'",
            what
        )
        .into()),
    }
}

/// Parse a --stretch value ("scale", "keep", "keep-aspect").
pub fn parse_stretch(s: &str) -> Result<ScaleMode, Box<dyn std::error::Error>> {
    match s.to_lowercase().as_str() {
        "scale" => Ok(ScaleMode::Scale),
        "keep" => Ok(ScaleMode::Keep),
        "keep-aspect" => Ok(ScaleMode::KeepAspect),
        _ => Err(format!(
            "Unknown stretch mode '{}', expected: scale, keep, keep-aspect",
            s
        )
        .into()),
    }
}

/// Parse an --align value: split on '-' into a vertical and a horizontal
/// part, in either order (e.g. both "top-left" and "left-top" work).
/// Parts are: top, bottom (vertical) / left, right (horizontal) /
/// center (fills whichever axis is not given).
/// A missing axis defaults to center: "top" == "top-center", "left" == "left-center".
pub fn parse_align(s: &str) -> Result<Align, Box<dyn std::error::Error>> {
    let mut h: Option<HAlign> = None;
    let mut v: Option<VAlign> = None;

    for part in s.split('-') {
        let part = part.trim().to_lowercase();
        match part.as_str() {
            "center" => {
                // Center fills whichever axis is still unset.
                if h.is_none() {
                    h = Some(HAlign::Center);
                }
                if v.is_none() {
                    v = Some(VAlign::Center);
                }
            }
            "left" => merge_h(&mut h, HAlign::Left, s)?,
            "right" => merge_h(&mut h, HAlign::Right, s)?,
            "top" => merge_v(&mut v, VAlign::Top, s)?,
            "bottom" => merge_v(&mut v, VAlign::Bottom, s)?,
            _ => {
                return Err(format!(
                    "Unknown align part '{}' in '{}', expected parts: top, bottom, \
                     left, right, center (e.g. top-left, left-top, bottom-right, center)",
                    part, s
                )
                .into())
            }
        }
    }

    Ok(Align {
        h: h.unwrap_or(HAlign::Center),
        v: v.unwrap_or(VAlign::Center),
    })
}

/// Resize a single image file and save to output directory.
///
/// Output format follows the source extension: jpg/jpeg sources are saved as
/// JPG (leftover filled black), everything else is saved as PNG (leftover
/// transparent). Original filenames are preserved.
pub fn resize_image(
    input_path: &Path,
    output_dir: &Path,
    size: (u32, u32),
    mode: ScaleMode,
    align: Align,
    compression: CompressionType,
) -> Result<(), Box<dyn std::error::Error>> {
    let img = image::open(input_path)?;
    let (w, h) = size;
    let ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let out_ext = if ext == "jpg" || ext == "jpeg" {
        "jpg"
    } else {
        "png"
    };

    let resized = resize_onto_canvas(&img, w, h, mode, align, fill_for_ext(out_ext));

    // Build output path preserving filename
    let file_stem = input_path.file_stem().unwrap().to_str().unwrap();
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

/// Resize a DynamicImage in memory (for atlas pipeline reuse).
pub fn resize_dynamic_image(
    img: &DynamicImage,
    size: (u32, u32),
    mode: ScaleMode,
    align: Align,
    ext: &str,
) -> DynamicImage {
    resize_onto_canvas(img, size.0, size.1, mode, align, fill_for_ext(ext))
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

/// Fill color for the leftover canvas area: transparent for PNG output,
/// black (opaque) for JPG output (JPEG has no alpha channel).
fn fill_for_ext(ext: &str) -> Rgba<u8> {
    match ext {
        "jpg" | "jpeg" => Rgba([0, 0, 0, 255]),
        _ => Rgba([0, 0, 0, 0]),
    }
}

/// Fit dimensions inside `w x h` preserving aspect ratio (contain).
fn contain_size(iw: u32, ih: u32, w: u32, h: u32) -> (u32, u32) {
    let f = (w as f64 / iw as f64).min(h as f64 / ih as f64);
    let dw = ((iw as f64 * f).round() as u32).max(1);
    let dh = ((ih as f64 * f).round() as u32).max(1);
    (dw.min(w), dh.min(h))
}

/// Layout result: scaled size plus top-left placement on the canvas.
#[derive(Debug, PartialEq, Eq)]
struct Layout {
    dw: u32,
    dh: u32,
    ox: u32,
    oy: u32,
}

/// Decide how big the image is drawn and where it is placed.
fn compute_layout(iw: u32, ih: u32, w: u32, h: u32, mode: ScaleMode, align: Align) -> Layout {
    let (dw, dh) = match mode {
        // Fill the whole canvas.
        ScaleMode::Scale => (w, h),
        // Always contain-fit.
        ScaleMode::KeepAspect => contain_size(iw, ih, w, h),
        // Keep native pixels when the image fits inside the canvas
        // (small → large); otherwise fall back to contain-fit (large → small).
        ScaleMode::Keep => {
            if iw <= w && ih <= h {
                (iw, ih)
            } else {
                contain_size(iw, ih, w, h)
            }
        }
    };

    let pad_x = w - dw;
    let pad_y = h - dh;
    let (ox, oy) = align.offset(pad_x, pad_y);
    Layout { dw, dh, ox, oy }
}

/// Draw `img` onto a `w x h` canvas using the given mode/align/fill color.
fn resize_onto_canvas(
    img: &DynamicImage,
    w: u32,
    h: u32,
    mode: ScaleMode,
    align: Align,
    fill: Rgba<u8>,
) -> DynamicImage {
    let (iw, ih) = img.dimensions();
    let lay = compute_layout(iw, ih, w, h, mode, align);

    let mut canvas: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_pixel(w, h, fill);

    let src = if lay.dw == iw && lay.dh == ih {
        img.to_rgba8()
    } else {
        img.resize_exact(lay.dw, lay.dh, FilterType::Lanczos3)
            .to_rgba8()
    };

    for y in 0..lay.dh {
        for x in 0..lay.dw {
            canvas.put_pixel(lay.ox + x, lay.oy + y, *src.get_pixel(x, y));
        }
    }

    DynamicImage::ImageRgba8(canvas)
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

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
    const TRANSPARENT: Rgba<u8> = Rgba([0, 0, 0, 0]);
    const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);

    fn solid(w: u32, h: u32, color: Rgba<u8>) -> DynamicImage {
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(w, h, color))
    }

    fn px(img: &DynamicImage, x: u32, y: u32) -> Rgba<u8> {
        img.get_pixel(x, y)
    }

    // ---------- parsing ----------

    #[test]
    fn test_parse_stretch() {
        assert_eq!(parse_stretch("scale").unwrap(), ScaleMode::Scale);
        assert_eq!(parse_stretch("keep").unwrap(), ScaleMode::Keep);
        assert_eq!(parse_stretch("keep-aspect").unwrap(), ScaleMode::KeepAspect);
        assert_eq!(parse_stretch("KEEP").unwrap(), ScaleMode::Keep);
        assert!(parse_stretch("stretch").is_err());
        assert!(parse_stretch("").is_err());
        // as_str round-trips
        for m in [ScaleMode::Scale, ScaleMode::Keep, ScaleMode::KeepAspect] {
            assert_eq!(parse_stretch(m.as_str()).unwrap(), m);
        }
    }

    #[test]
    fn test_parse_align() {
        // center alone centers both axes
        assert_eq!(parse_align("center").unwrap(), Align::CENTER);
        assert_eq!(parse_align("CENTER").unwrap(), Align::CENTER);
        // single part -> the other axis defaults to center
        assert_eq!(
            parse_align("top").unwrap(),
            Align::new(HAlign::Center, VAlign::Top)
        );
        assert_eq!(
            parse_align("left").unwrap(),
            Align::new(HAlign::Left, VAlign::Center)
        );
        assert_eq!(
            parse_align("right").unwrap(),
            Align::new(HAlign::Right, VAlign::Center)
        );
        assert_eq!(
            parse_align("bottom").unwrap(),
            Align::new(HAlign::Center, VAlign::Bottom)
        );
        // both parts, hyphen split: order is irrelevant
        assert_eq!(
            parse_align("top-left").unwrap(),
            Align::new(HAlign::Left, VAlign::Top)
        );
        assert_eq!(
            parse_align("left-top").unwrap(),
            Align::new(HAlign::Left, VAlign::Top)
        );
        assert_eq!(
            parse_align("top-right").unwrap(),
            Align::new(HAlign::Right, VAlign::Top)
        );
        assert_eq!(
            parse_align("right-top").unwrap(),
            Align::new(HAlign::Right, VAlign::Top)
        );
        assert_eq!(
            parse_align("bottom-left").unwrap(),
            Align::new(HAlign::Left, VAlign::Bottom)
        );
        assert_eq!(
            parse_align("left-bottom").unwrap(),
            Align::new(HAlign::Left, VAlign::Bottom)
        );
        assert_eq!(
            parse_align("bottom-right").unwrap(),
            Align::new(HAlign::Right, VAlign::Bottom)
        );
        assert_eq!(
            parse_align("right-bottom").unwrap(),
            Align::new(HAlign::Right, VAlign::Bottom)
        );
        // "center" fills whichever axis is still missing, in any position
        assert_eq!(
            parse_align("top-center").unwrap(),
            Align::new(HAlign::Center, VAlign::Top)
        );
        assert_eq!(
            parse_align("center-top").unwrap(),
            Align::new(HAlign::Center, VAlign::Top)
        );
        assert_eq!(
            parse_align("left-center").unwrap(),
            Align::new(HAlign::Left, VAlign::Center)
        );
        assert_eq!(
            parse_align("center-left").unwrap(),
            Align::new(HAlign::Left, VAlign::Center)
        );
        // whitespace / case tolerance
        assert_eq!(
            parse_align(" TOP-LEFT ").unwrap(),
            Align::new(HAlign::Left, VAlign::Top)
        );
        // errors
        assert!(parse_align("topleft").is_err());
        assert!(parse_align("tl").is_err());
        assert!(parse_align("top-bottom").is_err());
        assert!(parse_align("left-right").is_err());
        assert!(parse_align("top-").is_err());
        assert!(parse_align("").is_err());
    }

    // ---------- layout ----------

    #[test]
    fn test_layout_scale_fills_canvas() {
        let lay = compute_layout(100, 50, 200, 200, ScaleMode::Scale, Align::CENTER);
        assert_eq!(lay, Layout { dw: 200, dh: 200, ox: 0, oy: 0 });
    }

    #[test]
    fn test_layout_keep_aspect_contain() {
        // 100x50 -> 200x200 contain scale 2x => 200x100, centered vertically.
        let lay = compute_layout(100, 50, 200, 200, ScaleMode::KeepAspect, Align::CENTER);
        assert_eq!(lay, Layout { dw: 200, dh: 100, ox: 0, oy: 50 });
    }

    #[test]
    fn test_layout_keep_native_when_fits() {
        // Image fully fits the canvas -> keep original pixels, any align.
        let br = compute_layout(
            100,
            50,
            200,
            200,
            ScaleMode::Keep,
            Align::new(HAlign::Right, VAlign::Bottom),
        );
        assert_eq!(br, Layout { dw: 100, dh: 50, ox: 100, oy: 150 });

        let center = compute_layout(100, 50, 200, 200, ScaleMode::Keep, Align::CENTER);
        assert_eq!(center, Layout { dw: 100, dh: 50, ox: 50, oy: 75 });

        let tl = compute_layout(
            100,
            50,
            200,
            200,
            ScaleMode::Keep,
            Align::new(HAlign::Left, VAlign::Top),
        );
        assert_eq!(tl, Layout { dw: 100, dh: 50, ox: 0, oy: 0 });
    }

    #[test]
    fn test_layout_keep_overflow_falls_back_to_keep_aspect() {
        // 300x50 wider than 200x200 -> treat as keep-aspect (shrink to fit).
        let lay = compute_layout(300, 50, 200, 200, ScaleMode::Keep, Align::CENTER);
        assert_eq!(lay, Layout { dw: 200, dh: 33, ox: 0, oy: 83 });
    }

    #[test]
    fn test_layout_keep_edge_equal_axis() {
        // Width already equals canvas width -> native, only vertical padding.
        let lay = compute_layout(200, 50, 200, 200, ScaleMode::Keep, Align::CENTER);
        assert_eq!(lay, Layout { dw: 200, dh: 50, ox: 0, oy: 75 });
    }

    #[test]
    fn test_layout_align_axis_mapping() {
        // Padding on both axes: top-left / top / right / bottom-left / bottom.
        assert_eq!(
            compute_layout(
                100,
                50,
                200,
                200,
                ScaleMode::Keep,
                Align::new(HAlign::Left, VAlign::Top)
            ),
            Layout { dw: 100, dh: 50, ox: 0, oy: 0 }
        );
        assert_eq!(
            compute_layout(
                100,
                50,
                200,
                200,
                ScaleMode::Keep,
                Align::new(HAlign::Center, VAlign::Top)
            ),
            Layout { dw: 100, dh: 50, ox: 50, oy: 0 }
        );
        assert_eq!(
            compute_layout(
                100,
                50,
                200,
                200,
                ScaleMode::Keep,
                Align::new(HAlign::Right, VAlign::Center)
            ),
            Layout { dw: 100, dh: 50, ox: 100, oy: 75 }
        );
        assert_eq!(
            compute_layout(
                100,
                50,
                200,
                200,
                ScaleMode::Keep,
                Align::new(HAlign::Left, VAlign::Bottom)
            ),
            Layout { dw: 100, dh: 50, ox: 0, oy: 150 }
        );
        assert_eq!(
            compute_layout(
                100,
                50,
                200,
                200,
                ScaleMode::Keep,
                Align::new(HAlign::Center, VAlign::Bottom)
            ),
            Layout { dw: 100, dh: 50, ox: 50, oy: 150 }
        );
    }

    // ---------- pixel-level compose checks ----------

    #[test]
    fn test_compose_keep_aspect_center_png_transparent_pad() {
        let img = solid(100, 50, RED);
        let out = resize_dynamic_image(&img, (200, 200), ScaleMode::KeepAspect, Align::CENTER, "png");
        assert_eq!(out.dimensions(), (200, 200));
        // Content rows 50..=149 red; pad above/below transparent.
        assert_eq!(px(&out, 100, 50), RED);
        assert_eq!(px(&out, 0, 149), RED);
        assert_eq!(px(&out, 0, 0), TRANSPARENT);
        assert_eq!(px(&out, 199, 199), TRANSPARENT);
        assert_eq!(px(&out, 100, 49), TRANSPARENT);
        assert_eq!(px(&out, 100, 150), TRANSPARENT);
    }

    #[test]
    fn test_compose_keep_aspect_bottom_jpg_black_pad() {
        let img = solid(100, 50, RED);
        let out = resize_dynamic_image(
            &img,
            (200, 200),
            ScaleMode::KeepAspect,
            Align::new(HAlign::Center, VAlign::Bottom),
            "jpg",
        );
        assert_eq!(out.dimensions(), (200, 200));
        // Content rows 100..=199; pad above black (opaque).
        assert_eq!(px(&out, 100, 150), RED);
        assert_eq!(px(&out, 0, 199), RED);
        assert_eq!(px(&out, 0, 0), BLACK);
        assert_eq!(px(&out, 100, 99), BLACK);
    }

    #[test]
    fn test_compose_keep_native_bottom_right_png() {
        let img = solid(100, 50, RED);
        let out = resize_dynamic_image(
            &img,
            (200, 200),
            ScaleMode::Keep,
            Align::new(HAlign::Right, VAlign::Bottom),
            "png",
        );
        assert_eq!(out.dimensions(), (200, 200));
        assert_eq!(px(&out, 150, 175), RED);
        assert_eq!(px(&out, 199, 199), RED);
        assert_eq!(px(&out, 0, 0), TRANSPARENT);
        assert_eq!(px(&out, 99, 199), TRANSPARENT);
        assert_eq!(px(&out, 199, 149), TRANSPARENT);
    }

    #[test]
    fn test_compose_keep_shrink_falls_back_to_contain() {
        let img = solid(300, 50, RED);
        let out = resize_dynamic_image(&img, (200, 200), ScaleMode::Keep, Align::CENTER, "png");
        assert_eq!(out.dimensions(), (200, 200));
        // Contain-fit: 200x33 centered -> rows 83..=115.
        assert_eq!(px(&out, 100, 99), RED);
        assert_eq!(px(&out, 0, 116), TRANSPARENT);
        assert_eq!(px(&out, 0, 82), TRANSPARENT);
    }

    #[test]
    fn test_compose_scale_fills_everything() {
        let img = solid(30, 40, RED);
        let out = resize_dynamic_image(
            &img,
            (200, 200),
            ScaleMode::Scale,
            Align::new(HAlign::Left, VAlign::Top),
            "jpg",
        );
        assert_eq!(out.dimensions(), (200, 200));
        assert_eq!(px(&out, 0, 0), RED);
        assert_eq!(px(&out, 199, 199), RED);
    }

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

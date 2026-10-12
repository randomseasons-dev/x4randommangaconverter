//! Wallpaper (sleep-screen) maker: one picture -> one device-size, always-portrait
//! 24-bit BMP. Steps, in order: ratio -> fill/crop (+ border colour) -> dither.

use crate::pipeline::{dither, Device};
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, ImageDecoder, Luma};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub struct WallpaperSettings {
    pub device: Device,
    /// Keep the picture's own ratio. false = stretch it to the device ratio (height kept).
    pub preserve_ratio: bool,
    /// Scale up or down until the picture covers the screen, then centre-crop (no borders).
    /// false = no scaling: centre-crop at native size, sides shorter than the screen get borders.
    pub fill_screen: bool,
    /// Border colour when borders occur: black (true) or white (false).
    pub black_border: bool,
    /// Floyd–Steinberg dither to the 4 device gray levels. false = plain 256-level gray.
    pub dither: bool,
}

impl Default for WallpaperSettings {
    fn default() -> Self {
        WallpaperSettings {
            device: Device::X4,
            preserve_ratio: true,
            fill_screen: true,
            black_border: false,
            dither: true,
        }
    }
}

fn resize(g: &GrayImage, w: u32, h: u32) -> GrayImage {
    image::imageops::resize(g, w.max(1), h.max(1), FilterType::Lanczos3)
}

/// Turn one grayscale picture into the final device-size wallpaper.
pub fn make_wallpaper(img: &GrayImage, s: &WallpaperSettings) -> GrayImage {
    let (pw, ph) = s.device.portrait();
    let (w, h) = (img.width().max(1), img.height().max(1));

    let placed: GrayImage = match (s.preserve_ratio, s.fill_screen) {
        // Stretched to the device ratio, then filled: that is exactly the screen size.
        (false, true) => resize(img, pw, ph),
        // Stretched to the device ratio at the picture's own height, no scaling after.
        (false, false) => {
            let sw = (h as f64 * pw as f64 / ph as f64).round() as u32;
            resize(img, sw, h)
        }
        // Cover the screen: the tighter side matches, the other overflows and is cropped.
        (true, true) => {
            let scale = (pw as f64 / w as f64).max(ph as f64 / h as f64);
            let nw = ((w as f64 * scale).round() as u32).max(pw);
            let nh = ((h as f64 * scale).round() as u32).max(ph);
            resize(img, nw, nh)
        }
        (true, false) => img.clone(),
    };

    // Centre on the screen canvas: overflow is cropped, short sides become borders.
    let bg = if s.black_border { 0 } else { 255 };
    let mut canvas = GrayImage::from_pixel(pw, ph, Luma([bg]));
    let ox = (pw as i64 - placed.width() as i64) / 2;
    let oy = (ph as i64 - placed.height() as i64) / 2;
    image::imageops::overlay(&mut canvas, &placed, ox, oy);

    if s.dither {
        GrayImage::from_raw(pw, ph, dither(&canvas)).expect("dither keeps the page size")
    } else {
        canvas
    }
}

/// Decode a picture to grayscale, honouring EXIF rotation (phone photos) and
/// flattening any transparency onto white.
pub fn load_picture(path: &Path) -> Result<GrayImage, String> {
    let err = |e: image::ImageError| format!("{}: {}", path.display(), e);
    let mut dec = image::ImageReader::open(path)
        .map_err(|e| format!("{}: {}", path.display(), e))?
        .with_guessed_format()
        .map_err(|e| format!("{}: {}", path.display(), e))?
        .into_decoder()
        .map_err(err)?;
    let orientation = dec.orientation().ok();
    let mut img = DynamicImage::from_decoder(dec).map_err(err)?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    if !img.color().has_alpha() {
        return Ok(img.to_luma8());
    }
    let la = img.to_luma_alpha8();
    let mut out = GrayImage::new(la.width(), la.height());
    for (o, p) in out.pixels_mut().zip(la.pixels()) {
        let (v, a) = (p.0[0] as u32, p.0[1] as u32);
        o.0[0] = ((v * a + 255 * (255 - a)) / 255) as u8;
    }
    Ok(out)
}

/// Load + convert one picture file to wallpaper BMP bytes.
pub fn render_bmp(path: &Path, s: &WallpaperSettings) -> Result<Vec<u8>, String> {
    Ok(encode_bmp24(&make_wallpaper(&load_picture(path)?, s)))
}

/// Render many pictures across all CPU cores in bounded batches; `on_each(i, result)`
/// is called in input order.
pub fn render_bmp_par(
    paths: &[PathBuf],
    s: &WallpaperSettings,
    mut on_each: impl FnMut(usize, Result<Vec<u8>, String>),
) {
    use rayon::prelude::*;
    const BATCH: usize = 32; // big photos decode to large buffers; keep a wave small
    let mut start = 0usize;
    while start < paths.len() {
        let end = (start + BATCH).min(paths.len());
        let batch: Vec<Result<Vec<u8>, String>> = paths[start..end]
            .par_iter()
            .map(|p| render_bmp(p, s))
            .collect();
        for (k, res) in batch.into_iter().enumerate() {
            on_each(start + k, res);
        }
        start = end;
    }
}

/// Uncompressed 24-bit BMP (BITMAPINFOHEADER, BI_RGB, bottom-up) — the format the
/// XTEink stock firmware and CrossPoint both read for sleep screens.
pub fn encode_bmp24(g: &GrayImage) -> Vec<u8> {
    let (w, h) = (g.width() as usize, g.height() as usize);
    let row = (w * 3 + 3) & !3; // rows padded to 4 bytes
    let data_len = row * h;
    let mut out = Vec::with_capacity(54 + data_len);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + data_len) as u32).to_le_bytes()); // file size
    out.extend_from_slice(&[0u8; 4]); // reserved
    out.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset
    out.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER size
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes()); // positive = bottom-up rows
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB (no compression)
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes()); // 72 DPI
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // palette colours
    out.extend_from_slice(&0u32.to_le_bytes()); // important colours
    let pad = row - w * 3;
    for y in (0..h).rev() {
        for x in 0..w {
            let v = g.get_pixel(x as u32, y as u32).0[0];
            out.extend_from_slice(&[v, v, v]); // B, G, R
        }
        out.extend(std::iter::repeat(0u8).take(pad));
    }
    out
}

/// Render a wallpaper to PNG bytes (for the UI preview).
pub fn render_png(path: &Path, s: &WallpaperSettings) -> Result<Vec<u8>, String> {
    let img = make_wallpaper(&load_picture(path)?, s);
    let mut buf = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}

//! Image preparation pipeline: classify -> rotate -> spread/split (structure first),
//! then trim -> resize -> dither (finish), producing ordered `Page`s for packing.

use crate::xtch::Page;
use image::{GrayImage, Luma, RgbaImage};
use imageproc::region_labelling::{connected_components, Connectivity};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orientation {
    Portrait,
    Landscape,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Split {
    None,
    Half,
    Thirds,
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub orientation: Orientation,
    pub split: Split,
    pub preserve_ratio: bool,
    /// Manga mode: right-to-left reading. Only affects spread half order. Default true.
    pub manga_mode: bool,
    /// Near-white threshold for trimming (>= this is "white"). Default 245.
    pub white_thresh: u8,
    /// Drop connected components smaller than this fraction of page area. Default 0.001 (0.1%).
    pub min_blob_frac: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            orientation: Orientation::Portrait,
            split: Split::None,
            preserve_ratio: true,
            manga_mode: true,
            white_thresh: 245,
            min_blob_frac: 0.001,
        }
    }
}

impl Settings {
    /// Base screen size for the chosen orientation.
    fn screen(&self) -> (u32, u32) {
        match self.orientation {
            Orientation::Portrait => (480, 800),
            Orientation::Landscape => (800, 480),
        }
    }
}

const PORTRAIT: (u32, u32) = (480, 800);
const LANDSCAPE: (u32, u32) = (800, 480);
const ASPECT_MIN: f32 = 1.15; // decisive aspect ratio
const SPREAD_AREA_MULT: f32 = 1.5; // >= this * common portrait area => spread
const BAND: f32 = 0.15; // +/-15% common-portrait band

#[derive(Clone, Copy, Debug, PartialEq)]
enum Class {
    Portrait,
    Spread,
    Rotated,
}

/// A prepared image piece + the exact page size it must become.
struct Piece {
    img: RgbaImage,
    tw: u32,
    th: u32,
}

/// Classify by raw dimensions relative to the common portrait area.
fn classify(w: u32, h: u32, common_area: f64) -> Class {
    let (long, short) = if w >= h { (w, h) } else { (h, w) };
    let ratio = long as f32 / short.max(1) as f32;
    let area = (w as f64) * (h as f64);
    if h >= w {
        // portrait aspect (taller or square) -> normal page
        Class::Portrait
    } else if ratio >= ASPECT_MIN && area >= SPREAD_AREA_MULT as f64 * common_area {
        Class::Spread
    } else {
        // landscape aspect but not big enough to be a spread -> a rotated single page
        Class::Rotated
    }
}

/// Median area of the portrait-aspect images = the "common portrait".
fn common_portrait_area(dims: &[(u32, u32)]) -> f64 {
    let mut areas: Vec<f64> = dims
        .iter()
        .filter(|(w, h)| *h > *w && (*h as f32 / (*w).max(1) as f32) >= ASPECT_MIN)
        .map(|(w, h)| *w as f64 * *h as f64)
        .collect();
    if areas.is_empty() {
        // fall back to all images if none are clearly portrait
        areas = dims.iter().map(|(w, h)| *w as f64 * *h as f64).collect();
    }
    areas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if areas.is_empty() {
        0.0
    } else {
        areas[areas.len() / 2]
    }
}

/// Crop a horizontal band [y0,y1) (full width) from an image.
fn band(img: &RgbaImage, y0: u32, y1: u32) -> RgbaImage {
    image::imageops::crop_imm(img, 0, y0, img.width(), y1 - y0).to_image()
}

/// Split a page (landscape output) into pieces per the split setting, all targeting `LANDSCAPE`.
fn landscape_pieces(img: &RgbaImage, split: Split, out: &mut Vec<Piece>) {
    let h = img.height() as f32;
    match split {
        Split::None => out.push(Piece {
            img: img.clone(),
            tw: LANDSCAPE.0,
            th: LANDSCAPE.1,
        }),
        Split::Half => {
            let mid = (h * 0.5) as u32;
            for (y0, y1) in [(0, mid), (mid, img.height())] {
                out.push(Piece {
                    img: band(img, y0, y1),
                    tw: LANDSCAPE.0,
                    th: LANDSCAPE.1,
                });
            }
        }
        Split::Thirds => {
            // 15% overlap => each strip ~43.33% of height.
            let sh = (1.0 + 2.0 * 0.15) / 3.0; // strip height fraction
            let starts = [0.0f32, 0.5 - sh / 2.0, 1.0 - sh];
            for s in starts {
                let y0 = (s * h).round().clamp(0.0, h) as u32;
                let y1 = ((s + sh) * h).round().clamp(0.0, h) as u32;
                out.push(Piece {
                    img: band(img, y0, y1.max(y0 + 1)),
                    tw: LANDSCAPE.0,
                    th: LANDSCAPE.1,
                });
            }
        }
    }
}

/// Emit pieces for one normal (portrait-content) page under the current settings.
fn emit_normal(img: &RgbaImage, s: &Settings, out: &mut Vec<Piece>) {
    match s.orientation {
        Orientation::Portrait => out.push(Piece {
            img: img.clone(),
            tw: PORTRAIT.0,
            th: PORTRAIT.1,
        }),
        Orientation::Landscape => landscape_pieces(img, s.split, out),
    }
}

/// Rotate 90° clockwise.
fn rotate_cw(img: &RgbaImage) -> RgbaImage {
    image::imageops::rotate90(img)
}

/// Build the ordered list of prepared pieces from classified source images.
fn prepare(imgs: Vec<RgbaImage>, s: &Settings) -> Vec<Piece> {
    let dims: Vec<(u32, u32)> = imgs.iter().map(|i| (i.width(), i.height())).collect();
    let common = common_portrait_area(&dims);
    let mut out = Vec::new();

    for img in &imgs {
        let class = classify(img.width(), img.height(), common);
        // 1) rotate rotated-portraits to portrait first
        let base = match class {
            Class::Rotated => rotate_cw(img),
            _ => img.clone(),
        };
        // 2) TRIM the full (rotated) page now — BEFORE any split, so thirds/halves
        //    divide actual content, not white margins.
        let timg = trim(&base, s.white_thresh, s.min_blob_frac);

        match class {
            Class::Rotated | Class::Portrait => emit_normal(&timg, s, &mut out),
            Class::Spread => {
                // F = full (trimmed) spread rotated CW, ALWAYS a portrait page.
                out.push(Piece {
                    img: rotate_cw(&timg),
                    tw: PORTRAIT.0,
                    th: PORTRAIT.1,
                });
                // split at midpoint of the trimmed spread; re-trim each half to
                // clean the inner gutter.
                let (tw, th) = (timg.width(), timg.height());
                let mid = tw / 2;
                let left = trim(
                    &image::imageops::crop_imm(&timg, 0, 0, mid, th).to_image(),
                    s.white_thresh,
                    s.min_blob_frac,
                );
                let right = trim(
                    &image::imageops::crop_imm(&timg, mid, 0, tw - mid, th).to_image(),
                    s.white_thresh,
                    s.min_blob_frac,
                );
                // reading order: manga (RTL) => right first, else left first.
                let halves = if s.manga_mode {
                    [right, left]
                } else {
                    [left, right]
                };
                for half in &halves {
                    match s.orientation {
                        Orientation::Portrait => out.push(Piece {
                            img: half.clone(),
                            tw: PORTRAIT.0,
                            th: PORTRAIT.1,
                        }),
                        Orientation::Landscape => landscape_pieces(half, s.split, &mut out),
                    }
                }
            }
        }
    }
    out
}

/// Connected-component trim: crop to the bounding box of large dark blobs,
/// dropping small isolated marks (page numbers, watermarks, specks).
fn trim(img: &RgbaImage, white_thresh: u8, min_blob_frac: f32) -> RgbaImage {
    let g = image::imageops::grayscale(img);
    let (w, h) = (g.width(), g.height());
    // foreground = content (dark); 255 = fg, 0 = bg(white)
    let mut fg = GrayImage::new(w, h);
    for (x, y, p) in g.enumerate_pixels() {
        fg.put_pixel(x, y, Luma([if p.0[0] < white_thresh { 255 } else { 0 }]));
    }
    let labels = connected_components(&fg, Connectivity::Eight, Luma([0u8]));
    let ncomp = labels.pixels().map(|p| p.0[0]).max().unwrap_or(0);
    if ncomp == 0 {
        return img.clone();
    }
    let mut areas = vec![0u32; (ncomp + 1) as usize];
    for p in labels.pixels() {
        areas[p.0[0] as usize] += 1;
    }
    let min_area = ((w as f32 * h as f32) * min_blob_frac).max(1.0) as u32;
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    let mut any = false;
    for (x, y, p) in labels.enumerate_pixels() {
        let l = p.0[0] as usize;
        if l != 0 && areas[l] >= min_area {
            any = true;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if !any {
        return img.clone();
    }
    image::imageops::crop_imm(img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
}

/// Resize `img` to exactly (tw,th). preserve_ratio => fit + white pad; else stretch.
fn fit(img: &RgbaImage, tw: u32, th: u32, preserve_ratio: bool) -> GrayImage {
    let g = image::imageops::grayscale(img);
    if !preserve_ratio {
        return image::imageops::resize(&g, tw, th, image::imageops::FilterType::Lanczos3);
    }
    let (w, h) = (g.width().max(1), g.height().max(1));
    let scale = (tw as f32 / w as f32).min(th as f32 / h as f32);
    let nw = (w as f32 * scale).round().max(1.0) as u32;
    let nh = (h as f32 * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(&g, nw, nh, image::imageops::FilterType::Lanczos3);
    let mut canvas = GrayImage::from_pixel(tw, th, Luma([255])); // white pad
    let ox = (tw - nw) / 2;
    let oy = (th - nh) / 2;
    image::imageops::overlay(&mut canvas, &resized, ox as i64, oy as i64);
    canvas
}

/// Floyd–Steinberg dither to the 4 device gray levels {0,85,170,255}.
fn dither(g: &GrayImage) -> Vec<u8> {
    let (w, h) = (g.width() as usize, g.height() as usize);
    let mut buf: Vec<f32> = g.pixels().map(|p| p.0[0] as f32).collect();
    const LEVELS: [f32; 4] = [0.0, 85.0, 170.0, 255.0];
    let snap = |v: f32| -> f32 {
        let idx = (v / 85.0).round().clamp(0.0, 3.0) as usize;
        LEVELS[idx]
    };
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let old = buf[i];
            let new = snap(old);
            buf[i] = new;
            let err = old - new;
            let mut add = |xx: usize, yy: usize, f: f32| {
                if xx < w && yy < h {
                    buf[yy * w + xx] += err * f;
                }
            };
            if x + 1 < w {
                add(x + 1, y, 7.0 / 16.0);
            }
            if y + 1 < h {
                if x > 0 {
                    add(x - 1, y + 1, 3.0 / 16.0);
                }
                add(x, y + 1, 5.0 / 16.0);
                if x + 1 < w {
                    add(x + 1, y + 1, 1.0 / 16.0);
                }
            }
        }
    }
    buf.iter().map(|&v| v.clamp(0.0, 255.0) as u8).collect()
}

/// Full conversion: source images -> ordered `.xtch` pages.
pub fn convert(imgs: Vec<RgbaImage>, s: &Settings) -> Vec<Page> {
    let pieces = prepare(imgs, s);
    pieces
        .into_iter()
        .map(|p| {
            // trim already happened in Phase 1 (prepare); here only fit + dither.
            let fitted = fit(&p.img, p.tw, p.th, s.preserve_ratio);
            let gray = dither(&fitted);
            Page {
                width: p.tw as u16,
                height: p.th as u16,
                gray,
            }
        })
        .collect()
}

/// Silence unused warning for `screen()` until the UI wires it in.
#[allow(dead_code)]
fn _use_screen(s: &Settings) -> (u32, u32) {
    s.screen()
}

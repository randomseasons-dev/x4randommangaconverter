//! xtch-core: convert CBZ/CBR/image folders into `.xtch` for the XTEink X4/X3.

pub mod pipeline;
pub mod xtch;

pub use pipeline::{Orientation, Settings, Split};
pub use xtch::{encode_xtch, Page};

use image::RgbaImage;
use std::io::Read;
use std::path::Path;

fn is_image_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".jpg")
        || n.ends_with(".jpeg")
        || n.ends_with(".png")
        || n.ends_with(".webp")
        || n.ends_with(".bmp")
        || n.ends_with(".gif")
}

/// Read all images from a folder (sorted by filename), decoded to RGBA.
pub fn read_folder(dir: &Path) -> Result<Vec<RgbaImage>, String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|s| s.to_str())
                    .map(is_image_name)
                    .unwrap_or(false)
        })
        .collect();
    names.sort();
    let mut out = Vec::new();
    for p in names {
        let img = image::open(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
        out.push(img.to_rgba8());
    }
    Ok(out)
}

/// Read all images from a CBZ (zip) archive, sorted by entry name.
pub fn read_cbz(path: &Path) -> Result<Vec<RgbaImage>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| is_image_name(n))
        .collect();
    names.sort();
    let mut out = Vec::new();
    for n in names {
        let mut f = zip.by_name(&n).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        let img = image::load_from_memory(&bytes).map_err(|e| format!("{}: {}", n, e))?;
        out.push(img.to_rgba8());
    }
    Ok(out)
}

/// Read either a `.cbz`/`.zip` file or a directory of images.
pub fn read_input(path: &Path) -> Result<Vec<RgbaImage>, String> {
    if path.is_dir() {
        read_folder(path)
    } else {
        read_cbz(path)
    }
}

/// Read input + run the pipeline, returning the ordered pages (before packing).
pub fn convert_pages(path: &Path, settings: &Settings) -> Result<Vec<Page>, String> {
    let imgs = read_input(path)?;
    if imgs.is_empty() {
        return Err("no images found in input".into());
    }
    Ok(pipeline::convert(imgs, settings))
}

/// One-shot: read input, run the pipeline, return `.xtch` bytes.
pub fn convert_input(path: &Path, settings: &Settings) -> Result<Vec<u8>, String> {
    Ok(encode_xtch(&convert_pages(path, settings)?))
}

/// Render a page to PNG bytes (for UI preview).
pub fn page_to_png(p: &Page) -> Result<Vec<u8>, String> {
    let img = image::GrayImage::from_raw(p.width as u32, p.height as u32, p.gray.clone())
        .ok_or("invalid page buffer")?;
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}

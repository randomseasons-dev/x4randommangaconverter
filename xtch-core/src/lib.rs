//! xtch-core: convert CBZ/CBR/image folders into `.xtch` for the XTEink X4/X3.

pub mod pipeline;
pub mod xtch;

pub use pipeline::{convert_pages_stream, convert_streaming, Orientation, Settings, Split};
pub use xtch::{assemble, encode_xtch, encoded_page, EncodedPage, Page};

use image::GrayImage;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

fn is_image_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".jpg")
        || n.ends_with(".jpeg")
        || n.ends_with(".png")
        || n.ends_with(".webp")
        || n.ends_with(".bmp")
        || n.ends_with(".gif")
}

/// A lazily-loadable source image: either a file on disk or in-memory (compressed) bytes.
pub enum Loader {
    Path(PathBuf),
    Bytes(Vec<u8>),
}

impl Loader {
    /// Cheap dimensions read (header only for files; header parse for bytes).
    pub fn dims(&self) -> Result<(u32, u32), String> {
        match self {
            Loader::Path(p) => {
                image::image_dimensions(p).map_err(|e| format!("{}: {}", p.display(), e))
            }
            Loader::Bytes(b) => image::ImageReader::new(Cursor::new(b))
                .with_guessed_format()
                .map_err(|e| e.to_string())?
                .into_dimensions()
                .map_err(|e| e.to_string()),
        }
    }

    /// Decode to 8-bit grayscale.
    pub fn load_luma(&self) -> Result<GrayImage, String> {
        match self {
            Loader::Path(p) => image::open(p)
                .map(|i| i.to_luma8())
                .map_err(|e| format!("{}: {}", p.display(), e)),
            Loader::Bytes(b) => image::load_from_memory(b)
                .map(|i| i.to_luma8())
                .map_err(|e| e.to_string()),
        }
    }
}

/// List loadable images from a folder (sorted by filename).
fn list_folder(dir: &Path) -> Result<Vec<Loader>, String> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
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
    Ok(names.into_iter().map(Loader::Path).collect())
}

/// List loadable images from a CBZ (zip), sorted by entry name. Holds only the
/// (compressed) image bytes in memory, not decoded pixels.
fn list_cbz(path: &Path) -> Result<Vec<Loader>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| is_image_name(n))
        .collect();
    names.sort();
    let mut out = Vec::with_capacity(names.len());
    for n in names {
        let mut f = zip.by_name(&n).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        out.push(Loader::Bytes(bytes));
    }
    Ok(out)
}

/// List images from either a `.cbz`/`.zip` file or a directory of images.
pub fn list_input(path: &Path) -> Result<Vec<Loader>, String> {
    if path.is_dir() {
        list_folder(path)
    } else {
        list_cbz(path)
    }
}

/// Cheaply count images in an input without decoding pixels (for progress totals).
pub fn count_images(path: &Path) -> usize {
    if path.is_dir() {
        std::fs::read_dir(path)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .file_name()
                            .and_then(|s| s.to_str())
                            .map(is_image_name)
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    } else {
        std::fs::File::open(path)
            .ok()
            .and_then(|f| zip::ZipArchive::new(f).ok())
            .map(|mut z| {
                (0..z.len())
                    .filter_map(|i| z.by_index(i).ok().map(|f| f.name().to_string()))
                    .filter(|n| is_image_name(n))
                    .count()
            })
            .unwrap_or(0)
    }
}

/// Read input + run the pipeline (streaming), reporting per-image progress via `on_progress(done)`.
pub fn convert_pages_cb(
    path: &Path,
    settings: &Settings,
    on_progress: impl FnMut(usize),
) -> Result<Vec<Page>, String> {
    let loaders = list_input(path)?;
    if loaders.is_empty() {
        return Err("no images found in input".into());
    }
    let dims: Vec<(u32, u32)> = loaders.iter().map(|l| l.dims()).collect::<Result<_, _>>()?;
    convert_streaming(&dims, |i| loaders[i].load_luma(), settings, on_progress)
}

/// Read input + run the pipeline, returning the ordered pages.
pub fn convert_pages(path: &Path, settings: &Settings) -> Result<Vec<Page>, String> {
    convert_pages_cb(path, settings, |_| {})
}

/// Read input + stream each produced page to `on_page` (for incremental packing /
/// size-based file splitting). `on_progress(done)` fires per source image.
pub fn convert_stream(
    path: &Path,
    settings: &Settings,
    on_page: impl FnMut(Page) -> Result<(), String>,
    on_progress: impl FnMut(usize),
) -> Result<(), String> {
    let loaders = list_input(path)?;
    if loaders.is_empty() {
        return Err("no images found in input".into());
    }
    let dims: Vec<(u32, u32)> = loaders.iter().map(|l| l.dims()).collect::<Result<_, _>>()?;
    convert_pages_stream(&dims, |i| loaders[i].load_luma(), settings, on_page, on_progress)
}

/// One-shot: read input, run the pipeline, return `.xtch` bytes.
pub fn convert_input(path: &Path, settings: &Settings) -> Result<Vec<u8>, String> {
    Ok(encode_xtch(&convert_pages(path, settings)?))
}

/// Find (source_index, local_page) for a global page index given per-image counts.
fn locate(counts: &[usize], idx: usize) -> (usize, usize) {
    let mut acc = 0usize;
    for (i, &c) in counts.iter().enumerate() {
        if idx < acc + c {
            return (i, idx - acc);
        }
        acc += c;
    }
    (counts.len().saturating_sub(1), 0)
}

/// Cheaply read image dimensions from (a prefix of) compressed bytes.
fn dims_from_bytes(b: &[u8]) -> Result<(u32, u32), String> {
    image::ImageReader::new(Cursor::new(b))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_dimensions()
        .map_err(|e| e.to_string())
}

/// Live-preview a single output page for the given settings, decoding only the one
/// source image that produces it. Returns (PNG bytes, total pages, clamped index).
pub fn preview_one(
    path: &Path,
    settings: &Settings,
    index: usize,
) -> Result<(Vec<u8>, usize, usize), String> {
    // Collect all source dims cheaply, and keep a way to load the chosen image.
    let (dims, load): (Vec<(u32, u32)>, Box<dyn Fn(usize) -> Result<GrayImage, String>>) =
        if path.is_dir() {
            let loaders = list_folder(path)?;
            if loaders.is_empty() {
                return Err("no images found in input".into());
            }
            let dims = loaders.iter().map(|l| l.dims()).collect::<Result<_, _>>()?;
            (dims, Box::new(move |i| loaders[i].load_luma()))
        } else {
            // CBZ: read a bounded prefix per entry for dims; full-read only the target.
            let path = path.to_path_buf();
            let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
            let mut names: Vec<String> = (0..zip.len())
                .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
                .filter(|n| is_image_name(n))
                .collect();
            names.sort();
            if names.is_empty() {
                return Err("no images found in input".into());
            }
            let mut dims = Vec::with_capacity(names.len());
            for n in &names {
                let mut f = zip.by_name(n).map_err(|e| e.to_string())?;
                let mut prefix = Vec::new();
                std::io::Read::take(&mut f, 256 * 1024)
                    .read_to_end(&mut prefix)
                    .map_err(|e| e.to_string())?;
                dims.push(dims_from_bytes(&prefix)?);
            }
            let names2 = names.clone();
            (
                dims,
                Box::new(move |i| {
                    let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
                    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
                    let mut f = zip.by_name(&names2[i]).map_err(|e| e.to_string())?;
                    let mut bytes = Vec::new();
                    f.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                    image::load_from_memory(&bytes)
                        .map(|im| im.to_luma8())
                        .map_err(|e| e.to_string())
                }),
            )
        };

    let common = pipeline::common_area(&dims);
    let counts: Vec<usize> = dims
        .iter()
        .map(|(w, h)| pipeline::piece_count(*w, *h, common, settings))
        .collect();
    let total: usize = counts.iter().sum();
    if total == 0 {
        return Err("input produced no pages".into());
    }
    let idx = index.min(total - 1);
    let (si, lj) = locate(&counts, idx);
    let img = load(si)?;
    let pages = pipeline::convert_one(&img, common, settings);
    let lj = lj.min(pages.len().saturating_sub(1));
    let png = page_to_png(&pages[lj])?;
    Ok((png, total, idx))
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

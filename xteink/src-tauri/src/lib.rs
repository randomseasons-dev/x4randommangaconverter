use base64::Engine;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::Emitter;
use xtch_core::{Orientation, Settings, Split};

/// Progress event payload emitted during conversion.
#[derive(serde::Serialize, Clone)]
struct Progress {
    done: usize,
    total: usize,
}

/// Settings coming from the UI.
#[derive(serde::Deserialize, Clone)]
struct Opts {
    orientation: String, // "portrait" | "landscape"
    split: String,       // "none" | "half" | "thirds"
    preserve_ratio: bool,
    manga_mode: bool,
    #[serde(default = "default_blob")]
    min_blob_frac: f32, // white-border trim rate (fraction of page area)
    #[serde(default)]
    contrast: f32, // -100..100, 0 = none
    #[serde(default)]
    split_mb: Option<u32>, // split output into files of <= N MB (folders only)
}

fn default_blob() -> f32 {
    0.004
}

impl Opts {
    fn to_settings(&self) -> Settings {
        Settings {
            orientation: if self.orientation == "landscape" {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            },
            split: match self.split.as_str() {
                "half" => Split::Half,
                "thirds" => Split::Thirds,
                _ => Split::None,
            },
            preserve_ratio: self.preserve_ratio,
            manga_mode: self.manga_mode,
            min_blob_frac: self.min_blob_frac.clamp(0.0, 0.05),
            contrast: self.contrast.clamp(-100.0, 100.0),
            ..Default::default()
        }
    }
}

#[derive(serde::Serialize)]
struct ConvertResult {
    name: String,
    ok: bool,
    pages: usize,
    files: usize,
    size: u64,
    out_path: String,
    error: Option<String>,
}

fn out_dir_for(path: &Path, out_dir: &Option<String>) -> PathBuf {
    match out_dir {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
    }
}

/// Convert one input, streaming pages and (for folders, when `split_mb` is set)
/// writing multiple `.xtch` files each <= `split_mb` MB. Single file otherwise.
fn convert_one_input(
    path: &Path,
    settings: &Settings,
    out_dir: &Option<String>,
    split_mb: Option<u32>,
    on_progress: impl FnMut(usize),
) -> ConvertResult {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .to_string();
    let name = format!("{}.xtch", stem);
    let dir = out_dir_for(path, out_dir);
    // Size-splitting applies only to image folders.
    let limit_bytes: Option<usize> = if path.is_dir() {
        split_mb
            .filter(|&mb| (10..=500).contains(&mb))
            .map(|mb| (mb as usize) * 1024 * 1024)
    } else {
        None
    };

    let mut parts: Vec<xtch_core::EncodedPage> = Vec::new();
    let mut part_bytes: usize = 48; // container header
    let mut part_index: usize = 0; // number of parts already flushed mid-stream
    let mut total_pages: usize = 0;
    let mut total_size: u64 = 0;
    let mut written: Vec<PathBuf> = Vec::new();

    let res = xtch_core::convert_stream(
        path,
        settings,
        |page| {
            let ep = xtch_core::encoded_page(&page);
            let add = 16 + ep.data.len(); // dir entry + block
            if let Some(limit) = limit_bytes {
                if !parts.is_empty() && part_bytes + add > limit {
                    part_index += 1;
                    let outp = dir.join(format!("{}_{}.xtch", stem, part_index));
                    let bytes = xtch_core::assemble(&parts);
                    std::fs::write(&outp, &bytes).map_err(|e| e.to_string())?;
                    total_size += bytes.len() as u64;
                    written.push(outp);
                    parts.clear();
                    part_bytes = 48;
                }
            }
            parts.push(ep);
            part_bytes += add;
            total_pages += 1;
            Ok(())
        },
        on_progress,
    );

    if let Err(e) = res {
        return ConvertResult {
            name,
            ok: false,
            pages: 0,
            files: 0,
            size: 0,
            out_path: String::new(),
            error: Some(e),
        };
    }

    // Flush the final (or only) part.
    if !parts.is_empty() {
        let outp = if part_index == 0 {
            dir.join(&name) // single file
        } else {
            dir.join(format!("{}_{}.xtch", stem, part_index + 1))
        };
        let bytes = xtch_core::assemble(&parts);
        if let Err(e) = std::fs::write(&outp, &bytes) {
            return ConvertResult {
                name,
                ok: false,
                pages: 0,
                files: 0,
                size: 0,
                out_path: String::new(),
                error: Some(e.to_string()),
            };
        }
        total_size += bytes.len() as u64;
        written.push(outp);
    }

    ConvertResult {
        name,
        ok: true,
        pages: total_pages,
        files: written.len(),
        size: total_size,
        out_path: written
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        error: None,
    }
}

/// Convert each input (cbz/zip file or image folder) to a `.xtch` saved next to it
/// (or in `out_dir`). Runs on a blocking thread so the UI stays responsive.
#[tauri::command]
async fn convert(
    app: tauri::AppHandle,
    paths: Vec<String>,
    opts: Opts,
    out_dir: Option<String>,
) -> Vec<ConvertResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = opts.to_settings();
        // grand total of source images across all inputs, for the progress bar
        let total: usize = paths
            .iter()
            .map(|p| xtch_core::count_images(Path::new(p)))
            .sum();
        let done = AtomicUsize::new(0);
        let _ = app.emit("convert-progress", Progress { done: 0, total });
        paths
            .iter()
            .map(|p| {
                let path = Path::new(p);
                let progress = |_local: usize| {
                    let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = app.emit("convert-progress", Progress { done: d, total });
                };
                convert_one_input(path, &settings, &out_dir, opts.split_mb, progress)
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Render a converted input's pages to PNG data URLs for preview.
#[tauri::command]
async fn preview(path: String, opts: Opts) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = opts.to_settings();
        let pages = xtch_core::convert_pages(Path::new(&path), &settings)?;
        let mut urls = Vec::with_capacity(pages.len());
        for pg in &pages {
            let png = xtch_core::page_to_png(pg)?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
            urls.push(format!("data:image/png;base64,{}", b64));
        }
        Ok(urls)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
struct PreviewOut {
    url: String,
    total: usize,
    index: usize,
}

/// Live single-page preview for the current settings (decodes only one source image).
#[tauri::command]
async fn preview_page(path: String, opts: Opts, index: usize) -> Result<PreviewOut, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = opts.to_settings();
        let (png, total, idx) = xtch_core::preview_one(Path::new(&path), &settings, index)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        Ok(PreviewOut {
            url: format!("data:image/png;base64,{}", b64),
            total,
            index: idx,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![convert, preview, preview_page])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

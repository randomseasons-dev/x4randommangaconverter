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
            ..Default::default()
        }
    }
}

#[derive(serde::Serialize)]
struct ConvertResult {
    name: String,
    ok: bool,
    pages: usize,
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
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("output")
                    .to_string();
                let name = format!("{}.xtch", stem);
                let outp = out_dir_for(path, &out_dir).join(&name);
                let progress = |_local: usize| {
                    let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = app.emit("convert-progress", Progress { done: d, total });
                };
                match xtch_core::convert_pages_cb(path, &settings, progress) {
                    Ok(pages) => {
                        let bytes = xtch_core::encode_xtch(&pages);
                        match std::fs::write(&outp, &bytes) {
                            Ok(_) => ConvertResult {
                                name,
                                ok: true,
                                pages: pages.len(),
                                size: bytes.len() as u64,
                                out_path: outp.display().to_string(),
                                error: None,
                            },
                            Err(e) => ConvertResult {
                                name,
                                ok: false,
                                pages: 0,
                                size: 0,
                                out_path: String::new(),
                                error: Some(e.to_string()),
                            },
                        }
                    }
                    Err(e) => ConvertResult {
                        name,
                        ok: false,
                        pages: 0,
                        size: 0,
                        out_path: String::new(),
                        error: Some(e),
                    },
                }
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![convert, preview])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

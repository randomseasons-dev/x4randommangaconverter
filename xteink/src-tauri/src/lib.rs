use base64::Engine;
use std::path::{Path, PathBuf};

// Faster allocator: big speedup under heavy alloc churn, and — unlike the default
// Windows allocator — it doesn't degrade as the heap grows (fixes slowdown-over-time).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::Emitter;
use xtch_core::{Device, Orientation, Settings, Split, WallpaperSettings};

/// Progress event payload emitted during conversion.
#[derive(serde::Serialize, Clone)]
struct Progress {
    done: usize,
    total: usize,
}

/// Settings coming from the UI.
#[derive(serde::Deserialize, Clone)]
struct Opts {
    #[serde(default = "default_device")]
    device: String, // "x4" | "x4pro" | "x3"
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

fn default_device() -> String {
    "x4".into()
}

fn parse_device(d: &str) -> Device {
    match d {
        "x4pro" => Device::X4Pro,
        "x3" => Device::X3,
        _ => Device::X4,
    }
}

fn default_blob() -> f32 {
    0.004
}

impl Opts {
    fn to_settings(&self) -> Settings {
        Settings {
            device: parse_device(&self.device),
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

    let res = xtch_core::convert_stream_par(
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
                    // Throttle IPC/UI churn: emit ~1 in 8 (and always the last page).
                    if d % 8 == 0 || d == total {
                        let _ = app.emit("convert-progress", Progress { done: d, total });
                    }
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

// ---------------------------------------------------------------------------
// Wallpapers creator
// ---------------------------------------------------------------------------

/// Wallpaper settings coming from the UI.
#[derive(serde::Deserialize, Clone)]
struct WallOpts {
    #[serde(default = "default_device")]
    device: String,
    preserve_ratio: bool,
    fill_screen: bool,
    black_border: bool,
    dither: bool,
    #[serde(default)]
    zip: bool,
}

impl WallOpts {
    fn to_settings(&self) -> WallpaperSettings {
        WallpaperSettings {
            device: parse_device(&self.device),
            preserve_ratio: self.preserve_ratio,
            fill_screen: self.fill_screen,
            black_border: self.black_border,
            dither: self.dither,
        }
    }
}

/// Keep only inputs the wallpaper maker accepts: folders and picture files.
#[tauri::command]
fn filter_pictures(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .filter(|p| {
            let path = Path::new(p);
            path.is_dir() || (path.is_file() && xtch_core::is_picture(path))
        })
        .collect()
}

/// All pictures behind the added inputs, in order (folders expanded, top level only).
fn all_pictures(paths: &[String]) -> Vec<PathBuf> {
    paths
        .iter()
        .flat_map(|p| xtch_core::list_pictures(Path::new(p)))
        .collect()
}

/// Live preview of one wallpaper (index across every added picture).
#[tauri::command]
async fn wallpaper_preview(
    paths: Vec<String>,
    opts: WallOpts,
    index: usize,
) -> Result<PreviewOut, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let pics = all_pictures(&paths);
        if pics.is_empty() {
            return Err("no pictures found".to_string());
        }
        let idx = index.min(pics.len() - 1);
        let png = xtch_core::wallpaper::render_png(&pics[idx], &opts.to_settings())?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        Ok(PreviewOut {
            url: format!("data:image/png;base64,{}", b64),
            total: pics.len(),
            index: idx,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
struct WallResult {
    name: String,
    ok: bool,
    count: usize,
    failed: usize,
    size: u64,
    out_path: String,
    error: Option<String>,
}

/// One output unit: a dropped folder, or all loose pictures that share a parent folder.
struct WallGroup {
    name: String,
    pics: Vec<PathBuf>,
    zip_path: PathBuf,
    dir_path: PathBuf,
}

const OUT_NAME: &str = "xteink screens";

fn wall_groups(paths: &[String]) -> Vec<WallGroup> {
    let mut groups: Vec<WallGroup> = Vec::new();
    let mut loose: Vec<(PathBuf, usize)> = Vec::new(); // parent dir -> group index
    for p in paths {
        let path = Path::new(p);
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("pictures")
                .to_string();
            // Zip sits next to the folder (inside it for a drive root); subfolder goes inside.
            let zip_dir = path.parent().unwrap_or(path).to_path_buf();
            groups.push(WallGroup {
                pics: xtch_core::list_pictures(path),
                zip_path: zip_dir.join(format!("{}_screens.zip", name)),
                dir_path: path.join(OUT_NAME),
                name,
            });
        } else if xtch_core::is_picture(path) {
            let parent = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
            match loose.iter().find(|(d, _)| *d == parent) {
                Some(&(_, gi)) => groups[gi].pics.push(path.to_path_buf()),
                None => {
                    loose.push((parent.clone(), groups.len()));
                    groups.push(WallGroup {
                        name: parent
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("pictures")
                            .to_string(),
                        pics: vec![path.to_path_buf()],
                        zip_path: parent.join(format!("{}.zip", OUT_NAME)),
                        dir_path: parent.join(OUT_NAME),
                    });
                }
            }
        }
    }
    groups
}

/// `<stem>_screen.bmp`, made unique within one output (case-insensitive, like Windows).
fn unique_name(stem: &str, used: &mut std::collections::HashSet<String>) -> String {
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{}_screen.bmp", stem)
        } else {
            format!("{}_screen_{}.bmp", stem, n)
        };
        if used.insert(name.to_lowercase()) {
            return name;
        }
        n += 1;
    }
}

enum WallSink {
    Dir(PathBuf),
    Zip(zip::ZipWriter<std::fs::File>),
}

impl WallSink {
    fn open(g: &WallGroup, as_zip: bool) -> Result<Self, String> {
        if as_zip {
            let f = std::fs::File::create(&g.zip_path).map_err(|e| e.to_string())?;
            Ok(WallSink::Zip(zip::ZipWriter::new(f)))
        } else {
            std::fs::create_dir_all(&g.dir_path).map_err(|e| e.to_string())?;
            Ok(WallSink::Dir(g.dir_path.clone()))
        }
    }

    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        use std::io::Write;
        match self {
            WallSink::Dir(d) => std::fs::write(d.join(name), bytes).map_err(|e| e.to_string()),
            WallSink::Zip(z) => {
                let opts = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated);
                z.start_file(name, opts).map_err(|e| e.to_string())?;
                z.write_all(bytes).map_err(|e| e.to_string())
            }
        }
    }

    fn finish(self) -> Result<(), String> {
        match self {
            WallSink::Dir(_) => Ok(()),
            WallSink::Zip(z) => z.finish().map(|_| ()).map_err(|e| e.to_string()),
        }
    }
}

fn make_group(
    g: &WallGroup,
    s: &WallpaperSettings,
    as_zip: bool,
    mut on_progress: impl FnMut(),
) -> WallResult {
    let out_path = if as_zip { &g.zip_path } else { &g.dir_path };
    let mut res = WallResult {
        name: g.name.clone(),
        ok: false,
        count: 0,
        failed: 0,
        size: 0,
        out_path: out_path.display().to_string(),
        error: None,
    };
    if g.pics.is_empty() {
        res.error = Some("no pictures found".into());
        return res;
    }
    // The output is created on the first good picture, so an all-failed group leaves nothing.
    let mut sink: Option<WallSink> = None;
    let mut used = std::collections::HashSet::new();
    let mut first_err: Option<String> = None;
    xtch_core::wallpaper::render_bmp_par(&g.pics, s, |i, r| {
        on_progress();
        match r {
            Ok(bytes) => {
                if sink.is_none() {
                    match WallSink::open(g, as_zip) {
                        Ok(sk) => sink = Some(sk),
                        Err(e) => {
                            res.failed += 1;
                            first_err.get_or_insert(e);
                            return;
                        }
                    }
                }
                let stem = g.pics[i]
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("picture");
                let name = unique_name(stem, &mut used);
                match sink.as_mut().unwrap().write(&name, &bytes) {
                    Ok(()) => {
                        res.count += 1;
                        res.size += bytes.len() as u64;
                    }
                    Err(e) => {
                        res.failed += 1;
                        first_err.get_or_insert(e);
                    }
                }
            }
            Err(e) => {
                res.failed += 1;
                first_err.get_or_insert(e);
            }
        }
    });
    if let Some(sk) = sink {
        if let Err(e) = sk.finish() {
            first_err.get_or_insert(e);
            res.count = 0;
        }
    }
    if as_zip && res.count > 0 {
        res.size = std::fs::metadata(&g.zip_path).map(|m| m.len()).unwrap_or(res.size);
    }
    res.ok = res.count > 0;
    res.error = first_err;
    res
}

/// Create the wallpapers: one zip or one subfolder per dropped folder / per parent folder
/// of loose pictures.
#[tauri::command]
async fn create_wallpapers(
    app: tauri::AppHandle,
    paths: Vec<String>,
    opts: WallOpts,
) -> Vec<WallResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = opts.to_settings();
        let groups = wall_groups(&paths);
        let total: usize = groups.iter().map(|g| g.pics.len()).sum();
        let mut done = 0usize;
        let _ = app.emit("convert-progress", Progress { done: 0, total });
        groups
            .iter()
            .map(|g| {
                make_group(g, &s, opts.zip, || {
                    done += 1;
                    if done % 4 == 0 || done == total {
                        let _ = app.emit("convert-progress", Progress { done, total });
                    }
                })
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            convert,
            preview,
            preview_page,
            filter_pictures,
            wallpaper_preview,
            create_wallpapers
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import {
  DeviceId,
  DeviceSelect,
  baseName,
  deviceOf,
  fmtSize,
  useDismiss,
  useWindowDrop,
} from "./shared";

type WallOpts = {
  preserve_ratio: boolean;
  fill_screen: boolean;
  black_border: boolean;
  dither: boolean;
  zip: boolean;
};

type WallResult = {
  name: string;
  ok: boolean;
  count: number;
  failed: number;
  size: number;
  out_path: string;
  error: string | null;
};

const PICTURE_EXT = ["jpg", "jpeg", "png", "webp", "bmp", "gif"];

export default function WallpaperTab({
  active,
  device,
  setDevice,
}: {
  active: boolean;
  device: DeviceId;
  setDevice: (d: DeviceId) => void;
}) {
  const [files, setFiles] = useState<string[]>([]);
  const [skipped, setSkipped] = useState(0);
  const [browseMenu, setBrowseMenu] = useState(false);
  const [opts, setOpts] = useState<WallOpts>({
    preserve_ratio: true,
    fill_screen: true,
    black_border: false,
    dither: true,
    zip: false,
  });
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<WallResult[]>([]);
  const [progress, setProgress] = useState({ done: 0, total: 0 });

  // Live preview state
  const [pvUrl, setPvUrl] = useState<string | null>(null);
  const [pvIndex, setPvIndex] = useState(0);
  const [pvTotal, setPvTotal] = useState(0);
  const [pvLoading, setPvLoading] = useState(false);

  const set = <K extends keyof WallOpts>(k: K, v: WallOpts[K]) =>
    setOpts((o) => ({ ...o, [k]: v }));

  // Only folders and picture files get in (archives and anything else are skipped).
  const addPaths = useCallback(async (arr: string[]) => {
    const ok = await invoke<string[]>("filter_pictures", { paths: arr });
    setSkipped(arr.length - ok.length);
    if (ok.length) setFiles((f) => Array.from(new Set([...f, ...ok])));
  }, []);

  const dragging = useWindowDrop(active, addPaths);

  useEffect(() => {
    const un = listen<{ done: number; total: number }>("convert-progress", (e) =>
      setProgress(e.payload)
    );
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Refresh the preview whenever the inputs, settings, device or index change (debounced).
  useEffect(() => {
    if (files.length === 0) {
      setPvUrl(null);
      setPvTotal(0);
      setPvIndex(0);
      return;
    }
    let cancelled = false;
    setPvLoading(true);
    const t = setTimeout(async () => {
      try {
        const res = await invoke<{ url: string; total: number; index: number }>(
          "wallpaper_preview",
          { paths: files, opts: { ...opts, device }, index: pvIndex }
        );
        if (cancelled) return;
        setPvUrl(res.url);
        setPvTotal(res.total);
        if (res.index !== pvIndex) setPvIndex(res.index);
      } catch {
        if (!cancelled) {
          setPvUrl(null);
          setPvTotal(0);
        }
      } finally {
        if (!cancelled) setPvLoading(false);
      }
    }, 200);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [files, opts, device, pvIndex]);

  const pvNext = () => setPvIndex((i) => Math.min(i + 1, Math.max(pvTotal - 1, 0)));
  const pvPrev = () => setPvIndex((i) => Math.max(i - 1, 0));

  const closeMenu = useCallback(() => setBrowseMenu(false), []);
  useDismiss(browseMenu, closeMenu);

  const addFiles = useCallback(async () => {
    const picked = await open({
      multiple: true,
      filters: [{ name: "Pictures", extensions: PICTURE_EXT }],
    });
    if (!picked) return;
    addPaths(Array.isArray(picked) ? picked : [picked]);
  }, [addPaths]);

  const addFolder = useCallback(async () => {
    const picked = await open({ directory: true, multiple: true });
    if (!picked) return;
    addPaths(Array.isArray(picked) ? picked : [picked]);
  }, [addPaths]);

  const removeFile = (p: string) => setFiles((f) => f.filter((x) => x !== p));

  const create = useCallback(async () => {
    if (files.length === 0) return;
    setBusy(true);
    setResults([]);
    setProgress({ done: 0, total: 0 });
    try {
      const res = await invoke<WallResult[]>("create_wallpapers", {
        paths: files,
        opts: { ...opts, device },
      });
      setResults(res);
    } catch (e) {
      setResults([
        {
          name: "error",
          ok: false,
          count: 0,
          failed: 0,
          size: 0,
          out_path: "",
          error: String(e),
        },
      ]);
    } finally {
      setBusy(false);
    }
  }, [files, opts, device]);

  const dev = deviceOf(device);

  return (
    <div hidden={!active}>
      <main className="grid">
        {/* LEFT: dropzone + file list */}
        <section className="col">
          <div
            className={dragging ? "dropzone dragging" : "dropzone"}
            onClick={(e) => {
              e.stopPropagation();
              setBrowseMenu((m) => !m);
            }}
            role="button"
            tabIndex={0}
          >
            <div className="arrow">↑</div>
            <div className="dz-title">
              {dragging ? (
                "Drop to add"
              ) : (
                <>
                  Drop pictures files picture folders here, or click to{" "}
                  <span className="linklike">browse</span>.
                </>
              )}
            </div>
            <div className="dz-sub">
              Output will be wallpaper files for your xteink device with your
              chosen settings. They can be used for custom sleep screens or
              anything you want.
            </div>
            {browseMenu && (
              <div className="browse-menu" onClick={(e) => e.stopPropagation()}>
                <button
                  onClick={() => {
                    setBrowseMenu(false);
                    addFiles();
                  }}
                >
                  Picture files…
                </button>
                <button
                  onClick={() => {
                    setBrowseMenu(false);
                    addFolder();
                  }}
                >
                  Picture folder…
                </button>
              </div>
            )}
          </div>
          {skipped > 0 && (
            <div className="hint hint-bad">
              Skipped {skipped} item{skipped > 1 ? "s" : ""}: only picture files
              and picture folders can be added.
            </div>
          )}

          {files.length > 0 && (
            <div className="panel">
              <div className="panel-h">
                FILES <span className="badge">{files.length}</span>
              </div>
              <ul className="filelist">
                {files.map((f) => (
                  <li key={f}>
                    <span className="fname" title={f}>
                      {baseName(f)}
                    </span>
                    <button className="x" onClick={() => removeFile(f)}>
                      ×
                    </button>
                  </li>
                ))}
              </ul>
              <button className="cta" disabled={busy} onClick={create}>
                {busy ? "CREATING…" : "CREATE WALLPAPERS →"}
              </button>
              {busy && (
                <div className="progress">
                  <div className="progress-bar">
                    <div
                      className="progress-fill"
                      style={{
                        width:
                          progress.total > 0
                            ? `${(progress.done / progress.total) * 100}%`
                            : "0%",
                      }}
                    />
                  </div>
                  <div className="progress-text">
                    {progress.total > 0
                      ? `${progress.done} / ${progress.total} pictures`
                      : "Reading…"}
                  </div>
                </div>
              )}
            </div>
          )}
        </section>

        {/* MIDDLE: settings */}
        <section className="col">
          <DeviceSelect device={device} onChange={setDevice} />

          <div className="panel">
            <div className="panel-h">BASIC SETTINGS</div>

            <label className="check">
              <input
                type="checkbox"
                checked={opts.preserve_ratio}
                onChange={(e) => set("preserve_ratio", e.target.checked)}
              />
              <span>Preserve pictures ratio</span>
            </label>

            <label className="check">
              <input
                type="checkbox"
                checked={opts.fill_screen}
                onChange={(e) => set("fill_screen", e.target.checked)}
              />
              <span>Resized To Fill Screen (recommended)</span>
            </label>

            <label
              className="check"
              style={{ opacity: opts.fill_screen ? 0.4 : 1 }}
            >
              <input
                type="checkbox"
                disabled={opts.fill_screen}
                checked={opts.black_border}
                onChange={(e) => set("black_border", e.target.checked)}
              />
              <span>Black Border (unticked = white)</span>
            </label>

            <label className="check">
              <input
                type="checkbox"
                checked={opts.dither}
                onChange={(e) => set("dither", e.target.checked)}
              />
              <span>4 gray levels dithering conversion (highly recommended)</span>
            </label>

            <label className="check">
              <input
                type="checkbox"
                checked={opts.zip}
                onChange={(e) => set("zip", e.target.checked)}
              />
              <span>output as zip file</span>
            </label>

            <div className="fixedinfo">
              Always portrait · {dev.w}×{dev.h} ·{" "}
              {opts.dither ? "4-gray 8-bit BMP" : "24-bit BMP"}
            </div>
          </div>
        </section>

        {/* RIGHT: live preview */}
        <section className="col">
          <div className="panel preview-panel">
            <div className="panel-h">PREVIEW</div>
            {files.length > 0 ? (
              <div className="preview-box">
                <div className="preview-stage">
                  {pvUrl && <img src={pvUrl} alt="preview" />}
                  {pvLoading && <div className="preview-load">updating…</div>}
                </div>
                <div className="preview-nav">
                  <button className="tri tri-back" onClick={pvPrev}>
                    ◀
                  </button>
                  <span className="pv-count">
                    {pvTotal > 0 ? `${pvIndex + 1} / ${pvTotal}` : "—"}
                  </span>
                  <button className="tri tri-fwd" onClick={pvNext}>
                    ▶
                  </button>
                </div>
              </div>
            ) : (
              <div className="preview-empty">
                Add a picture to see a live preview of the wallpaper.
              </div>
            )}
          </div>
        </section>
      </main>

      {/* RESULTS */}
      {results.length > 0 && (
        <section className="results">
          <div className="results-h">COMPLETE</div>
          <ul>
            {results.map((r, i) => (
              <li key={i} className={r.ok ? "ok" : "err"}>
                <div className="r-main">
                  <div className="r-name">{baseName(r.out_path) || r.name}</div>
                  {r.ok ? (
                    <div className="r-meta">
                      {r.count} wallpaper{r.count > 1 ? "s" : ""} ·{" "}
                      {fmtSize(r.size)}
                      {r.failed > 0 && (
                        <span className="err-text">
                          {" "}
                          · {r.failed} failed: {r.error}
                        </span>
                      )}
                    </div>
                  ) : (
                    <div className="r-meta err-text">{r.error}</div>
                  )}
                </div>
                {r.ok && (
                  <div className="r-actions">
                    <button onClick={() => revealItemInDir(r.out_path)}>
                      Show file
                    </button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

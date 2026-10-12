import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import {
  DeviceId,
  DeviceSelect,
  baseName,
  fmtSize,
  useDismiss,
  useWindowDrop,
} from "./shared";

type Opts = {
  orientation: "portrait" | "landscape";
  split: "none" | "half" | "thirds";
  preserve_ratio: boolean;
  manga_mode: boolean;
  min_blob_frac: number; // white-border trim aggression as a fraction (0.004 = 0.4%)
  contrast: number; // -100..100, 0 = none
};

type ConvertResult = {
  name: string;
  ok: boolean;
  pages: number;
  files: number;
  size: number;
  out_path: string;
  error: string | null;
};

export default function MangaTab({
  active,
  device,
  setDevice,
}: {
  active: boolean;
  device: DeviceId;
  setDevice: (d: DeviceId) => void;
}) {
  const [files, setFiles] = useState<string[]>([]);
  const [browseMenu, setBrowseMenu] = useState(false);
  const [opts, setOpts] = useState<Opts>({
    orientation: "portrait",
    split: "none",
    preserve_ratio: true,
    manga_mode: true,
    min_blob_frac: 0.004,
    contrast: 0,
  });
  const [splitText, setSplitText] = useState("");
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<ConvertResult[]>([]);
  const [previewPages, setPreviewPages] = useState<string[] | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number }>({
    done: 0,
    total: 0,
  });

  // Live preview state
  const [pvUrl, setPvUrl] = useState<string | null>(null);
  const [pvIndex, setPvIndex] = useState(0);
  const [pvTotal, setPvTotal] = useState(0);
  const [pvLoading, setPvLoading] = useState(false);
  const firstFile = files[0];

  const set = <K extends keyof Opts>(k: K, v: Opts[K]) =>
    setOpts((o) => ({ ...o, [k]: v }));

  // Orientation change: default to overlapping thirds when switching to Landscape.
  const onOrientation = (v: Opts["orientation"]) =>
    setOpts((o) => ({
      ...o,
      orientation: v,
      split: v === "landscape" ? "thirds" : o.split,
    }));

  const addPaths = useCallback(
    (arr: string[]) => setFiles((f) => Array.from(new Set([...f, ...arr]))),
    []
  );

  // Progress events from the backend.
  useEffect(() => {
    const un = listen<{ done: number; total: number }>(
      "convert-progress",
      (e) => setProgress(e.payload)
    );
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Native drag-and-drop of files/folders onto the window (while this tab is shown).
  const dragging = useWindowDrop(active, addPaths);

  // Reset to first page whenever the previewed input changes.
  useEffect(() => {
    setPvIndex(0);
  }, [firstFile]);

  // Refresh the preview whenever the input, settings, or page index change (debounced).
  useEffect(() => {
    if (!firstFile) {
      setPvUrl(null);
      setPvTotal(0);
      return;
    }
    let cancelled = false;
    setPvLoading(true);
    const t = setTimeout(async () => {
      try {
        const res = await invoke<{ url: string; total: number; index: number }>(
          "preview_page",
          { path: firstFile, opts: { ...opts, device }, index: pvIndex }
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
  }, [firstFile, opts, device, pvIndex]);

  const pvNext = () => setPvIndex((i) => Math.min(i + 1, Math.max(pvTotal - 1, 0)));
  const pvPrev = () => setPvIndex((i) => Math.max(i - 1, 0));
  // Manga mode (RTL): left triangle advances; otherwise the right one does.
  const onLeftTriangle = () => (opts.manga_mode ? pvNext() : pvPrev());
  const onRightTriangle = () => (opts.manga_mode ? pvPrev() : pvNext());
  // Forward triangle is green, backward red.
  const leftTriClass = opts.manga_mode ? "tri tri-fwd" : "tri tri-back";
  const rightTriClass = opts.manga_mode ? "tri tri-back" : "tri tri-fwd";

  // Close the browse menu on any outside click or Escape.
  const closeMenu = useCallback(() => setBrowseMenu(false), []);
  useDismiss(browseMenu, closeMenu);

  const splitDisabled = opts.orientation === "portrait";

  const addFiles = useCallback(async () => {
    const picked = await open({
      multiple: true,
      filters: [{ name: "Comic archive", extensions: ["cbz", "zip", "cbr"] }],
    });
    if (!picked) return;
    const arr = Array.isArray(picked) ? picked : [picked];
    setFiles((f) => Array.from(new Set([...f, ...arr])));
  }, []);

  const addFolder = useCallback(async () => {
    const picked = await open({ directory: true, multiple: true });
    if (!picked) return;
    const arr = Array.isArray(picked) ? picked : [picked];
    setFiles((f) => Array.from(new Set([...f, ...arr])));
  }, []);

  const removeFile = (p: string) => setFiles((f) => f.filter((x) => x !== p));

  const convert = useCallback(async () => {
    if (files.length === 0) return;
    setBusy(true);
    setResults([]);
    setProgress({ done: 0, total: 0 });
    try {
      // Split size: use only if a valid in-bounds number was typed; else no split.
      const n = parseInt(splitText, 10);
      const split_mb =
        Number.isFinite(n) && n >= 10 && n <= 500 ? n : null;
      const res = await invoke<ConvertResult[]>("convert", {
        paths: files,
        opts: { ...opts, device, split_mb },
        outDir: null,
      });
      setResults(res);
    } catch (e) {
      setResults([
        {
          name: "error",
          ok: false,
          pages: 0,
          files: 0,
          size: 0,
          out_path: "",
          error: String(e),
        },
      ]);
    } finally {
      setBusy(false);
    }
  }, [files, opts, device, splitText]);

  const doPreview = useCallback(
    async (path: string) => {
      setPreviewBusy(true);
      setPreviewPages([]);
      try {
        const urls = await invoke<string[]>("preview", {
          path,
          opts: { ...opts, device },
        });
        setPreviewPages(urls);
      } catch (e) {
        setPreviewPages(null);
        alert("Preview failed: " + e);
      } finally {
        setPreviewBusy(false);
      }
    },
    [opts, device]
  );

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
                  Drop files (CBZ / CBR) or picture folders here, or click to{" "}
                  <span className="linklike">browse</span>.
                </>
              )}
            </div>
            <div className="dz-sub">
              Output will be .xtch files for your xteink device with your
              chosen settings.
            </div>
            {browseMenu && (
              <div className="browse-menu" onClick={(e) => e.stopPropagation()}>
                <button
                  onClick={() => {
                    setBrowseMenu(false);
                    addFiles();
                  }}
                >
                  CBZ / CBR files…
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
              <button className="cta" disabled={busy} onClick={convert}>
                {busy ? "CONVERTING…" : "CONVERT →"}
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
                      ? `${progress.done} / ${progress.total} pages`
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

            <label className="lbl">ORIENTATION</label>
            <select
              value={opts.orientation}
              onChange={(e) =>
                onOrientation(e.target.value as Opts["orientation"])
              }
            >
              <option value="portrait">Portrait</option>
              <option value="landscape">Landscape</option>
            </select>

            <label className="check">
              <input
                type="checkbox"
                checked={opts.manga_mode}
                onChange={(e) => set("manga_mode", e.target.checked)}
              />
              <span>Manga mode (right-to-left reading order)</span>
            </label>

            <label className="check">
              <input
                type="checkbox"
                checked={opts.preserve_ratio}
                onChange={(e) => set("preserve_ratio", e.target.checked)}
              />
              <span>Preserve picture ratio (no stretched corners)</span>
            </label>

            <div className="fixedinfo">
              2-bit grayscale (XTCH) · 4 gray levels · Floyd–Steinberg dither
            </div>

            <label className="lbl" style={{ opacity: splitDisabled ? 0.4 : 1 }}>
              PAGE SPLIT
            </label>
            <select
              disabled={splitDisabled}
              value={opts.split}
              onChange={(e) => set("split", e.target.value as Opts["split"])}
            >
              <option value="none">No split</option>
              <option value="half">Split in half</option>
              <option value="thirds">Overlapping thirds (15%)</option>
            </select>
            {splitDisabled && (
              <div className="hint">Splitting applies to Landscape only.</div>
            )}
          </div>

          <div className="panel">
            <label className="lbl">White border trimming aggression</label>
            <input
              type="range"
              min={0}
              max={5}
              step={0.1}
              value={opts.min_blob_frac * 100}
              onChange={(e) =>
                set("min_blob_frac", parseFloat(e.target.value) / 100)
              }
            />
            <div className="slider-val">
              {(opts.min_blob_frac * 100).toFixed(1)}%
            </div>

            <label className="lbl" style={{ marginTop: 10 }}>
              Contrast
            </label>
            <input
              type="range"
              min={-100}
              max={100}
              step={1}
              value={opts.contrast}
              onChange={(e) => set("contrast", parseInt(e.target.value, 10))}
            />
            <div className="slider-val">
              {opts.contrast > 0 ? `+${opts.contrast}` : opts.contrast}
            </div>
          </div>

          <div className="panel">
            <label className="lbl">Split output files (MB per file)</label>
            <input
              type="text"
              inputMode="numeric"
              className="numbox"
              placeholder="off"
              value={splitText}
              onChange={(e) => setSplitText(e.target.value)}
            />
            {(() => {
              const n = parseInt(splitText, 10);
              const bad =
                splitText.trim() !== "" &&
                (!Number.isFinite(n) || n < 10 || n > 500);
              return (
                <div className={bad ? "hint hint-bad" : "hint"}>
                  {bad
                    ? "Enter a number from 10 to 500 (or leave blank)."
                    : "Folders only · 10–500 MB per file · blank = one file."}
                </div>
              );
            })()}
          </div>
        </section>

        {/* RIGHT: live preview */}
        <section className="col">
          <div className="panel preview-panel">
            <div className="panel-h">PREVIEW</div>
            {firstFile ? (
              <div className="preview-box">
                <div className="preview-stage">
                  {pvUrl && <img src={pvUrl} alt="preview" />}
                  {pvLoading && <div className="preview-load">updating…</div>}
                </div>
                <div className="preview-nav">
                  <button className={leftTriClass} onClick={onLeftTriangle}>
                    ◀
                  </button>
                  <span className="pv-count">
                    {pvTotal > 0 ? `${pvIndex + 1} / ${pvTotal}` : "—"}
                  </span>
                  <button className={rightTriClass} onClick={onRightTriangle}>
                    ▶
                  </button>
                </div>
              </div>
            ) : (
              <div className="preview-empty">
                Add a file to see a live preview of the final result.
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
                  <div className="r-name">{r.name}</div>
                  {r.ok ? (
                    <div className="r-meta">
                      {r.pages} pages ·{" "}
                      {r.files > 1 ? `${r.files} files · ` : ""}
                      {fmtSize(r.size)}
                    </div>
                  ) : (
                    <div className="r-meta err-text">{r.error}</div>
                  )}
                </div>
                {r.ok && (
                  <div className="r-actions">
                    <button onClick={() => doPreview(files[i])}>Preview</button>
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

      {/* PREVIEW MODAL */}
      {previewPages !== null && (
        <div className="modal" onClick={() => setPreviewPages(null)}>
          <div className="modal-body" onClick={(e) => e.stopPropagation()}>
            <div className="modal-h">
              <span>
                Preview {previewBusy ? "…" : `(${previewPages.length} pages)`}
              </span>
              <button className="x" onClick={() => setPreviewPages(null)}>
                ×
              </button>
            </div>
            <div className="pgrid">
              {previewBusy && <div className="spinner">Rendering…</div>}
              {previewPages.map((src, i) => (
                <div className="pcell" key={i}>
                  <img src={src} alt={`page ${i + 1}`} />
                  <span>{i + 1}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

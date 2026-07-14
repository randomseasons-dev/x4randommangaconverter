import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import "./App.css";

type Opts = {
  orientation: "portrait" | "landscape";
  split: "none" | "half" | "thirds";
  preserve_ratio: boolean;
  manga_mode: boolean;
};

type ConvertResult = {
  name: string;
  ok: boolean;
  pages: number;
  size: number;
  out_path: string;
  error: string | null;
};

const fmtSize = (n: number) =>
  n < 1024
    ? `${n} B`
    : n < 1024 * 1024
    ? `${(n / 1024).toFixed(1)} KB`
    : `${(n / 1024 / 1024).toFixed(1)} MB`;

const baseName = (p: string) => p.split(/[\\/]/).pop() || p;

export default function App() {
  const [files, setFiles] = useState<string[]>([]);
  const [opts, setOpts] = useState<Opts>({
    orientation: "portrait",
    split: "none",
    preserve_ratio: true,
    manga_mode: true,
  });
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<ConvertResult[]>([]);
  const [previewPages, setPreviewPages] = useState<string[] | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);

  const set = <K extends keyof Opts>(k: K, v: Opts[K]) =>
    setOpts((o) => ({ ...o, [k]: v }));

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
    try {
      const res = await invoke<ConvertResult[]>("convert", {
        paths: files,
        opts,
        outDir: null,
      });
      setResults(res);
    } catch (e) {
      setResults([
        {
          name: "error",
          ok: false,
          pages: 0,
          size: 0,
          out_path: "",
          error: String(e),
        },
      ]);
    } finally {
      setBusy(false);
    }
  }, [files, opts]);

  const doPreview = useCallback(
    async (path: string) => {
      setPreviewBusy(true);
      setPreviewPages([]);
      try {
        const urls = await invoke<string[]>("preview", { path, opts });
        setPreviewPages(urls);
      } catch (e) {
        setPreviewPages(null);
        alert("Preview failed: " + e);
      } finally {
        setPreviewBusy(false);
      }
    },
    [opts]
  );

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          XTEink <span>Manga Creator</span>
        </div>
        <div className="tag">offline · .xtch</div>
      </header>

      <main className="grid">
        {/* LEFT: dropzone + file list */}
        <section className="col">
          <div
            className="dropzone"
            onClick={addFiles}
            role="button"
            tabIndex={0}
          >
            <div className="arrow">↑</div>
            <div className="dz-title">Add CBZ / CBR files</div>
            <div className="dz-sub">
              or click to browse ·{" "}
              <button
                className="linklike"
                onClick={(e) => {
                  e.stopPropagation();
                  addFolder();
                }}
              >
                add an image folder
              </button>
            </div>
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
            </div>
          )}
        </section>

        {/* RIGHT: settings */}
        <section className="col">
          <div className="panel">
            <div className="panel-h">TARGET DEVICE</div>
            <div className="devrow">
              <button className="dev active">[X4]</button>
              <button className="dev" disabled title="X3 not supported yet">
                [X3]
              </button>
            </div>
          </div>

          <div className="panel">
            <div className="panel-h">BASIC SETTINGS</div>

            <label className="lbl">ORIENTATION</label>
            <select
              value={opts.orientation}
              onChange={(e) =>
                set("orientation", e.target.value as Opts["orientation"])
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
              <span>Preserve picture ratio (white-pad, no stretch)</span>
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
                      {r.pages} pages · {fmtSize(r.size)}
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

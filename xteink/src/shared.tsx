import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

export type DeviceId = "x4" | "x4pro" | "x3";

// Device profiles: the choice drives the page size used by the whole pipeline
// (the backend holds the same table in `xtch_core::Device`).
export const DEVICES: { id: DeviceId; label: string; w: number; h: number }[] = [
  { id: "x4", label: "XTEink X4", w: 480, h: 800 },
  { id: "x4pro", label: "XTEink X4 Pro", w: 480, h: 800 },
  { id: "x3", label: "XTEink X3", w: 528, h: 792 },
];

export const deviceOf = (id: DeviceId) => DEVICES.find((d) => d.id === id)!;

export const fmtSize = (n: number) =>
  n < 1024
    ? `${n} B`
    : n < 1024 * 1024
    ? `${(n / 1024).toFixed(1)} KB`
    : `${(n / 1024 / 1024).toFixed(1)} MB`;

export const baseName = (p: string) => p.split(/[\\/]/).pop() || p;

export function DeviceSelect({
  device,
  onChange,
}: {
  device: DeviceId;
  onChange: (d: DeviceId) => void;
}) {
  return (
    <div className="panel">
      <div className="panel-h">TARGET DEVICE</div>
      <select value={device} onChange={(e) => onChange(e.target.value as DeviceId)}>
        {DEVICES.map((d) => (
          <option key={d.id} value={d.id}>
            {d.label} ({d.w}×{d.h})
          </option>
        ))}
      </select>
    </div>
  );
}

/** Native window drag-and-drop, delivered only while `active` (the visible tab). */
export function useWindowDrop(active: boolean, onPaths: (paths: string[]) => void) {
  const [dragging, setDragging] = useState(false);
  const activeRef = useRef(active);
  activeRef.current = active;
  const cbRef = useRef(onPaths);
  cbRef.current = onPaths;
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((event) => {
      if (!activeRef.current) return;
      const t = event.payload.type;
      if (t === "enter" || t === "over") setDragging(true);
      else if (t === "leave") setDragging(false);
      else if (t === "drop") {
        setDragging(false);
        if (event.payload.paths?.length) cbRef.current(event.payload.paths);
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);
  return dragging;
}

/** Close a popup menu on any outside click or Escape. */
export function useDismiss(open: boolean, close: () => void) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("click", close);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, close]);
}

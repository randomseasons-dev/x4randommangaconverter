import { useState } from "react";
import MangaTab from "./MangaTab";
import WallpaperTab from "./WallpaperTab";
import { DeviceId } from "./shared";
import "./App.css";

const TABS = ["Manga converter", "Wallpapers creator", "coming soon..."];
const TAGS = ["offline · .xtch", "offline · .bmp", "offline"];

export default function App() {
  const [tab, setTab] = useState(0);
  // One target device for the whole app, shared by every tab.
  const [device, setDevice] = useState<DeviceId>("x4");

  return (
    <div className="app">
      <header className="topbar">
        <nav className="tabs">
          {TABS.map((label, i) => (
            <button
              key={i}
              className={i === tab ? "tab active" : "tab"}
              onClick={() => setTab(i)}
            >
              {label}
            </button>
          ))}
        </nav>
        <div className="tag">{TAGS[tab]}</div>
      </header>

      {/* Tabs stay mounted so each keeps its files and settings while hidden. */}
      <MangaTab active={tab === 0} device={device} setDevice={setDevice} />
      <WallpaperTab active={tab === 1} device={device} setDevice={setDevice} />
      {tab === 2 && <div className="panel coming-soon">Coming soon...</div>}
    </div>
  );
}

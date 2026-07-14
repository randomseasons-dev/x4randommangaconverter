# XTEink Manga Creator

An **offline** desktop app that converts manga (CBZ/CBR archives or folders of images)
into `.xtch` files for the **XTEink X4** e-reader.

Built with [Tauri](https://tauri.app/) (Rust core + React UI). No internet required — all
processing runs locally.

## Features

- **Input:** CBZ / CBR / ZIP archives, or a folder of JPG/PNG images.
- **Auto-trim:** crops white borders and removes page numbers / watermarks
  (connected-component detection drops small isolated marks).
- **Classification:** detects normal pages, rotated pages (auto-rotated upright), and
  double-page spreads.
- **Double-spread handling:** shows the full spread, then splits it into two pages in
  reading order (right-to-left in Manga mode).
- **Orientation:** Portrait (480×800) or Landscape (800×480).
- **Page split (landscape):** overlapping thirds (15% overlap), split-in-half, or none.
- **Resize:** preserve aspect ratio (white-padded) or stretch to fill.
- **Dithering:** Floyd–Steinberg to 4 gray levels (2-bit XTCH).
- **Output:** byte-exact `.xtch` packing, with an in-app page preview.

## Project layout

- `xtch-core/` — pure-Rust library: input reading, image pipeline, and the `.xtch` codec.
  Includes `xtch-cli` for headless conversion/testing.
- `xteink/` — the Tauri desktop app (React frontend + Rust commands wrapping `xtch-core`).

## Build

Prerequisites: Rust (MSVC toolchain), Node.js, and the WebView2 runtime (Windows).

```sh
cd xteink
npm install
npm run tauri dev     # run in development
npm run tauri build   # produce a standalone .exe + installers
```

The standalone build lands in `xteink/src-tauri/target/release/`.

## The `.xtch` format

The 2-bit `XTH` page format was reverse-engineered and validated against real device files:
a container (`XTCH` magic, page directory) plus per-page two-bit-plane, column-major
(right-to-left columns, 8 rows/byte) pixel data with a 4-level gray LUT.

## License

Personal project. Not affiliated with XTEink or xtcjs.app.

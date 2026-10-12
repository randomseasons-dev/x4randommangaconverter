# Ver 1.5.0 changelog:
- Renamed to xteink all-in-one converter (window, exe and installers) as more features will be added and separated in tabs. Currently the Manga Converter feature has been completed and is the default tab.
- X4 Pro and X3 devices are added with fully functional features.
- Text changes for easier reading.
- Preview arrows now have better guiding color: next page = green, previous page = red (because of Manga mode's reversed reading direction).
- Includes v1.4.0: ~2.7× faster parallel conversion, no more slow-down on long batches.
- X4 Pro: .xtch files need CrossPoint Reader firmware v1.6.0 or newer (avoid the first 1.6.0 build).

# Ver 1.4.0 changelog:
- Much faster conversion: pages are processed in parallel on all CPU cores (~2.7× faster).
- Fixed conversion getting slower over time on long batches (new memory allocator).

# Ver 1.3.1 changelog:
- Target device is now a dropdown (X3 and future devices to come).
- White-border trimming default is now reduced to 0.4%.
- Split-size box now accepts free number entry (bounds are checked, not auto-adjusted).
- Compacted the settings column so it takes less vertical space.
- Minor label wording fixes.

# Ver 1.3 changelog:
- Added Contrast slider. Preview panel updates in real time.
- Added split output file size (for picture folders drop in).

# Ver 1.2 changelog:
- Added preview panel.
- Added white border adjustment rate bar. 
- Fixed dropping  files not working. 

# Future features:
- Customizable double-page display. Currently fixed: full page then split. 
- Customizable dithering method. 
- Other ideas. 

# xteink all-in-one converter

An **offline** desktop app that converts manga (CBZ/CBR archives or folders of images)
into `.xtch` files for **XTEink** e-readers: **X4**, **X4 Pro** and **X3**.

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
- **Devices:** X4 and X4 Pro (480×800), X3 (528×792) — the chosen device sets the page size.
- **Orientation:** Portrait or Landscape (e.g. 480×800 / 800×480 on the X4).
- **Page split (landscape):** overlapping thirds (15% overlap), split-in-half, or none.
- **Resize:** preserve aspect ratio (white-padded) or stretch to fill.
- **Dithering:** Floyd–Steinberg to 4 gray levels (2-bit XTCH).
- **Output:** byte-exact `.xtch` packing, with an in-app page preview.



## The `.xtch` format

The 2-bit `XTH` page format was reverse-engineered and validated against real device files:
a container (`XTCH` magic, page directory) plus per-page two-bit-plane, column-major
(right-to-left columns, 8 rows/byte) pixel data with a 4-level gray LUT.

## Reading `.xtch` on the X4 Pro

The X4 Pro's stock firmware lists only EPUB / TXT / JPG / BMP. Manga `.xtch` files are read by
the [CrossPoint Reader](https://github.com/crosspoint-reader/crosspoint-reader) firmware, which
supports the X4 Pro from v1.6.0 (use a build newer than the first 1.6.0 release, which had a
memory bug opening `.xtch` on the Pro).

## License

Personal project. Not affiliated with XTEink or xtcjs.app.

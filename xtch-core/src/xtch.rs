//! Byte-exact `.xtch` encoder for the XTEink X4/X3 (2-bit "XTH" pages).
//! Format reverse-engineered & validated against a real sample (round-trip byte-identical
//! except one reader-ignored metadata u16). See project memory `xtch-format-spec`.

/// One page ready to pack: `gray` is row-major, `width*height` bytes, 0..=255.
pub struct Page {
    pub width: u16,
    pub height: u16,
    pub gray: Vec<u8>,
}

/// Gray value -> 2-bit level. Reader LUT is level->gray {0:255,1:170,2:85,3:0},
/// so we pick the nearest level. (Pipeline usually pre-snaps to these 4 values.)
#[inline]
fn quantize(g: u8) -> u8 {
    const LUT: [i32; 4] = [255, 170, 85, 0];
    let g = g as i32;
    let mut best = 0u8;
    let mut bd = i32::MAX;
    for (l, &v) in LUT.iter().enumerate() {
        let d = (v - g).abs();
        if d < bd {
            bd = d;
            best = l as u8;
        }
    }
    best
}

/// Encode a single 2-bit page block (22-byte header + two bit-planes).
/// Planes are column-major, columns stored right-to-left, 8 rows/byte (MSB = top).
pub fn encode_page(p: &Page) -> Vec<u8> {
    let w = p.width as usize;
    let h = p.height as usize;
    let a = (h + 7) / 8; // bytes per column
    let plane = a * w;
    let mut low = vec![0u8; plane];
    let mut high = vec![0u8; plane];

    for u in 0..w {
        let base = (w - 1 - u) * a; // columns right-to-left
        for b in 0..h {
            let lvl = quantize(p.gray[b * w + u]);
            let y = base + (b >> 3);
            let m = 7 - (b & 7);
            if lvl & 1 != 0 {
                low[y] |= 1 << m;
            }
            if lvl & 2 != 0 {
                high[y] |= 1 << m;
            }
        }
    }

    let data_len = (2 * plane) as u32;
    let mut out = Vec::with_capacity(22 + 2 * plane);
    out.extend_from_slice(b"XTH\0"); // @0 magic
    out.extend_from_slice(&p.width.to_le_bytes()); // @4
    out.extend_from_slice(&p.height.to_le_bytes()); // @6
    out.extend_from_slice(&0u16.to_le_bytes()); // @8 flags = 0
    out.extend_from_slice(&data_len.to_le_bytes()); // @10 pixel-data length
    out.extend_from_slice(&[0u8; 8]); // @14..21 reserved (incl. @20 reader-ignored metadata)
    out.extend_from_slice(&low);
    out.extend_from_slice(&high);
    out
}

/// A page already encoded to its `.xtch` block bytes (+ its dimensions).
/// Lets us pack pages incrementally (e.g. size-based file splitting) without
/// holding all the 8-bit `Page` buffers in memory.
pub struct EncodedPage {
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

/// Encode a page to its block form.
pub fn encoded_page(p: &Page) -> EncodedPage {
    EncodedPage {
        width: p.width,
        height: p.height,
        data: encode_page(p),
    }
}

/// Assemble a full `.xtch` container from already-encoded page blocks.
pub fn assemble(pages: &[EncodedPage]) -> Vec<u8> {
    let n = pages.len();
    let mut header = vec![0u8; 48];
    header[0..4].copy_from_slice(b"XTCH");
    header[4..6].copy_from_slice(&1u16.to_le_bytes()); // version
    header[6..8].copy_from_slice(&(n as u16).to_le_bytes()); // page count
    header[0x18..0x20].copy_from_slice(&48u64.to_le_bytes()); // directory offset
    let data_off = 48 + 16 * n;
    header[0x20..0x28].copy_from_slice(&(data_off as u64).to_le_bytes());

    let mut dir = Vec::with_capacity(16 * n);
    let mut cur = data_off;
    for p in pages {
        dir.extend_from_slice(&(cur as u64).to_le_bytes()); // offset
        dir.extend_from_slice(&(p.data.len() as u32).to_le_bytes()); // size (full block)
        dir.extend_from_slice(&p.width.to_le_bytes());
        dir.extend_from_slice(&p.height.to_le_bytes());
        cur += p.data.len();
    }

    let mut out = header;
    out.extend_from_slice(&dir);
    for p in pages {
        out.extend_from_slice(&p.data);
    }
    out
}

/// Assemble the full `.xtch` container from ordered pages.
pub fn encode_xtch(pages: &[Page]) -> Vec<u8> {
    let enc: Vec<EncodedPage> = pages.iter().map(encoded_page).collect();
    assemble(&enc)
}

/// Byte size of a container holding `n` page blocks of the given total block bytes.
pub fn container_size(n: usize, total_block_bytes: usize) -> usize {
    48 + 16 * n + total_block_bytes
}

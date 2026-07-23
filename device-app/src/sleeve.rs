//! Sleeve bitmaps: the 1bpp square a record shows.
//!
//! ## Packing convention
//!
//! Stored art is packed so that the display, which decodes row-major, shows
//! the image upright. Measured from on-device renders (see
//! `docs/art/README.md`): the screen shows the buffer's row-major decode
//! rotated 90 degrees clockwise, so the packer pre-rotates counter-clockwise.
//! For an image pixel `(x, y)` of an `N x N` sleeve:
//!
//! ```text
//! bit = (N - 1 - x) * N + y      byte = bit / 8, mask = 0x80 >> (bit % 8)
//! ```
//!
//! A bit set means white. The host-side packer (`scripts/sleeve.py`) emits
//! exactly this, and `docs/art/test-pattern.bin` exists to confirm it on
//! screen rather than on trust.
//!
//! ## Working in stored space
//!
//! [`decimate`] never undoes the rotation. Writing the stored index as
//! `k = i * N + j`, the rotation is just a relabelling of `(x, y)` as
//! `(i, j)`, and it maps 2x2 blocks to 2x2 blocks. So a box filter applied in
//! `(i, j)` produces a correctly rotated half-size sleeve in the same
//! convention, with no unpacking and no second buffer.

use alloc::vec::Vec;

/// Turn canonical sleeve bytes into the buffer NBGL should draw.
///
/// The stored (and hashed) bytes are exactly what `scripts/sleeve.py` emits: a
/// set bit is lit art, per that tool's `1 = white` convention. But this 1bpp
/// path renders a set bit as *black* (measured: `scripts/check-packing.py`,
/// correlation 1.0 with `invert`). So to put white art on a black ground, the
/// way every validated preview looks, the device inverts the bits at render
/// time. The canonical bytes, and therefore the certificate's sleeve hash, are
/// left untouched; only this display copy is flipped.
pub fn to_display(canonical: &[u8]) -> Vec<u8> {
    canonical.iter().map(|b| !b).collect()
}

/// Read pixel `(i, j)` in stored space from a packed 1bpp square.
#[inline]
fn get_bit(data: &[u8], n: usize, i: usize, j: usize) -> bool {
    let k = i * n + j;
    let byte = k >> 3;
    byte < data.len() && data[byte] & (0x80 >> (k & 7)) != 0
}

/// Set pixel `(i, j)` in stored space.
#[inline]
fn set_bit(data: &mut [u8], n: usize, i: usize, j: usize) {
    let k = i * n + j;
    let byte = k >> 3;
    if byte < data.len() {
        data[byte] |= 0x80 >> (k & 7);
    }
}

/// Halve a packed 1bpp square by 2x2 box filter: a block is lit when at least
/// two of its four pixels are. Allocates only the (quarter-size) result, so a
/// grid of thumbnails costs a few hundred bytes each rather than a second
/// full-size copy.
pub fn decimate(src: &[u8], n: usize) -> Vec<u8> {
    let half = n / 2;
    let mut out = alloc::vec![0u8; half * half / 8];
    for i in 0..half {
        for j in 0..half {
            let lit = get_bit(src, n, 2 * i, 2 * j) as u8
                + get_bit(src, n, 2 * i + 1, 2 * j) as u8
                + get_bit(src, n, 2 * i, 2 * j + 1) as u8
                + get_bit(src, n, 2 * i + 1, 2 * j + 1) as u8;
            if lit >= 2 {
                set_bit(&mut out, half, i, j);
            }
        }
    }
    out
}

/// Reflection band height under the cover on the record card: ~30% of the
/// 160px cover, the short Cover Flow mirror.
pub const REFLECT_H: usize = 48;

/// 4x4 ordered Bayer matrix (unnormalised thresholds 0..15). Turns the smooth
/// brightness ramp of the reflection into a strict 1-bit dither, no greyscale.
const BAYER4: [[u8; 4]; 4] = [
    [0, 8, 2, 10],
    [12, 4, 14, 6],
    [3, 11, 1, 9],
    [15, 7, 13, 5],
];

/// A 5x7 bitmap font for the record numeral: five bits per row (bit 4 =
/// leftmost column), seven rows top to bottom. These are proper numeral shapes
/// (the `1` has a flag and a base, so it never reads as a bare bar or an `l`).
/// Code `0..=9` is that digit; code `10` is the leading `#`, so the numeral
/// reads as `#N` (a record number) rather than a bare count.
fn glyph_5x7(c: u8) -> [u8; 7] {
    match c {
        0 => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        1 => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        2 => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        3 => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        4 => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        5 => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        6 => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        7 => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        8 => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        9 => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        // '#': the octothorpe that marks a record number.
        10 => [0b01010, 0b11111, 0b01010, 0b01010, 0b11111, 0b01010, 0b00000],
        _ => [0; 7],
    }
}

/// Set one displayed pixel black in a packed 1bpp icon of displayed size
/// `w`x`h`. The device's 1bpp blit shows a set bit as black and maps displayed
/// pixel (x, y) to buffer bit `(w - 1 - x) * h + y` (the same convention the
/// square sleeve is measured to use, generalised to a rectangle). Composited
/// buffers are display-polarity: a set bit is black ink on the white card.
#[inline]
fn ink(buf: &mut [u8], w: usize, h: usize, x: usize, y: usize) {
    if x >= w || y >= h {
        return;
    }
    let k = (w - 1 - x) * h + y;
    let byte = k >> 3;
    if byte < buf.len() {
        buf[byte] |= 0x80 >> (k & 7);
    }
}

/// Where the card reads its cover from, pixel by pixel, so no full-size display
/// copy has to live in RAM beside the composite: a verified sleeve is read
/// straight out of NVM (canonical bytes, no heap), and the generative fallback
/// is computed on the fly from the album id.
pub enum Cover<'a> {
    /// Canonical stored bytes (a set bit is white art); the static NVM sleeve.
    Canonical(&'a [u8]),
    /// Generate the label art from the album id at draw time.
    Fallback([u8; 32]),
}

/// Is the cover pixel at displayed (x, y) black ink? The card composites in
/// display polarity (set = black), which is the complement of the canonical
/// "set = white" convention both sources share.
#[inline]
fn cover_black(src: &Cover, n: usize, x: usize, y: usize) -> bool {
    let white = match src {
        Cover::Canonical(bytes) => {
            let k = (n - 1 - x) * n + y;
            let byte = k >> 3;
            byte < bytes.len() && bytes[byte] & (0x80 >> (k & 7)) != 0
        }
        Cover::Fallback(album_id) => fallback_white(n, album_id, x, y),
    };
    !white
}

/// Draw the record numeral `#N` into the left column of the card from the 5x7
/// font, each cell scaled to a solid block, vertically centred against the
/// cover. A leading `#` glyph precedes the digits. The cell scale shrinks as the
/// glyph count grows so the numeral always stays inside the column, down to a
/// four-digit edition (`#1000`).
fn draw_number(buf: &mut [u8], w: usize, h: usize, col_w: usize, cover_h: usize, number: u16) {
    // Glyph codes, most-significant first: a leading '#' (code 10) then the
    // decimal digits. Up to five digits covers the whole u16 range.
    let mut glyphs = [0u8; 6];
    let mut ng = 0usize;
    glyphs[ng] = 10; // '#'
    ng += 1;

    let mut digits = [0u8; 5];
    let mut nd = 0usize;
    let mut v = number;
    if v == 0 {
        digits[0] = 0;
        nd = 1;
    }
    while v > 0 && nd < digits.len() {
        digits[nd] = (v % 10) as u8;
        nd += 1;
        v /= 10;
    }
    // digits[] holds least-significant first; append most-significant first.
    for i in 0..nd {
        glyphs[ng] = digits[nd - 1 - i];
        ng += 1;
    }

    let margin = 6usize;
    let usable = col_w.saturating_sub(2 * margin);
    // Each glyph is 5 cells wide; glyphs are separated by one cell of space.
    let cells_wide = ng * 5 + (ng - 1);
    let scale = (usable / cells_wide).min(cover_h.saturating_sub(24) / 7).max(1);
    let glyph_w = 5 * scale;
    let gap = scale;
    let total_w = ng * glyph_w + (ng - 1) * gap;
    let start_x = col_w.saturating_sub(total_w) / 2;
    let start_y = (cover_h - 7 * scale) / 2;

    for (i, &code) in glyphs.iter().take(ng).enumerate() {
        let rows = glyph_5x7(code);
        let gx = start_x + i * (glyph_w + gap);
        for (r, bits) in rows.iter().enumerate() {
            for c in 0..5 {
                if bits & (1 << (4 - c)) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            ink(buf, w, h, gx + c * scale + dx, start_y + r * scale + dy);
                        }
                    }
                }
            }
        }
    }
}

/// Compose the record card artwork in RAM: a large "#N" on the left, the 160px
/// cover to its right, and under the cover a short Cover Flow mirror (the cover
/// flipped, ordered-dithered from ~0.55 at the seam down to 0). Returns the
/// packed display-polarity bitmap and its displayed width and height. Nothing
/// here is stored: the reflection exists only for this draw.
pub fn record_card(cover: &Cover, cover_n: usize, number: Option<u16>) -> (Vec<u8>, u16, u16) {
    let num_col = if number.is_some() { 112usize } else { 0 };
    let w = num_col + cover_n; // displayed width
    let h = cover_n + REFLECT_H; // displayed height
    let mut buf = alloc::vec![0u8; w * h / 8];

    if let Some(n) = number {
        draw_number(&mut buf, w, h, num_col, cover_n, n);
    }

    // The cover, copied pixel-for-pixel into the right column.
    for sy in 0..cover_n {
        for sx in 0..cover_n {
            if cover_black(cover, cover_n, sx, sy) {
                ink(&mut buf, w, h, num_col + sx, sy);
            }
        }
    }

    // The reflection: cover rows mirrored downward from the seam, each pixel
    // kept only when its fading brightness clears the Bayer threshold at its
    // position. Brightness starts at 0.55 (of full ink density) at the seam and
    // ramps to 0 at the bottom of the band.
    for rr in 0..REFLECT_H {
        let src_y = cover_n - 1 - rr;
        let level = (55 * 16 * (REFLECT_H - rr) as u32) / (REFLECT_H as u32 * 100);
        let disp_y = cover_n + rr;
        for sx in 0..cover_n {
            if !cover_black(cover, cover_n, sx, src_y) {
                continue;
            }
            let x = num_col + sx;
            let threshold = BAYER4[disp_y & 3][x & 3] as u32;
            if level > threshold {
                ink(&mut buf, w, h, x, disp_y);
            }
        }
    }

    (buf, w as u16, h as u16)
}

/// The label art a record falls back to: when it holds no sleeve, or when the
/// stored sleeve's hash does not match the one in the certificate.
///
/// Generated from the album id rather than picked from a set of compiled
/// bitmaps. Shipping the alternatives as glyphs cost tens of kilobytes of
/// flash, which on this app competes directly with the size of the *real*
/// sleeve an artist can store (see `docs/protocol.md`). Computing it also
/// makes every album distinct instead of one of eight.
///
/// A vinyl record seen face-on: a black disc on a light ground, ringed by a
/// solid rim, cut by fine grooves, and centred on a solid label with a punched
/// spindle hole. Matches the aesthetic of `glyphs/vinyl_64x64.gif`. Reads as a
/// record, not a target: the vinyl body stays mostly black, and the grooves are
/// single-pixel rings spaced `pitch` apart, never the equal light/dark bands
/// that made the old placeholder look like radar.
///
/// The groove pitch and the label radius are derived from the album id, and one
/// id bit adds a thin ring inside the label, so no two editions render the same
/// face while every one still reads as a record.
///
/// Returns canonical bytes (a set bit is white art, per the module's polarity
/// note); the caller inverts through [`to_display`] before drawing.
pub fn fallback_sleeve(n: usize, album_id: &[u8; 32]) -> Vec<u8> {
    let mut out = alloc::vec![0u8; n * n / 8];
    for i in 0..n {
        for j in 0..n {
            if fallback_white(n, album_id, i, j) {
                set_bit(&mut out, n, i, j);
            }
        }
    }
    out
}

/// The generative label art as a per-pixel function (canonical: `true` is white
/// art). Radially symmetric, so it can be sampled in either stored `(i, j)` or
/// displayed `(x, y)` coordinates and the record card reads it directly without
/// materialising a full-size buffer. See [`fallback_sleeve`] for the intent.
pub fn fallback_white(n: usize, album_id: &[u8; 32], i: usize, j: usize) -> bool {
    let seed = album_id[0];
    let centre = (n / 2) as i32;
    let radius = centre - 2;
    let pitch = 5 + (seed & 3) as i32; // 5..8 px between grooves
    let label_r = radius / 3 + ((seed >> 2) & 3) as i32 * 2;
    let hole_r = 3;
    let label_ring = (seed >> 4) & 1 == 1;
    let ring_r = label_r / 2;

    let dx = i as i32 - centre;
    let dy = j as i32 - centre;
    let d2 = dx * dx + dy * dy;
    // Integer distance, free of floats (banned on this target).
    let mut d = 0i32;
    while (d + 1) * (d + 1) <= d2 {
        d += 1;
    }
    if d > radius {
        true // light ground around the disc
    } else if d <= hole_r {
        false // spindle hole
    } else if d <= label_r {
        !(label_ring && d == ring_r) // solid label, maybe one groove
    } else if d <= label_r + 1 || d >= radius - 1 {
        false // crisp black gap round the label, solid black outer rim
    } else {
        d % pitch == 0 // fine white grooves on the black vinyl
    }
}

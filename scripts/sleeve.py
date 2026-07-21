"""presse sleeve composer: turn artwork into the exact 1bpp bitmap a device stores.

A sleeve is the album's cover art. The device draws the *title* itself, at
runtime, from the signed certificate; the bitmap is pure art and carries no
typography on purpose (a baked-in title could disagree with the certificate,
and the certificate is the thing that is actually signed).

Pipeline
--------
    source image (any format/size)
      -> centre square crop (never distorted)
      -> resample to N x N (separable Lanczos-3)
      -> tone curve (percentile autocontrast + gamma)
      -> dither to 1 bit (atkinson | floyd | threshold)
      -> pack 1bpp
      -> SHA-256 of the packed bytes

Packing format
--------------
N x N pixels, 1 bit per pixel, N*N/8 bytes, no header. Bit value 1 = white
(lit), 0 = black, matching the value polarity of the 4bpp glyphs already in
this repo (0 = black .. 15 = white).

N ships at 160 (3200 bytes): 192 does not fit the app's ~32 KB NVRAM data
region, 160 does, measured on device. N must be a multiple of 32 because the
device writes the art in 64-byte cells, so N*N/8 has to divide by 64. Other
sizes (128, 192) still work behind `--size` for bench work.

The scan order is NOT plain row-major. It was derived empirically from two
on-device renders rather than assumed (see docs/art/README.md):

  * `docs/screens/19-art-test.png` -- the ART_TEST prototype fills a
    row-major buffer with a dark square at its top-LEFT; the Flex shows that
    square at the top-RIGHT.
  * `docs/screens/22-cover-fixed.png` -- decoding the stored 4bpp cover
    column-major, high-nibble-first, reproduces the screen pixel-for-pixel
    (correlation 1.0000 against the screenshot).

Both say the same thing: the display is the row-major decode of the buffer
rotated 90 degrees clockwise. So we pre-rotate 90 degrees counter-clockwise
when packing. For a pixel (x, y) of the image the device should show:

    bit_index = (N - 1 - x) * N + y
    byte      = bit_index // 8
    bit       = 7 - (bit_index % 8)          # MSB = first pixel of the byte

MSB-first is the 1bpp analogue of the high-nibble-first ordering that the
4bpp screenshot confirmed. It is the one part of the convention that is an
assumption rather than a measurement, because a swapped bit order inside a
byte is nearly invisible at glyph scale: run `--test-pattern`, upload it, and
look. The pattern is asymmetric under every flip, rotation and bit reversal,
so one glance settles it. `--invert` flips the polarity if 1 turns out to
mean black.

Determinism
-----------
Same input bytes + same options => byte-identical output. Guaranteed:

  * every arithmetic step (crop, resample, tone curve, dither, pack) is pure
    Python float64 in this file, so no library version, SIMD path or CPU
    changes a pixel. Pillow only decodes the input file and writes the
    preview; it never touches the packed bytes.
  * no timestamps, no randomness (no RNG is imported), no locale-dependent
    parsing or formatting, no dependence on filesystem or dict ordering.
  * the preview PNG is a nearest-neighbour blow-up of the same bits, so it
    cannot disagree with the asset.

Recompute the hash from the source with the same flags to verify an asset.
"""

from __future__ import annotations

import argparse
import hashlib
import math
import os
import sys

from PIL import Image

DITHERS = ("atkinson", "floyd", "threshold")

# Atkinson throws away a quarter of the quantisation error, which biases
# every midtone slightly dark; photographs come out heavier than the eye
# expects. A small gamma lift cancels it. Solid black and solid white are
# fixed points of a gamma curve, so this never touches flat graphics -- only
# the tones in between move.
DEFAULT_PHOTO_GAMMA = 1.2
DEFAULT_GAMMA_WHY = "the note by DEFAULT_PHOTO_GAMMA in sleeve.py"


# --------------------------------------------------------------------------
# Grayscale buffers: (width, height, [float 0..255] in row-major order)
# --------------------------------------------------------------------------

def load_gray(path: str) -> tuple[int, int, list[float]]:
    with Image.open(path) as im:
        im = im.convert("L")
        w, h = im.size
        return w, h, [float(v) for v in im.getdata()]


def center_square(w: int, h: int, px: list[float]) -> tuple[int, list[float]]:
    """Centre crop to a square. Never distorts; ties break towards the top-left."""
    s = min(w, h)
    x0 = (w - s) // 2
    y0 = (h - s) // 2
    out = []
    for y in range(y0, y0 + s):
        row = y * w
        out.extend(px[row + x0:row + x0 + s])
    return s, out


def _box_reduce(s: int, px: list[float], k: int) -> tuple[int, list[float]]:
    """Exact k x k box average. Cheap pre-pass so Lanczos never needs a huge
    kernel on big sources; k divides the retained area, the remainder is
    dropped symmetrically."""
    n = s // k
    keep = n * k
    off = (s - keep) // 2
    out = [0.0] * (n * n)
    inv = 1.0 / (k * k)
    for oy in range(n):
        base = oy * n
        for ox in range(n):
            acc = 0.0
            for dy in range(k):
                row = (off + oy * k + dy) * s + off + ox * k
                acc += sum(px[row:row + k])
            out[base + ox] = acc * inv
    return n, out


def _lanczos(x: float) -> float:
    if x < 0.0:
        x = -x
    if x >= 3.0:
        return 0.0
    if x < 1e-12:
        return 1.0
    px = math.pi * x
    return (math.sin(px) / px) * (math.sin(px / 3.0) / (px / 3.0))


def _coeffs(src: int, dst: int) -> list[tuple[int, list[float]]]:
    """Per-output-pixel (start, weights) for a separable Lanczos-3 pass."""
    scale = src / dst
    fscale = scale if scale > 1.0 else 1.0
    support = 3.0 * fscale
    out = []
    for i in range(dst):
        center = (i + 0.5) * scale
        lo = int(center - support + 0.5)
        hi = int(center + support + 0.5)
        if lo < 0:
            lo = 0
        if hi > src:
            hi = src
        ws = [_lanczos((j + 0.5 - center) / fscale) for j in range(lo, hi)]
        total = sum(ws)
        if total != 0.0:
            ws = [w / total for w in ws]
        out.append((lo, ws))
    return out


def resample_square(s: int, px: list[float], n: int) -> list[float]:
    """Square -> n x n, separable Lanczos-3, with an integer box pre-pass."""
    if s == n:
        return list(px)
    while s >= n * 4:
        s, px = _box_reduce(s, px, 2)
    cs = _coeffs(s, n)
    # horizontal
    tmp = [0.0] * (n * s)
    for y in range(s):
        row = y * s
        orow = y * n
        for i, (lo, ws) in enumerate(cs):
            acc = 0.0
            base = row + lo
            for k, wgt in enumerate(ws):
                acc += px[base + k] * wgt
            tmp[orow + i] = acc
    # vertical
    out = [0.0] * (n * n)
    for j, (lo, ws) in enumerate(cs):
        orow = j * n
        for x in range(n):
            acc = 0.0
            for k, wgt in enumerate(ws):
                acc += tmp[(lo + k) * n + x] * wgt
            v = acc
            out[orow + x] = 0.0 if v < 0.0 else (255.0 if v > 255.0 else v)
    return out


# --------------------------------------------------------------------------
# Tone
# --------------------------------------------------------------------------

def tone(px: list[float], gamma: float, clip: float) -> list[float]:
    """Percentile autocontrast then gamma.

    The stretch is what lets a flat, low-contrast photo survive the drop to
    one bit; `clip` is the fraction of pixels allowed to burn out at each
    end. Already-full-range art (any of the generated designs) is unchanged
    by the stretch, so graphics keep their exact blacks and whites.
    """
    n = len(px)
    if clip > 0.0:
        hist = [0] * 256
        for v in px:
            iv = int(v)
            hist[255 if iv > 255 else (0 if iv < 0 else iv)] += 1
        cut = int(n * clip)
        acc = 0
        lo = 0
        for i in range(256):
            acc += hist[i]
            if acc > cut:
                lo = i
                break
        acc = 0
        hi = 255
        for i in range(255, -1, -1):
            acc += hist[i]
            if acc > cut:
                hi = i
                break
        if hi - lo >= 8:
            k = 255.0 / (hi - lo)
            px = [(v - lo) * k for v in px]
            px = [0.0 if v < 0.0 else (255.0 if v > 255.0 else v) for v in px]
    if gamma != 1.0:
        inv = 1.0 / gamma
        px = [255.0 * ((v / 255.0) ** inv) for v in px]
    return px


# --------------------------------------------------------------------------
# Dithering -> list of 0/1, 1 = white
# --------------------------------------------------------------------------

# (dx, dy, numerator); denominator is applied by the caller.
_ATKINSON = ((1, 0, 1), (2, 0, 1), (-1, 1, 1), (0, 1, 1), (1, 1, 1), (0, 2, 1))
_FLOYD = ((1, 0, 7), (-1, 1, 3), (0, 1, 5), (1, 1, 1))


def _diffuse(n: int, px: list[float], kernel, denom: float, thr: float) -> list[int]:
    buf = list(px)
    bits = [0] * (n * n)
    for y in range(n):
        row = y * n
        for x in range(n):
            old = buf[row + x]
            new = 255.0 if old >= thr else 0.0
            bits[row + x] = 1 if new > 0.0 else 0
            err = (old - new) / denom
            if err == 0.0:
                continue
            for dx, dy, num in kernel:
                nx = x + dx
                ny = y + dy
                if 0 <= nx < n and ny < n:
                    buf[ny * n + nx] += err * num
    return bits


def dither(name: str, n: int, px: list[float], thr: float) -> list[int]:
    if name == "atkinson":
        # Only 6/8 of the error is propagated: the classic Mac Plus look,
        # which keeps blacks black and whites white instead of smearing a
        # gradient across the whole frame. Costs some tonal accuracy in the
        # extremes, which is exactly the crispness we want at 160px.
        return _diffuse(n, px, _ATKINSON, 8.0, thr)
    if name == "floyd":
        return _diffuse(n, px, _FLOYD, 16.0, thr)
    if name == "threshold":
        return [1 if v >= thr else 0 for v in px]
    raise ValueError(f"unknown dither {name!r}")


# --------------------------------------------------------------------------
# Packing / preview
# --------------------------------------------------------------------------

def pack_1bpp(n: int, bits: list[int], invert: bool = False) -> bytes:
    """See the module docstring: pre-rotated 90 CCW, MSB = first pixel."""
    out = bytearray(n * n // 8)
    for y in range(n):
        row = y * n
        for x in range(n):
            v = bits[row + x]
            if invert:
                v ^= 1
            if v:
                k = (n - 1 - x) * n + y
                out[k >> 3] |= 0x80 >> (k & 7)
    return bytes(out)


def unpack_1bpp(n: int, data: bytes, invert: bool = False) -> list[int]:
    """Inverse of pack_1bpp, so a stored asset can be re-previewed."""
    bits = [0] * (n * n)
    for y in range(n):
        for x in range(n):
            k = (n - 1 - x) * n + y
            v = 1 if data[k >> 3] & (0x80 >> (k & 7)) else 0
            bits[y * n + x] = v ^ 1 if invert else v
    return bits


def write_preview(path: str, n: int, bits: list[int], scale: int) -> None:
    im = Image.new("1", (n, n))
    im.putdata(bits)
    im = im.resize((n * scale, n * scale), Image.NEAREST)
    im.save(path, optimize=True)


# --------------------------------------------------------------------------
# Test pattern
# --------------------------------------------------------------------------

def test_pattern(n: int) -> list[int]:
    """Asymmetric under every flip, rotation and bit reversal, so one look at
    a device settles the packing convention.

      * main diagonal, top-left to bottom-right (finds a transpose)
      * solid n/8 square at the TOP-LEFT, starting on row 2 (finds any
        rotation or flip)
      * single lit pixel on row 1, two columns in from the RIGHT edge
      * a 3-pixel band along the BOTTOM edge only
      * row 0 is a ruler of 8 lit / 8 dark, starting LIT at x=0: one byte per
        run, so a reversed bit order inside a byte shifts the runs by half a
        period and the row starts dark instead. Written last, with explicit
        zeroes, so nothing else can fill its gaps.
    """
    q = max(4, n // 8)
    bits = [0] * (n * n)
    for i in range(n):
        bits[i * n + i] = 1
    for y in range(2, 2 + q):
        for x in range(q):
            bits[y * n + x] = 1
    bits[1 * n + (n - 2)] = 1
    for y in range(n - 3, n):
        for x in range(n):
            bits[y * n + x] = 1
    for x in range(n):
        bits[x] = 1 if (x // 8) % 2 == 0 else 0
    return bits


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------

def compose(path_or_gray, n: int, dither_name: str, gamma: float,
            clip: float, thr: float) -> list[int]:
    """Source (file path, or an already-built (size, pixels) tuple) -> bits."""
    if isinstance(path_or_gray, tuple):
        s, px = path_or_gray
    else:
        w, h, px = load_gray(path_or_gray)
        s, px = center_square(w, h, px)
    px = resample_square(s, px, n)
    px = tone(px, gamma, clip)
    return dither(dither_name, n, px, thr)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        prog="sleeve.py",
        description="Compose an album sleeve into a packed 1bpp device bitmap.")
    src = p.add_mutually_exclusive_group(required=True)
    src.add_argument("--in", dest="src", metavar="IMAGE", help="source image file")
    src.add_argument("--design", metavar="NAME",
                     help="generated demo cover (see scripts/sleeve_art.py; "
                          "'list' names them)")
    src.add_argument("--test-pattern", action="store_true",
                     help="emit the packing-convention probe pattern")
    p.add_argument("--size", type=int, default=160,
                   help="edge in pixels, multiple of 32 (default 160, the "
                        "size that ships)")
    p.add_argument("--dither", choices=DITHERS, default="atkinson",
                   help="default atkinson")
    p.add_argument("--out", help="packed .bin to write")
    p.add_argument("--preview", help="preview .png to write")
    p.add_argument("--preview-scale", type=int, default=4, help="default 4")
    p.add_argument("--gamma", type=float, default=None,
                   help=">1 lightens midtones before dithering. Default 1.2 "
                        f"for --in (see {DEFAULT_GAMMA_WHY}), 1.0 for --design "
                        "and --test-pattern")
    p.add_argument("--clip", type=float, default=0.005,
                   help="autocontrast percentile per end, 0 disables "
                        "(default 0.005)")
    p.add_argument("--threshold", type=float, default=128.0,
                   help="black/white decision level, 0..255 (default 128)")
    p.add_argument("--invert", action="store_true",
                   help="pack 1 = black instead of 1 = white")
    a = p.parse_args(argv)

    if a.size % 32 or a.size < 32:
        # 64-byte NVM cells on the device: N*N/8 must divide by 64.
        p.error("--size must be a positive multiple of 32")
    if a.preview_scale < 1:
        p.error("--preview-scale must be >= 1")

    if a.test_pattern:
        bits = test_pattern(a.size)
    elif a.design:
        import sleeve_art
        if a.design == "list":
            for name in sleeve_art.names():
                print(f"{name}\t{sleeve_art.DESIGNS[name].title}")
            return 0
        if a.design not in sleeve_art.names():
            p.error(f"unknown design {a.design!r}; --design list to see them")
        d = sleeve_art.DESIGNS[a.design]
        # A design is already composed in full range, so it gets neither the
        # autocontrast stretch nor the photo gamma unless asked.
        bits = compose(d.render(), a.size, a.dither,
                       d.gamma if a.gamma is None else a.gamma,
                       0.0, a.threshold)
    else:
        gamma = DEFAULT_PHOTO_GAMMA if a.gamma is None else a.gamma
        bits = compose(a.src, a.size, a.dither, gamma, a.clip, a.threshold)

    data = pack_1bpp(a.size, bits, a.invert)
    if a.out:
        os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
        with open(a.out, "wb") as f:
            f.write(data)
    if a.preview:
        os.makedirs(os.path.dirname(os.path.abspath(a.preview)), exist_ok=True)
        write_preview(a.preview, a.size, bits, a.preview_scale)

    print(f"{hashlib.sha256(data).hexdigest()}  {a.size}x{a.size} 1bpp "
          f"{len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    raise SystemExit(main())

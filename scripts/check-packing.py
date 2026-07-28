"""Measure the 1bpp packing convention against a real device render.

Decodes a stored sleeve under each candidate convention and correlates every
one against the on-screen pixels, so bit order and polarity are established
rather than assumed. Usage:

    check-packing.py <sleeve.bin> <screenshot.png> [N]
"""

import sys

from PIL import Image


def decode(data: bytes, n: int, rotate: bool, msb: bool, invert: bool) -> list[int]:
    """Unpack to a row-major n*n list of 0/1, image order."""
    bits = [0] * (n * n)
    for y in range(n):
        for x in range(n):
            k = (n - 1 - x) * n + y if rotate else y * n + x
            byte = data[k >> 3]
            mask = (0x80 >> (k & 7)) if msb else (1 << (k & 7))
            v = 1 if byte & mask else 0
            bits[y * n + x] = v ^ 1 if invert else v
    return bits


def screen_bits(path: str, n: int) -> list[int] | None:
    """Find the n*n sleeve in a screenshot and return it as 0/1, 1 = white."""
    im = Image.open(path).convert("L")
    px = im.load()
    w, h = im.size
    # The sleeve is the only large non-white block; bound it.
    xs, ys = [], []
    for y in range(h):
        for x in range(w):
            if px[x, y] < 128:
                xs.append(x)
                ys.append(y)
    if not xs:
        return None
    x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    # Text under the image would widen the box; keep the top n rows square.
    x0 = max(0, min(x0, x1 - n + 1))
    y0 = max(0, min(y0, y1 - n + 1))
    if x0 + n > w or y0 + n > h:
        return None
    return [1 if px[x0 + i, y0 + j] >= 128 else 0 for j in range(n) for i in range(n)]


def main() -> int:
    src, shot = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 128
    data = open(src, "rb").read()
    observed = screen_bits(shot, n)
    if observed is None:
        print("could not locate the sleeve in the screenshot")
        return 1

    results = []
    for rotate in (True, False):
        for msb in (True, False):
            for invert in (True, False):
                expected = decode(data, n, rotate, msb, invert)
                same = sum(1 for a, b in zip(expected, observed) if a == b)
                results.append((same / len(observed), rotate, msb, invert))
    results.sort(reverse=True)
    print(f"{'match':>8}  rotate  msb-first  invert")
    for score, rotate, msb, invert in results:
        print(f"{score:8.4f}  {rotate!s:6}  {msb!s:9}  {invert}")
    best = results[0]
    print()
    print(f"best: rotate={best[1]} msb_first={best[2]} invert={best[3]} at {best[0]:.4f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

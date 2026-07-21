"""Generate the vinyl record icons (grayscale GIFs) for the device app.
Run in WSL: python3 scripts/make-icons.py"""

import os

from PIL import Image, ImageDraw

ROOT = os.path.join(os.path.dirname(__file__), "..", "device-app")
SS = 8  # supersampling factor


def vinyl(size: int) -> Image.Image:
    n = size * SS
    img = Image.new("L", (n, n), 255)
    d = ImageDraw.Draw(img)
    c = n / 2
    disc_r = n * 0.48

    def circle(r, **kw):
        d.ellipse([c - r, c - r, c + r, c + r], **kw)

    circle(disc_r, fill=25)
    for frac in (0.62, 0.74, 0.86, 0.97):
        circle(disc_r * frac, outline=110, width=max(SS // 2, 1))
    circle(disc_r * 0.32, fill=200)
    circle(disc_r * 0.30, outline=140, width=max(SS // 2, 1))
    circle(disc_r * 0.07, fill=255)
    return img.resize((size, size), Image.LANCZOS)


def sleeve(size: int) -> Image.Image:
    """Empty record sleeve: outline square, center hole, nothing inside."""
    n = size * SS
    img = Image.new("L", (n, n), 255)
    d = ImageDraw.Draw(img)
    m = n * 0.06
    d.rounded_rectangle([m, m, n - m, n - m], radius=n * 0.06, outline=60,
                        width=max(SS // 2, 1) * 2)
    c = n / 2
    r = n * 0.13
    d.ellipse([c - r, c - r, c + r, c + r], outline=140, width=max(SS // 2, 1))
    return img.resize((size, size), Image.LANCZOS)


def press(size: int) -> Image.Image:
    """The press: a vinyl under the stamper plate."""
    n = size * SS
    img = Image.new("L", (n, n), 255)
    d = ImageDraw.Draw(img)
    c = n / 2
    plate_h = n * 0.14
    d.rounded_rectangle([n * 0.12, 0, n * 0.88, plate_h], radius=n * 0.04, fill=60)
    d.rectangle([c - n * 0.05, plate_h, c + n * 0.05, n * 0.30], fill=60)
    disc_c = n * 0.62
    disc_r = n * 0.36

    def circle(r, **kw):
        d.ellipse([c - r, disc_c - r, c + r, disc_c + r], **kw)

    circle(disc_r, fill=25)
    for frac in (0.62, 0.84):
        circle(disc_r * frac, outline=110, width=max(SS // 2, 1))
    circle(disc_r * 0.30, fill=200)
    circle(disc_r * 0.07, fill=255)
    return img.resize((size, size), Image.LANCZOS)


def vinyl_variant(size: int, seed: int) -> Image.Image:
    """Album artwork: same record, distinct label art per variant. The app
    picks variant = album_id % 8, so every album keeps a stable look."""
    n = size * SS
    img = Image.new("L", (n, n), 255)
    d = ImageDraw.Draw(img)
    c = n / 2
    disc_r = n * 0.48

    def circle(r, **kw):
        d.ellipse([c - r, c - r, c + r, c + r], **kw)

    circle(disc_r, fill=25)
    groove_sets = [(0.60, 0.72, 0.86), (0.58, 0.66, 0.74, 0.90), (0.64, 0.88),
                   (0.56, 0.70, 0.84, 0.95), (0.62, 0.78, 0.93), (0.59, 0.81),
                   (0.57, 0.68, 0.79, 0.91), (0.66, 0.75, 0.89)]
    for frac in groove_sets[seed % 8]:
        circle(disc_r * frac, outline=110, width=max(SS // 2, 1))
    label_r = disc_r * 0.34
    label_shade = [210, 170, 120, 235, 190, 145, 220, 95][seed % 8]
    circle(label_r, fill=label_shade)
    ink = 40 if label_shade > 150 else 240
    wedges = 2 + (seed % 4)
    import math

    for w in range(wedges):
        ang = (2 * math.pi / wedges) * w + seed
        x1 = c + label_r * 0.45 * math.cos(ang)
        y1 = c + label_r * 0.45 * math.sin(ang)
        x2 = c + label_r * 0.85 * math.cos(ang)
        y2 = c + label_r * 0.85 * math.sin(ang)
        d.line([x1, y1, x2, y2], fill=ink, width=max(SS, 2))
    circle(disc_r * 0.055, fill=255)
    return img.resize((size, size), Image.LANCZOS)


def save_gif(img: Image.Image, path: str):
    img.convert("P").save(path, format="GIF")
    print(f"wrote {path} ({img.size[0]}x{img.size[1]})")


if __name__ == "__main__":
    for v in range(8):
        save_gif(vinyl_variant(96, v), os.path.join(ROOT, "glyphs", f"vinyl_v{v}_96x96.gif"))
    save_gif(vinyl(64), os.path.join(ROOT, "glyphs", "vinyl_64x64.gif"))
    save_gif(sleeve(64), os.path.join(ROOT, "glyphs", "sleeve_64x64.gif"))
    save_gif(press(64), os.path.join(ROOT, "glyphs", "press_64x64.gif"))
    save_gif(vinyl(40), os.path.join(ROOT, "icons", "vinyl_40x40.gif"))
    save_gif(vinyl(32), os.path.join(ROOT, "icons", "vinyl_32x32.gif"))

#!/usr/bin/env python3
"""Stitch the captured device screens into docs/demo.gif.

Each beat is composited as [Flex A screen] | [plain-language caption] |
[Flex B screen] on warm paper, mirroring the two-screen cockpit. Key beats
(identical 4 words, the cover landing on B, the GENUINE verdict) are held
longer by duplicating frames. Run by scripts/record-demo.sh after a capture;
reads docs/screens/frames/raw/*.png, writes docs/demo.gif."""

import os
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RAW = os.path.join(ROOT, "docs", "screens", "frames", "raw")
OUT = os.path.join(ROOT, "docs", "demo.gif")

# --- canvas / palette ----------------------------------------------------
W, H = 940, 430
PAPER = (244, 239, 230)
INK = (26, 24, 22)
MUTED = (122, 114, 104)
ACCENT = (188, 82, 48)      # terracotta
SHELL = (250, 248, 244)
SHELL_EDGE = (208, 203, 195)
DIVIDER = (222, 216, 206)

SW = 208                    # device screen width on canvas
SH = round(SW * 600 / 480)  # 260
Y0 = 84                     # top of the device screens
XL = 40                     # left screen x
XR = W - 40 - SW            # right screen x
CX = (XL + SW + XR) // 2    # center of the caption column


def font(bold, size):
    names = (
        ["DejaVuSans-Bold.ttf", "DejaVuSans-Bold.ttf"]
        if bold else ["DejaVuSans.ttf"]
    )
    paths = []
    for n in names:
        paths += [
            f"/usr/share/fonts/truetype/dejavu/{n}",
            f"/usr/share/fonts/dejavu/{n}",
            n,
        ]
    for p in paths:
        try:
            return ImageFont.truetype(p, size)
        except OSError:
            continue
    return ImageFont.load_default()


F_MARK = font(True, 21)
F_KICK = font(True, 15)
F_CAP = font(True, 27)
F_SUB = font(False, 19)
F_LABEL = font(False, 14)
F_BADGE = font(True, 24)


def rrect(d, box, r, fill=None, outline=None, width=1):
    d.rounded_rectangle(box, radius=r, fill=fill, outline=outline, width=width)


def text_center(d, cx, y, s, f, fill):
    w = d.textlength(s, font=f)
    d.text((cx - w / 2, y), s, font=f, fill=fill)
    return w


def wrap(d, s, f, maxw):
    out, line = [], ""
    for word in s.split():
        trial = (line + " " + word).strip()
        if d.textlength(trial, font=f) <= maxw or not line:
            line = trial
        else:
            out.append(line)
            line = word
    if line:
        out.append(line)
    return out


def kern(d, cx, y, s, f, fill, sp):
    # letter-spaced, centered
    total = sum(d.textlength(c, font=f) + sp for c in s) - sp
    x = cx - total / 2
    for c in s:
        d.text((x, y), c, font=f, fill=fill)
        x += d.textlength(c, font=f) + sp


def device(canvas, path, x, label):
    shell = (x - 11, Y0 - 11, x + SW + 11, Y0 + SH + 11)
    d = ImageDraw.Draw(canvas)
    rrect(d, shell, 16, fill=SHELL, outline=SHELL_EDGE, width=1)
    scr = Image.open(path).convert("RGB").resize((SW, SH), Image.LANCZOS)
    canvas.paste(scr, (x, Y0))
    d.rectangle((x, Y0, x + SW - 1, Y0 + SH - 1), outline=(214, 210, 202), width=1)
    text_center(d, x + SW / 2, Y0 + SH + 22, label, F_LABEL, MUTED)


def wire(d, phase):
    # An animated "untrusted relay" wire under the two devices: marching dots
    # imply the APDU traffic shuttling A <-> B through the hostile laptop.
    y = 406
    x0, x1 = 78, W - 78
    d.line((x0, y, x1, y), fill=(228, 222, 212), width=2)
    # outward arrowheads (bidirectional shuttle)
    for xa, dx in ((x0, -1), (x1, 1)):
        d.polygon([(xa + dx * 8, y - 5), (xa + dx * 8, y + 5), (xa, y)],
                  fill=(206, 200, 190))
    spacing, r = 26, 3
    off = int(phase * 7) % spacing
    x = x0 + 12 + off
    while x < x1 - 12:
        d.ellipse((x - r, y - r, x + r, y + r), fill=ACCENT)
        x += spacing
    # label pill masking the rail at center
    s = "untrusted relay"
    tw = d.textlength(s, font=F_LABEL)
    box = (W / 2 - tw / 2 - 12, y - 12, W / 2 + tw / 2 + 12, y + 12)
    rrect(d, box, 12, fill=PAPER, outline=(220, 214, 204), width=1)
    d.text((W / 2 - tw / 2, y - F_LABEL.size / 2 - 1), s, font=F_LABEL, fill=MUTED)


def badge(d, cx, y, s):
    w = d.textlength(s, font=F_BADGE)
    padx, pady = 22, 11
    box = (cx - w / 2 - padx, y, cx + w / 2 + padx, y + F_BADGE.size + 2 * pady)
    rrect(d, box, (box[3] - box[1]) / 2, fill=INK)
    d.text((cx - w / 2, y + pady - 2), s, font=F_BADGE, fill=(247, 244, 238))
    return box[3] - box[1]


def frame(a_png, b_png, kicker, main, sub, badge_text=None, phase=0):
    img = Image.new("RGB", (W, H), PAPER)
    d = ImageDraw.Draw(img)

    # header
    kern(d, W / 2, 22, "ENCLAVE RECORDS", F_MARK, INK, 3)
    text_center(d, W / 2, 50, "finite editions of a digital work, enforced by silicon",
                F_SUB, MUTED)
    d.line((40, 74, W - 40, 74), fill=DIVIDER, width=1)

    device(img, os.path.join(RAW, a_png + ".png"), XL, "Flex A  ·  master")
    device(img, os.path.join(RAW, b_png + ".png"), XR, "Flex B  ·  receiver")

    colw = XR - (XL + SW) - 44
    ccx = CX

    # kicker
    ky = 138
    kern(d, ccx, ky, kicker, F_KICK, ACCENT, 2)

    # caption main (wrapped)
    lines = wrap(d, main, F_CAP, colw)
    y = ky + 30
    for ln in lines:
        text_center(d, ccx, y, ln, F_CAP, INK)
        y += F_CAP.size + 6

    y += 6
    if badge_text:
        badge(d, ccx, y, badge_text)
        y += F_BADGE.size + 30
    if sub:
        for ln in wrap(d, sub, F_SUB, colw):
            text_center(d, ccx, y, ln, F_SUB, MUTED)
            y += F_SUB.size + 4

    wire(d, phase)
    return img


# --- storyboard ----------------------------------------------------------
# (a_png, b_png, kicker, main, sub, badge, hold_frames)
BEATS = [
    ("a-empty", "b-empty", "TWO LEDGER FLEX",
     "A master and a receiver",
     "the laptop between them is untrusted", None, 6),
    ("a-cut", "b-empty", "STEP 1  ·  CUT",
     "Cut the master",
     "edition of 5, sealed in silicon forever", None, 9),
    ("a-sas", "b-sas", "STEP 2  ·  PAIR",
     "Both screens show the SAME 4 words",
     "the humans compare them; a lying relay makes them differ", None, 13),
    ("a-press", "b-receive", "STEP 3  ·  PRESS",
     "Press 1 of 5",
     "the counter drops in silicon, bound to this receiver's chip", None, 9),
    ("a-card", "b-card", "STEP 3  ·  PRESS",
     "The cover travels with the pressing",
     "numbered 1 of 5, bound to this device", None, 11),
    ("a-card", "b-prov", "STEP 4  ·  VERIFY",
     "Verified offline",
     "pressing 1 of 5  ·  no server, no chain", "GENUINE", 15),
]


def main():
    frames, durations = [], []
    base = 180  # ms per frame -> ~5.5 fps
    phase = 0
    for a, b, k, m, s, bd, hold in BEATS:
        for _ in range(hold):
            frames.append(frame(a, b, k, m, s, bd, phase=phase))
            durations.append(base)
            phase += 1

    # quantize to a shared adaptive palette for crisp, small output
    pal_src = frames[-1].quantize(colors=128, method=Image.MEDIANCUT)
    qframes = [fr.quantize(colors=128, palette=pal_src, dither=Image.NONE)
               for fr in frames]

    qframes[0].save(
        OUT, save_all=True, append_images=qframes[1:], loop=0,
        duration=durations, optimize=True, disposal=1,
    )
    size = os.path.getsize(OUT)
    print(f"wrote {OUT}")
    print(f"  {len(qframes)} frames, {W}x{H}, {size/1e6:.2f} MB")


if __name__ == "__main__":
    main()

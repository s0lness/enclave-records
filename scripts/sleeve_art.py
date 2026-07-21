"""Generated demo sleeves for the presse library grid.

Each design draws a grayscale square at SS resolution in normalised
coordinates; sleeve.py then does the one and only downscale + dither, so a
generated cover goes through exactly the same pipeline as an artist's photo.

Every design is original work made of primitive shapes and gradients. None of
them reproduces an existing album cover; `ram` is a graphic homage (two robot
helmets, a stage light, a table edge), not a copy of any photograph.

Drawing convention: coordinates are 0..1, y grows downward, values are
0 (black) .. 255 (white). Solid areas are painted exactly 0 or 255 so the
dither leaves them alone; only the gradients pick up texture.
"""

from __future__ import annotations

import math

# 6x the shipped 160x160 target: an integer supersample means the structural
# grid below survives the downscale exactly.
GRID = 160
SS = GRID * 6


class Canvas:
    def __init__(self, s: int = SS, v: float = 0.0):
        self.s = s
        self.px = [float(v)] * (s * s)

    # -- helpers ----------------------------------------------------------
    def _span(self, a: float, b: float) -> tuple[int, int]:
        lo = int(math.floor(a * self.s))
        hi = int(math.ceil(b * self.s))
        return max(0, lo), min(self.s, hi)

    def _set(self, x: int, y: int, v: float) -> None:
        self.px[y * self.s + x] = v

    # -- primitives -------------------------------------------------------
    def rect(self, x0: float, y0: float, x1: float, y1: float, v: float) -> None:
        xa, xb = self._span(x0, x1)
        ya, yb = self._span(y0, y1)
        for y in range(ya, yb):
            row = y * self.s
            for x in range(xa, xb):
                self.px[row + x] = v

    def shade(self, x0: float, y0: float, x1: float, y1: float, fn) -> None:
        """fn(u, v) -> value, or None to leave the pixel alone."""
        xa, xb = self._span(x0, x1)
        ya, yb = self._span(y0, y1)
        inv = 1.0 / self.s
        for y in range(ya, yb):
            row = y * self.s
            vy = (y + 0.5) * inv
            for x in range(xa, xb):
                r = fn((x + 0.5) * inv, vy)
                if r is not None:
                    self.px[row + x] = r

    def disc(self, cx: float, cy: float, r: float, v: float) -> None:
        r2 = r * r
        self.shade(cx - r, cy - r, cx + r, cy + r,
                   lambda u, w: v if (u - cx) ** 2 + (w - cy) ** 2 <= r2 else None)

    def ring(self, cx: float, cy: float, r: float, t: float, v: float) -> None:
        ri, ro = (r - t / 2) ** 2, (r + t / 2) ** 2
        self.shade(cx - r - t, cy - r - t, cx + r + t, cy + r + t,
                   lambda u, w: v if ri <= (u - cx) ** 2 + (w - cy) ** 2 <= ro else None)

    def rrect(self, x0: float, y0: float, x1: float, y1: float, rad: float,
              v: float) -> None:
        ix0, iy0, ix1, iy1 = x0 + rad, y0 + rad, x1 - rad, y1 - rad
        r2 = rad * rad

        def f(u, w):
            cx = ix0 if u < ix0 else (ix1 if u > ix1 else u)
            cy = iy0 if w < iy0 else (iy1 if w > iy1 else w)
            return v if (u - cx) ** 2 + (w - cy) ** 2 <= r2 else None

        self.shade(x0, y0, x1, y1, f)

    def line(self, x0: float, y0: float, x1: float, y1: float, t: float,
             v: float) -> None:
        dx, dy = x1 - x0, y1 - y0
        ll = dx * dx + dy * dy
        h = t / 2
        h2 = h * h

        def f(u, w):
            if ll == 0.0:
                s = 0.0
            else:
                s = ((u - x0) * dx + (w - y0) * dy) / ll
                s = 0.0 if s < 0.0 else (1.0 if s > 1.0 else s)
            px_, py_ = x0 + s * dx, y0 + s * dy
            return v if (u - px_) ** 2 + (w - py_) ** 2 <= h2 else None

        self.shade(min(x0, x1) - t, min(y0, y1) - t,
                   max(x0, x1) + t, max(y0, y1) + t, f)

    def result(self) -> tuple[int, list[float]]:
        return self.s, self.px


def _clamp(v: float) -> float:
    return 0.0 if v < 0.0 else (255.0 if v > 255.0 else v)


def g(px: float) -> float:
    """Normalised coordinate for `px` on the 160-pixel design grid.

    Thin rules have to land on whole target pixels, otherwise the resample
    spreads a 1.4-pixel line over two rows of mid-gray and the dither turns
    it into a dashed line. Everything structural is expressed in these units,
    so the designs are pixel-exact at the shipped 160 and merely good at the
    other sizes.
    """
    return px / GRID


# ---------------------------------------------------------------------------
# Designs
# ---------------------------------------------------------------------------

def ram() -> tuple[int, list[float]]:
    """"Random Access Memories" -- two robot helmets under a stage light.

    Original graphic homage: primitive shapes only, no photograph involved.
    """
    c = Canvas(v=0.0)

    # Stage light: a hard white ring the helmets stand in front of, plus a
    # faint wash inside it. The ring is a drawn element rather than a bright
    # gradient on purpose -- a gradient strong enough to see behind a white
    # subject dithers into a band that reads as hair, whereas a 2px circle
    # stays a circle at every size.
    c.disc(0.5, g(66), g(54), 55.0)
    c.ring(0.5, g(66), g(54), g(2), 255.0)

    def helmet(cx: float, top: float, w: float, h: float, shell_v: float,
               glint: bool) -> None:
        left, right, bot = cx - w / 2, cx + w / 2, top + h
        rad = w * 0.44
        # Black keyline first, shell on top: the two helmets touch, and this
        # is what keeps them from fusing into one blob.
        c.rrect(left - g(2), top - g(2), right + g(2), bot + g(2),
                rad + g(2), 0.0)
        # A flat shell value, not a gradient: one solid white and one even
        # mid-tone dither the same way at every size, and the pair reads as
        # two materials rather than as noise.
        c.rrect(left, top, right, bot, rad, shell_v)
        # Visor: a hard black band, the feature that makes the silhouette
        # read as a helmet at all.
        vt, vb = top + h * 0.30, top + h * 0.56
        c.rrect(left + w * 0.05, vt, right - w * 0.05, vb, (vb - vt) / 2, 0.0)
        if glint:
            c.rect(left + w * 0.19, vt + (vb - vt) * 0.28,
                   left + w * 0.44, vt + (vb - vt) * 0.46, 255.0)
        else:
            c.rect(right - w * 0.42, vt + (vb - vt) * 0.28,
                   right - w * 0.19, vt + (vb - vt) * 0.46, 255.0)
        # Chin band.
        c.rect(left + w * 0.24, bot - h * 0.16, right - w * 0.24,
               bot - h * 0.16 + g(2), 0.0)

    helmet(0.300, g(46), 0.300, 0.400, shell_v=255.0, glint=True)
    helmet(0.700, g(53), 0.300, 0.400, shell_v=150.0, glint=False)

    # Table edge, then solid black below it.
    c.rect(0.0, g(129), 1.0, 1.0, 0.0)
    c.rect(g(10), g(129), g(150), g(132), 255.0)
    return c.result()


def eclipse() -> tuple[int, list[float]]:
    """"Solar Debt" -- a black sun with a dithered corona over a hard horizon."""
    c = Canvas(v=0.0)
    cx, cy, r = 0.5, 0.42, 0.245

    def corona(u, w):
        d = math.hypot(u - cx, w - cy)
        if d <= r:
            return None
        g = 1.0 - (d - r) / 0.30
        if g <= 0.0:
            return None
        # Faint radial spokes keep the corona from looking like a blur.
        sp = 1.0 + 0.16 * math.cos(12.0 * math.atan2(w - cy, u - cx))
        return _clamp(255.0 * (g ** 1.5) * sp)

    c.shade(0.0, 0.0, 1.0, g(125), corona)
    c.disc(cx, cy, r, 0.0)
    c.ring(cx, cy, r + g(1), g(2), 255.0)
    c.rect(0.0, g(125), 1.0, 1.0, 0.0)
    c.rect(0.0, g(125), 1.0, g(128), 255.0)
    # Three receding rules below the horizon: weight at the bottom of the
    # frame, and a rhythm the eye can hold at thumbnail size.
    for i, y in enumerate((135, 144, 151)):
        inset = g(10 + i * 17)
        c.rect(inset, g(y), 1.0 - inset, g(y + 2), 255.0)
    return c.result()


def monolith() -> tuple[int, list[float]]:
    """"Concrete Sleep" -- a black slab standing in a graded field."""
    c = Canvas(v=0.0)
    hz = g(120)
    # Sky: black overhead rising to white at the horizon. The full sweep of
    # one gradient across three quarters of the frame is the widest tonal
    # ramp in the set and the hardest thing the dither has to do.
    c.shade(0.0, 0.0, 1.0, hz,
            lambda u, w: _clamp(255.0 * (w / hz) ** 1.6))
    # Slab: solid black with a white keyline, so it holds its edge against
    # the dark top of the sky as well as the bright bottom.
    c.rect(g(62), g(20), g(98), hz, 255.0)
    c.rect(g(65), g(23), g(95), hz, 0.0)
    # Ground: solid white, so the composition flips polarity at the horizon.
    c.rect(0.0, hz, 1.0, 1.0, 255.0)
    c.rect(0.0, hz, 1.0, g(123), 0.0)

    # Cast shadow: black wedge from the slab's base to the lower right.
    def shadow(u, w):
        t = (w - g(123)) / (1.0 - g(123))
        if t < 0.0:
            return None
        x0 = g(65) + t * g(5)
        x1 = g(95) + t * g(92)
        return 0.0 if x0 <= u <= x1 else None

    c.shade(0.0, g(123), 1.0, 1.0, shadow)
    return c.result()


def transit() -> tuple[int, list[float]]:
    """"Null Island" -- a perspective grid running to a vanishing point."""
    c = Canvas(v=0.0)
    hz = g(88)
    vx = 0.5
    # Sun above the horizon, sliced by scanlines that widen downwards.
    c.disc(vx, g(54), g(28), 255.0)
    y = g(26)
    for i in range(9):
        c.rect(0.0, y, 1.0, y + g(1 + i * 0.5), 0.0)
        y += g(7)
    # Horizon haze, the only dithered area: it separates sky from grid and
    # keeps the frame from being pure line art.
    c.shade(0.0, hz - g(16), 1.0, hz,
            lambda u, w: _clamp(110.0 * max(0.0, 1.0 - (hz - w) / g(16)) ** 2.4))
    c.rect(0.0, hz, 1.0, 1.0, 0.0)
    c.rect(0.0, hz, 1.0, hz + g(2), 255.0)
    # Ground grid: verticals converge on the vanishing point, horizontals
    # space out geometrically towards the viewer. Both are deliberately
    # sparse near the horizon -- lines any closer together than about three
    # pixels stop being lines and become a gray band.
    for k in range(-5, 6):
        c.line(vx + k * g(7.0), hz, vx + k * 0.52, 1.02, g(1.8), 255.0)
    # Rows are placed and thickened in whole grid pixels: a 1.6-pixel rule
    # lands half on two rows and dithers into a dashed line.
    step = 11.0
    yp = 88.0 + 11.0
    while yp < GRID:
        top = int(yp)
        c.rect(0.0, g(top), 1.0, g(top + (2 if top < 130 else 3)), 255.0)
        step *= 1.44
        yp += step
    return c.result()


class Design:
    def __init__(self, title: str, fn, gamma: float = 1.0):
        self.title = title
        self.fn = fn
        self.gamma = gamma

    def render(self) -> tuple[int, list[float]]:
        return self.fn()


DESIGNS = {
    "ram": Design("Random Access Memories", ram),
    "eclipse": Design("Solar Debt", eclipse),
    "monolith": Design("Concrete Sleep", monolith),
    "transit": Design("Null Island", transit),
}


def names() -> list[str]:
    return list(DESIGNS)

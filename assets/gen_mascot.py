#!/usr/bin/env python3
"""aimon mascot: a chibi guard robot — big head, cyan eyes, raised arm — watches an invader
ghost feed on its chest gauge, then zaps it with the pink bolt in its glove. The sprite is
drawn as chunky shaded pixel shapes on a 48x48 grid, presented on a rounded vector canvas.
3s loop: gauge climbs, eyes track, bolt fires, the ghost pops, pressure drops, it respawns.

Run: /tmp/aimon-venv/bin/python assets/gen_mascot.py
"""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image

# Theme (src/theme.rs neon), plus sprite shades chosen for value contrast on the dark ground.
GROUND = (0x0B, 0x0E, 0x1A)
BODY = (0x3A, 0x46, 0x70)
BODY_L = (0x57, 0x64, 0x9C)
BODY_D = (0x23, 0x2C, 0x52)
OUT = (0x0A, 0x0E, 0x1C)
VISOR = (0x0C, 0x10, 0x26)
BRAND = (0xFF, 0x2E, 0x97)
PINK_L = (0xFF, 0x6C, 0xB0)
ACCENT = (0x00, 0xE5, 0xFF)
CYAN_L = (0x7D, 0xF3, 0xFF)
OK = (0x39, 0xFF, 0x14)
WARN = (0xFF, 0xB0, 0x00)
BAD = (0xFF, 0x38, 0x60)
MUTED = (0x8A, 0x98, 0xC4)
WHITE = (0xEA, 0xF0, 0xFF)

GRID = 48
SCALE = 10
FPS = 25
FRAMES = 75  # 3s loop

GHOST_XY = (35, 6)
GHOST = [
    "..yyy.yy.",
    ".yyyyyyy.",
    "yyeyyeyy.",
    "yyyyyyyyy",
    "yyyyyyyyy",
    ".yy.yy.y.",
    "y.y..y.y.",
]
BOLT_CELLS = [(32, 18), (33, 17), (33, 16), (34, 15), (34, 14), (35, 13), (36, 12)]
STAR_CELLS = [(39, 7), (42, 9), (37, 5), (41, 13)]


class Grid:
    def __init__(self) -> None:
        self.px: dict[tuple[int, int], tuple[int, ...]] = {}

    def put(self, x: int, y: int, c: tuple[int, ...]) -> None:
        if 0 <= x < GRID and 0 <= y < GRID:
            self.px[(x, y)] = c

    def rect(self, x0: int, y0: int, x1: int, y1: int, c: tuple[int, ...]) -> None:
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                self.put(x, y, c)

    def rrect(self, x0: int, y0: int, x1: int, y1: int, r: int, fill: tuple[int, ...],
              light: tuple[int, ...] | None = None, dark: tuple[int, ...] | None = None) -> None:
        """Rounded rect, outlined in OUT, optional light band on top / dark band on bottom."""
        self._rrect_rows(x0, y0, x1, y1, r, lambda *_: OUT)
        self._rrect_rows(x0 + 1, y0 + 1, x1 - 1, y1 - 1, max(1, r - 1),
                         lambda x, y, f, l, t: light if l else (dark if t else fill),
                         light_rows=2 if light else 0, dark_rows=2 if dark else 0)

    def _rrect_rows(self, x0: int, y0: int, x1: int, y1: int, r: int, color, light_rows: int = 0,
                    dark_rows: int = 0) -> None:
        h = y1 - y0 + 1
        for y in range(y0, y1 + 1):
            dyt, dyb = y - y0, y1 - y
            off = r - 1 if dyt >= r and dyb >= r else max(
                (r - 1) - int(math.isqrt(max(0, (r - 1) ** 2 - (min(dyt, dyb) - r + 1) ** 2))), 0
            )
            c = color(x0 + off, y, None, dyt < light_rows, dyb < dark_rows)
            for x in range(x0 + off, x1 - off + 1):
                self.put(x, y, c)

    def stamp(self, rows: list[str], x0: int, y0: int, palette: dict[str, tuple[int, ...]]) -> None:
        for dy, row in enumerate(rows):
            for dx, cell in enumerate(row):
                if cell != ".":
                    self.put(x0 + dx, y0 + dy, palette[cell])

    def to_image(self) -> Image.Image:
        img = Image.new("RGBA", (GRID, GRID))
        img.putdata([self.px.get((x, y), (0, 0, 0, 0)) for y in range(GRID) for x in range(GRID)])
        return img


def ease(t: float) -> float:
    return t * t * (3 - 2 * t)


def gauge_frac(t: float) -> float:
    if t < 0.68:
        return t / 0.68
    if t < 0.85:
        return 1.0 - ease((t - 0.68) / 0.17)
    return 0.0


def gauge_color(f: float) -> tuple[int, ...]:
    if f >= 0.85:
        return BAD
    if f >= 0.7:
        return WARN
    return OK


def ghost_alpha(t: float) -> float:
    if t < 0.68:
        return 1.0
    if t < 0.78:
        return 1.0 - (t - 0.68) / 0.10
    if t < 0.92:
        return 0.0
    return (t - 0.92) / 0.08


def draw(t: float) -> Grid:
    g = Grid()
    frac = gauge_frac(t)
    zapping = 0.68 <= t < 0.80
    bob = -1 if math.sin(2 * math.pi * t) > 0 else 0

    # shadow under the feet
    g.rect(11, 45, 24, 45, (0x16, 0x1C, 0x33, 255))

    y = bob  # the whole robot breathes

    # legs + feet
    g.rrect(11, 39 + y, 16, 44 + y, 2, BODY, dark=BODY_D)
    g.rrect(19, 39 + y, 24, 44 + y, 2, BODY, dark=BODY_D)

    # body + chest panel + gauge
    g.rrect(8, 24 + y, 25, 39 + y, 5, BODY, light=BODY_L, dark=BODY_D)
    g.rrect(12, 28 + y, 23, 34 + y, 2, VISOR)
    x0, x1 = 13, 22
    g.rect(x0, 31 + y, x1, 32 + y, (0x05, 0x08, 0x18, 255))
    w = round((x1 - x0 + 1) * frac)
    if w:
        g.rect(x0, 31 + y, x0 + w - 1, 32 + y, gauge_color(frac))
    # belt
    g.rect(10, 35 + y, 23, 35 + y, (0x2C, 0x36, 0x60, 255))

    # left arm resting; right arm raised, pink glove aiming at the ghost
    g.rrect(4, 27 + y, 7, 34 + y, 2, BODY, dark=BODY_D)
    g.rrect(26, 27 + y, 29, 31 + y, 2, BODY, light=BODY_L)
    g.rrect(27, 23 + y, 31, 26 + y, 2, BODY, light=BODY_L)
    glove = BAD if zapping else BRAND
    g.rrect(29, 18 + y, 32, 22 + y, 1, glove, light=(0xFF, 0x6C, 0xB0, 255) if not zapping else None)

    # neck
    g.rect(14, 22 + y, 19, 23 + y, BODY_D)

    # head: big, chibi
    g.rrect(6, 6 + y, 27, 21 + y, 5, BODY, light=BODY_L, dark=BODY_D)
    # ear nubs
    g.rrect(4, 11 + y, 6, 15 + y, 1, MUTED, dark=BODY_D)
    g.rrect(27, 11 + y, 29, 15 + y, 1, MUTED, dark=BODY_D)
    # visor
    g.rrect(9, 10 + y, 25, 18 + y, 4, VISOR)

    # eyes: 4x5 cyan with light top and dark pupils; blink flattens; glance tracks the ghost
    blink = 0.44 <= t < 0.50
    look = 2 if (0.60 <= t < 0.75 or zapping) else (1 if math.sin(2 * math.pi * t * 0.5) > 0 else 0)
    for ex in (11, 19):
        if blink:
            g.rect(ex, 15 + y, ex + 3, 15 + y, ACCENT)
        else:
            g.rect(ex, 12 + y, ex + 3, 16 + y, ACCENT)
            g.rect(ex, 12 + y, ex + 3, 12 + y, CYAN_L)
            px = min(ex + look, ex + 2)
            g.rect(px, 14 + y, px + 1, 15 + y, VISOR)

    # brows: determined, color climbs with pressure
    brow = BAD if frac >= 0.85 else (WARN if frac >= 0.7 else ACCENT)
    g.put(10, 11 + y, brow)
    g.put(11, 11 + y, brow)
    g.put(21, 11 + y, brow)
    g.put(22, 11 + y, brow)

    # antenna: stem + ball that reads the pressure
    g.rect(16, 4 + y, 17, 5 + y, OUT)
    tip = BAD if frac >= 0.85 else (WARN if frac >= 0.7 else ACCENT)
    g.rrect(14, 0 + y, 18, 3 + y, 1, tip, light=(0x7D, 0xF3, 0xFF, 255) if tip == ACCENT else None)

    # the zap: bolt from the glove to the ghost, impact stars
    if zapping:
        for x, yy in BOLT_CELLS:
            g.put(x, yy, BRAND)
        for x, yy in STAR_CELLS:
            g.put(x, yy, WHITE)

    # ghost: flee-bob, X eyes when zapped, dithered respawn, poof ring after
    ga = ghost_alpha(t)
    if ga > 0:
        gy = GHOST_XY[1] + (1 if math.sin(2 * math.pi * t * 2) > 0 else 0)
        dither = ga < 1.0 and (int(t * FPS) % 2 == 0)
        for dy, row in enumerate(GHOST):
            for dx, cell in enumerate(row):
                if cell == ".":
                    continue
                if dither and (dx + dy) % 2 == 0:
                    continue
                g.put(GHOST_XY[0] + dx, gy + dy, WARN if cell == "y" else OUT)
        for dx in (2, 5):
            g.put(GHOST_XY[0] + dx, gy + 2, BAD if zapping else OUT)
        if not zapping and ga >= 1.0:
            g.put(GHOST_XY[0] + 9, gy + 1, ACCENT)  # sweat drop
    elif 0.78 <= t < 0.92:
        p = (t - 0.78) / 0.14
        cx, cy = GHOST_XY[0] + 4, GHOST_XY[1] + 3
        r = 3 if p < 0.5 else 5
        for i in range(-r, r + 1):
            for x, yy in ((cx + i, cy - r), (cx + i, cy + r), (cx - r, cy + i), (cx + r, cy + i)):
                g.put(x, yy, MUTED)
        if p > 0.35:
            for dx, dy in ((-2, -2), (2, -2), (0, 0), (-2, 2), (2, 2)):
                g.put(cx + dx, cy + dy, BAD)

    return g


def rounded_bg(size: int, radius: int) -> Image.Image:
    from PIL import ImageDraw

    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=(*GROUND, 255))
    return img


def svg_frame() -> str:
    """Vector canvas + the static frame (t=0.15) as crisp rects + SMIL groups for the story."""

    def hexs(c: tuple[int, ...]) -> str:
        return f"#{c[0]:02x}{c[1]:02x}{c[2]:02x}"

    def anim(values: str, key_times: str) -> str:
        return (
            f'<animate attributeName="opacity" values="{values}" keyTimes="{key_times}" '
            'dur="3s" repeatCount="indefinite"/>'
        )

    cells = [
        f'<rect x="{x}" y="{y}" width="1" height="1" fill="{hexs(c)}"/>'
        for (x, y), c in sorted(draw(0.15).px.items())
    ]
    out = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 48 48" '
        'shape-rendering="crispEdges" role="img" aria-label="aimon mascot: a pixel guard robot '
        'zaps an orphan-process ghost before its chest gauge maxes out">',
        "<!-- Generated by assets/gen_mascot.py -->",
        f'<rect width="48" height="48" rx="10" fill="{hexs(GROUND)}"/>',
        *cells,
        # bolt + stars + ghost X-eyes + poof (absent from the static frame)
        '<g fill="#ff2e97" opacity="0">'
        + "".join(f'<rect x="{x}" y="{yy}" width="1" height="1"/>' for x, yy in BOLT_CELLS)
        + anim("0;0;1;1;0;0", "0;0.68;0.70;0.78;0.80;1")
        + "</g>",
        '<g fill="#eaf0ff" opacity="0">'
        + "".join(f'<rect x="{x}" y="{yy}" width="1" height="1"/>' for x, yy in STAR_CELLS)
        + anim("0;0;1;0;0", "0;0.70;0.74;0.80;1")
        + "</g>",
        '<g fill="#ff3860" opacity="0"><rect x="37" y="8" width="1" height="1"/>'
        '<rect x="40" y="8" width="1" height="1"/>'
        + anim("0;0;1;1;0;0", "0;0.68;0.70;0.78;0.80;1")
        + "</g>",
        '<g fill="#8a98c4" opacity="0">'
        + "".join(
            f'<rect x="{GHOST_XY[0] + 4 + dx}" y="{GHOST_XY[1] + 3 + dy}" width="1" height="1"/>'
            for dx, dy in [(-3, 0), (3, 0), (0, -3), (0, 3), (-5, 0), (5, 0), (0, -5), (0, 5)]
        )
        + anim("0;0;1;0;0", "0;0.78;0.84;0.92;1")
        + "</g>",
        "</svg>",
    ]
    return "\n".join(out)


def main() -> None:
    out = Path(__file__).parent
    canvas = rounded_bg(512, 96)
    frames = []
    for i in range(FRAMES):
        sprite = draw(i / FRAMES).to_image().resize((GRID * SCALE, GRID * SCALE), Image.NEAREST)
        frame = canvas.copy()
        frame.alpha_composite(sprite, (16, 16))
        frames.append(frame.convert("P", palette=Image.ADAPTIVE, colors=64))
    gif = out / "mascot.gif"
    frames[0].save(
        gif,
        save_all=True,
        append_images=frames[1:],
        duration=int(1000 / FPS),
        loop=0,
        optimize=True,
    )
    frames[int(0.15 * FRAMES)].convert("RGB").save(out / "mascot-512.png", optimize=True)
    (out / "mascot.svg").write_text(svg_frame())
    print(f"{gif.name}: {gif.stat().st_size / 1024:.0f} KB, {FRAMES} frames @ {FPS} fps")


if __name__ == "__main__":
    main()

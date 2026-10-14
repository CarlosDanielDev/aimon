#!/usr/bin/env python3
"""aimon mascot, pixel art: the TUI itself is the guard. A tiny terminal window watches its
belly gauge climb while an orphan-process ghost feeds on it; the window zaps the ghost,
pressure drops, the ghost respawns. 32x32 grid upscaled 16x — GIF (app logo, ~20 KB), PNG,
and an animated SVG, all from this one grid.

Run: /tmp/aimon-venv/bin/python assets/gen_mascot.py
"""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image

# Theme (src/theme.rs neon)
GROUND = (0x0B, 0x0E, 0x1A)
PANEL = (0x14, 0x1A, 0x2E)
BRAND = (0xFF, 0x2E, 0x97)
ACCENT = (0x00, 0xE5, 0xFF)
OK = (0x39, 0xFF, 0x14)
WARN = (0xFF, 0xB0, 0x00)
BAD = (0xFF, 0x38, 0x60)
MUTED = (0x8A, 0x98, 0xC4)

GRID = 32
SCALE = 16
FPS = 25
FRAMES = 75  # 3s loop

# Ghost sprite top-left: right side, inside the frame.
GHOST = (21, 20)
BOLT_CELLS = [(26, 9), (25, 10), (26, 11), (24, 12), (25, 13), (24, 14), (23, 15), (23, 16), (23, 17)]


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

    def to_image(self) -> Image.Image:
        img = Image.new("RGBA", (GRID, GRID))
        img.putdata([self.px.get((x, y), (*GROUND, 255)) for y in range(GRID) for x in range(GRID)])
        return img


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def ease(t: float) -> float:
    return t * t * (3 - 2 * t)


def gauge_frac(t: float) -> float:
    if t < 0.68:
        return t / 0.68
    if t < 0.85:
        return 1.0 - ease((t - 0.68) / 0.17)
    return 0.0


def gauge_color(f: float) -> tuple[int, int, int]:
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

    # window frame, title bar, traffic lights
    g.rect(2, 6, 29, 25, MUTED)
    g.rect(3, 7, 28, 24, PANEL)
    g.rect(3, 7, 28, 8, BRAND)
    for i, c in enumerate((BAD, WARN, OK)):
        g.put(4 + i * 2, 8, c)

    # eyes: 2x3 blocks; blink flattens them to one row; glance shifts one col toward the ghost
    look = 1 if 0.60 <= t < 0.75 else 0
    blink = 0.44 <= t < 0.50
    for ex in (9, 19):
        if blink:
            g.rect(ex + look, 13, ex + 1 + look, 13, ACCENT)
        else:
            g.rect(ex + look, 11, ex + 1 + look, 13, ACCENT)

    # belly gauge: track x6..25, fill grows right, color follows pressure
    g.rect(6, 17, 25, 18, MUTED)
    g.rect(7, 17, 24, 18, GROUND)
    w = round(18 * frac)
    if w:
        g.rect(7, 17, 7 + w - 1, 18, gauge_color(frac))

    # bolt zigzag from the title bar down the right edge to the ghost
    if zapping:
        for x, y in BOLT_CELLS:
            g.put(x, y, BRAND)

    # ghost sprite, bobbing one row
    gy = GHOST[1] + (1 if math.sin(2 * math.pi * t) > 0 else 0)
    if ghost_alpha(t) > 0.5:
        for dy, row in enumerate([".###.", "#####", "#e#e#", "#####", "#.#.#"]):
            for dx, cell in enumerate(row):
                if cell == "#":
                    g.put(GHOST[0] + dx, gy + dy, WARN)
        for ex_off in (1, 3):  # eye pixels: dark, X-red while zapped
            g.put(GHOST[0] + ex_off, gy + 2, BAD if zapping else GROUND)
    elif 0.76 <= t < 0.92:
        # poof: hollow square, then a red X fading over it
        p = (t - 0.76) / 0.16
        cx, cy = GHOST[0] + 2, GHOST[1] + 2
        r = 2 if p < 0.5 else 3
        for i in range(-r, r + 1):
            for x, y in ((cx + i, cy - r), (cx + i, cy + r), (cx - r, cy + i), (cx + r, cy + i)):
                if 3 <= x <= 28 and 9 <= y <= 24:
                    g.put(x, y, MUTED)
        if p > 0.4:
            for dx, dy in ((-1, -1), (1, -1), (0, 0), (-1, 1), (1, 1)):
                g.put(cx + dx, cy + dy, BAD)

    return g


def svg_static_cells() -> list[str]:
    """Every non-background pixel of the static frame (t=0.15) as one SVG rect."""
    cells = []
    for (x, y), c in sorted(draw(0.15).px.items()):
        cells.append(f'<rect x="{x}" y="{y}" width="1" height="1" fill="#{c[0]:02x}{c[1]:02x}{c[2]:02x}"/>')
    return cells


def svg_frame() -> str:
    """Static pixel base + SMIL groups for bolt, X-eyes, poof and blink."""
    def anim(opacity_values: str, key_times: str) -> str:
        return (
            f'<animate attributeName="opacity" values="{opacity_values}" keyTimes="{key_times}" '
            'dur="3s" repeatCount="indefinite"/>'
        )

    out = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 32 32" '
        'shape-rendering="crispEdges" role="img" aria-label="aimon mascot pixel art: a tiny '
        'terminal window watches its memory gauge fill, then zaps the orphan-process ghost">',
        "<!-- Generated by assets/gen_mascot.py -->",
        f'<rect width="{GRID}" height="{GRID}" fill="#0b0e1a"/>',
        *svg_static_cells(),
        # blink overlays (hide the top two rows of each eye)
        '<rect x="9" y="11" width="2" height="2" fill="#141a2e" opacity="0">'
        + anim("0;0;1;0;0", "0;0.44;0.46;0.50;1") + "</rect>",
        '<rect x="19" y="11" width="2" height="2" fill="#141a2e" opacity="0">'
        + anim("0;0;1;0;0", "0;0.44;0.46;0.50;1") + "</rect>",
        # bolt
        f'<g fill="#{BRAND[0]:02x}{BRAND[1]:02x}{BRAND[2]:02x}" opacity="0">'
        + "".join(f'<rect x="{x}" y="{y}" width="1" height="1"/>' for x, y in BOLT_CELLS)
        + anim("0;0;1;1;0;0", "0;0.68;0.70;0.78;0.80;1") + "</g>",
        # ghost X-eyes
        '<g fill="#ff3860" opacity="0"><rect x="22" y="22" width="1" height="1"/>'
        '<rect x="24" y="22" width="1" height="1"/>'
        + anim("0;0;1;1;0;0", "0;0.68;0.70;0.78;0.80;1") + "</g>",
        # poof ring + X
        '<g fill="#8a98c4" opacity="0">'
        + "".join(
            f'<rect x="{GHOST[0] + 2 + dx}" y="{GHOST[1] + 2 + dy}" width="1" height="1"/>'
            for dx, dy in [(-2, 0), (2, 0), (0, -2), (0, 2), (-3, 0), (3, 0), (0, -3), (0, 3)]
        )
        + anim("0;0;1;0;0", "0;0.76;0.82;0.90;1") + "</g>",
        '<g fill="#ff3860" opacity="0">'
        + "".join(
            f'<rect x="{GHOST[0] + 2 + dx}" y="{GHOST[1] + 2 + dy}" width="1" height="1"/>'
            for dx, dy in [(-1, -1), (1, -1), (0, 0), (-1, 1), (1, 1)]
        )
        + anim("0;0;1;0;0", "0;0.80;0.84;0.92;1") + "</g>",
        "</svg>",
    ]
    return "\n".join(out)


def main() -> None:
    out = Path(__file__).parent
    big = [draw(i / FRAMES).to_image().resize((GRID * SCALE, GRID * SCALE), Image.NEAREST) for i in range(FRAMES)]
    pal = [f.convert("P", palette=Image.ADAPTIVE, colors=32) for f in big]
    gif = out / "mascot.gif"
    pal[0].save(
        gif,
        save_all=True,
        append_images=pal[1:],
        duration=int(1000 / FPS),
        loop=0,
        optimize=True,
    )
    big[int(0.15 * FRAMES)].convert("RGB").save(out / "mascot-512.png", optimize=True)
    (out / "mascot.svg").write_text(svg_frame())
    print(f"{gif.name}: {gif.stat().st_size / 1024:.0f} KB, {FRAMES} frames @ {FPS} fps")


if __name__ == "__main__":
    main()

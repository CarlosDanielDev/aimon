#!/usr/bin/env python3
"""aimon art, generated: pixel sprites on a rounded vector canvas.

- icon: a terminal window (prompt, cursor) sweeping its processes into a trash can.
- mascot: the guardian creature sweeps orphan debris with a broom; its "!" bubble pops
  when memory pressure climbs. 3s loop.

Run: /tmp/aimon-venv/bin/python assets/gen_mascot.py
"""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image

GROUND = (0x0B, 0x0E, 0x1A)
PANEL = (0x14, 0x1A, 0x2E)
BODY = (0x1F, 0x8F, 0xA8)
BODY_L = (0x45, 0xC8, 0xDC)
BODY_D = (0x14, 0x5E, 0x74)
OUT = (0x08, 0x12, 0x1E)
BELLY = (0xBF, 0xEE, 0xF7)
CYAN = (0x4D, 0xD7, 0xE8)
CYAN_L = (0x9F, 0xEB, 0xF5)
BRAND = (0xFF, 0x2E, 0x97)
PINK_L = (0xFF, 0x6C, 0xB0)
ACCENT = (0x00, 0xE5, 0xFF)
OK = (0x39, 0xFF, 0x14)
WARN = (0xFF, 0xB0, 0x00)
BAD = (0xFF, 0x38, 0x60)
MUTED = (0x8A, 0x98, 0xC4)
WHITE = (0xF2, 0xF8, 0xFF)
INK = (0x10, 0x1A, 0x2C)

GRID = 48
SCALE = 10
FPS = 25
FRAMES = 75  # 3s loop


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

    def ellipse(self, cx: float, cy: float, rx: int, ry: int, c: tuple[int, ...]) -> None:
        for y in range(int(cy - ry), int(cy + ry) + 1):
            for x in range(int(cx - rx), int(cx + rx) + 1):
                if ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1.0:
                    self.put(x, y, c)

    def ball(self, cx: float, cy: float, r: int, fill: tuple[int, ...],
             shade: tuple[int, ...] | None = None, light: tuple[int, ...] | None = None) -> None:
        self.ellipse(cx, cy, r + 1, r + 1, OUT)
        self.ellipse(cx, cy, r, r, fill)
        if shade is not None:
            for y in range(int(cy - r), int(cy + r) + 1):
                for x in range(int(cx - r), int(cx + r) + 1):
                    if ((x - cx + 1.5) / (r - 1)) ** 2 + ((y - cy + 1.5) / (r - 1)) ** 2 > 1.0:
                        if self.px.get((x, y)) == fill:
                            self.put(x, y, shade)
        if light is not None:
            for y in range(int(cy - r), int(cy + r) + 1):
                for x in range(int(cx - r), int(cx + r) + 1):
                    if ((x - cx - 2.0) / (r - 1)) ** 2 + ((y - cy - 2.0) / (r - 1)) ** 2 > 1.0:
                        if self.px.get((x, y)) == fill and x <= cx and y <= cy:
                            self.put(x, y, light)

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


# ---------------------------------------------------------------- icon

def draw_icon() -> Grid:
    """Terminal window + prompt + cursor; a trash can eating process debris."""
    g = Grid()
    # window: cyan frame, filled title band with pink traffic dots
    g.rect(5, 5, 34, 41, CYAN)
    g.rect(7, 7, 32, 39, GROUND)
    g.rect(7, 7, 32, 12, CYAN)
    for i in range(3):
        g.rect(9 + i * 4, 9, 11 + i * 4, 11, BRAND)
    # prompt: ">" chevron + cursor
    for i, yy in enumerate(range(20, 29)):
        g.rect(12 + abs(4 - i), yy, 13 + abs(4 - i), yy, CYAN_L)
    g.rect(17, 29, 24, 30, CYAN_L)
    # trash can at the corner, eating debris
    g.rect(32, 27, 45, 29, CYAN_L)  # lid
    g.rect(36, 25, 38, 26, CYAN_L)  # handle
    g.rect(33, 31, 44, 43, CYAN_L)  # body
    g.rect(35, 33, 36, 41, GROUND)
    g.rect(40, 33, 41, 41, GROUND)  # slots
    # debris flying in
    for x, y, c in [(28, 20, BRAND), (24, 24, BRAND), (28, 28, PINK_L),
                    (26, 33, BRAND), (43, 15, BRAND), (46, 20, ACCENT)]:
        g.rect(x, y, x + 1, y + 1, c)
    return g


# ---------------------------------------------------------------- mascot

def gauge_frac(t: float) -> float:
    if t < 0.6:
        return t / 0.6
    if t < 0.85:
        return 1.0 - ease((t - 0.6) / 0.25)
    return 0.0


def gauge_color(f: float) -> tuple[int, ...]:
    if f >= 0.85:
        return BAD
    if f >= 0.7:
        return WARN
    return OK


BUBBLE = (34, 3)  # top-left of the "!" speech bubble
DEBRIS = [(5, 38), (9, 42), (13, 44), (16, 40), (3, 44), (8, 36), (12, 39), (18, 44)]


def draw(t: float) -> Grid:
    g = Grid()
    frac = gauge_frac(t)
    alert = 0.6 <= t < 0.9  # the "!" bubble pops while pressure is high
    stroke = int(t / 3 * 12) % 2  # broom sweeps: two positions
    bob = -1 if math.sin(2 * math.pi * t) > 0 else 0
    y = bob

    g.rect(10, 46, 38, 46, (0x14, 0x22, 0x2E, 255))  # ground shadow

    # ears
    for x0, x1, yy in [(14, 14, 4), (13, 15, 5), (12, 16, 6), (12, 17, 7)]:
        g.rect(x0, yy + y, x1, yy + y, OUT)
        g.rect(x0 + 1, yy + y, x1 - 1, yy + y, BODY)
    g.rect(14, 6 + y, 14, 8 + y, PINK_L)
    for x0, x1, yy in [(31, 31, 4), (30, 32, 5), (29, 33, 6), (29, 34, 7)]:
        g.rect(x0, yy + y, x1, yy + y, OUT)
        g.rect(x0 + 1, yy + y, x1 - 1, yy + y, BODY)
    g.rect(31, 6 + y, 32, 8 + y, PINK_L)

    # head, tilted toward the sweep (down-left)
    g.ball(25, 17 + y, 11, BODY, shade=BODY_D, light=BODY_L)
    g.rect(23, 28 + y, 28, 30 + y, BODY_D)  # neck

    # face: eyes look down-left at the debris
    blink = 0.3 <= t < 0.36
    for ex in (17, 28):
        if blink:
            g.rect(ex, 16 + y, ex + 3, 16 + y, INK)
            continue
        g.rect(ex, 12 + y, ex + 3, 16 + y, INK)
        g.rect(ex, 12 + y, ex + 1, 13 + y, WHITE)
        g.put(ex + 3, 16 + y, CYAN_L)
    # mouth: "o" while alerting, smile otherwise
    if alert:
        g.rect(23, 22 + y, 26, 23 + y, INK)
    else:
        g.rect(22, 22 + y, 25, 22 + y, INK)
        g.put(21, 21 + y, INK)
        g.put(26, 21 + y, INK)
    g.rect(12, 19 + y, 13, 20 + y, ACCENT)
    g.rect(35, 19 + y, 36, 20 + y, ACCENT)

    # body + belly meter
    g.ellipse(25, 38 + y, 10, 7, OUT)
    g.ellipse(25, 38 + y, 9, 6, BODY)
    g.ellipse(25, 39 + y, 7, 4, BELLY)
    g.rect(20, 38 + y, 30, 39 + y, BODY_D)
    w = round(10 * frac)
    if w:
        g.rect(20, 38 + y, 20 + w - 1, 39 + y, gauge_color(frac))

    # feet
    g.ball(19, 45 + y, 2, BODY, shade=BODY_D)
    g.ball(31, 45 + y, 2, BODY, shade=BODY_D)

    # left arm; right arm grips the broom
    g.rect(13, 34 + y, 14, 37 + y, BODY_L)
    g.rect(33, 30 + y, 34, 34 + y, BODY_L)
    g.rect(33, 29 + y, 34, 30 + y, BRAND)  # little glove on the handle

    # broom: long cyan handle, brush head sweeping the floor well left of the feet
    if stroke:
        for i in range(15):
            g.put(33 - i, 29 + i + y, CYAN)
            g.put(34 - i, 29 + i + y, CYAN_L)
        g.rect(13, 43 + y, 21, 44 + y, CYAN)
        g.rect(12, 45 + y, 22, 46 + y, PINK_L)  # bristles
        swept = DEBRIS
    else:
        for i in range(13):
            g.put(32 - i, 29 + i + y, CYAN)
            g.put(33 - i, 29 + i + y, CYAN_L)
        g.rect(16, 42 + y, 24, 43 + y, CYAN)
        g.rect(15, 44 + y, 25, 46 + y, PINK_L)
        swept = [(x - 4, yy) for x, yy in DEBRIS]
    # debris: orphan pixels being swept away, fading at the edge
    for i, (x, yy) in enumerate(swept):
        c = BRAND if i % 2 == 0 else CYAN_L
        if int(t * FPS + i) % 7 == 0:
            continue  # flicker
        g.put(x, yy, c)
        if i % 3 == 0:
            g.put(x + 1, yy, c)

    # "!" speech bubble pops while alerting
    if alert:
        pop = (t - 0.6) < 0.04
        bx, by = BUBBLE[0], BUBBLE[1] + (1 if pop else 0)
        g.rect(bx, by, bx + 11, by + 9, BRAND)
        g.rect(bx + 1, by + 1, bx + 10, by + 8, PANEL)
        g.rect(bx + 2, by + 9, bx + 3, by + 10, BRAND)  # tail
        g.rect(bx + 5, by + 2, bx + 6, by + 5, BRAND)   # !
        g.rect(bx + 5, by + 7, bx + 6, by + 7, BRAND)

    return g


def rounded_bg(size: int, radius: int) -> Image.Image:
    from PIL import ImageDraw

    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=(*GROUND, 255))
    return img


def svg_frame() -> str:
    """Static poster frame (t=0.2) as crisp rects — the GIF carries the motion."""

    def hexs(c: tuple[int, ...]) -> str:
        return f"#{c[0]:02x}{c[1]:02x}{c[2]:02x}"

    cells = [
        f'<rect x="{x}" y="{yy}" width="1" height="1" fill="{hexs(c)}"/>'
        for (x, yy), c in sorted(draw(0.2).px.items())
    ]
    return "\n".join(
        [
            '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 48 48" '
            'shape-rendering="crispEdges" role="img" aria-label="aimon mascot: a pixel guardian '
            'creature sweeps orphan-process debris while its alert bubble pops">',
            f'<rect width="48" height="48" rx="10" fill="{hexs(GROUND)}"/>',
            *cells,
            "</svg>",
        ]
    )


def icon_svg() -> str:
    def hexs(c: tuple[int, ...]) -> str:
        return f"#{c[0]:02x}{c[1]:02x}{c[2]:02x}"

    cells = [
        f'<rect x="{x}" y="{yy}" width="1" height="1" fill="{hexs(c)}"/>'
        for (x, yy), c in sorted(draw_icon().px.items())
    ]
    return "\n".join(
        [
            '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 48 48" '
            'shape-rendering="crispEdges" role="img" aria-label="aimon icon: a terminal window '
            'sweeps its processes into a trash can">',
            f'<rect width="48" height="48" rx="10" fill="{hexs(GROUND)}"/>',
            *cells,
            "</svg>",
        ]
    )


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
    frames[int(0.2 * FRAMES)].convert("RGB").save(out / "mascot-512.png", optimize=True)
    (out / "mascot.svg").write_text(svg_frame())

    icon_canvas = rounded_bg(512, 96)
    icon_sprite = draw_icon().to_image().resize((GRID * SCALE, GRID * SCALE), Image.NEAREST)
    icon_canvas.alpha_composite(icon_sprite, (16, 16))
    icon_canvas.convert("RGB").save(out / "icon-512.png", optimize=True)
    icon_canvas.resize((32, 32), Image.LANCZOS).save(out / "icon-32.png", optimize=True)
    (out / "icon.svg").write_text(icon_svg())
    print(f"{gif.name}: {gif.stat().st_size / 1024:.0f} KB; icon-512/icon-32/icon.svg written")


if __name__ == "__main__":
    main()

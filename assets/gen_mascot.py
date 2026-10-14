#!/usr/bin/env python3
"""aimon mascot: a chibi guardian creature — monster-design language, not a monster you know.

Round teal body, big sparkly eyes, one notched ear, a belly that literally IS the memory
gauge, and a pink zigzag tail that fires the zap. It watches an invader ghost feed on its
belly meter, gets cross, zaps the ghost with its tail, and goes happy when pressure drops.
Pixel sprite (48 grid, outlined + shaded) on a rounded vector canvas. 3s loop.

Run: /tmp/aimon-venv/bin/python assets/gen_mascot.py
"""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image

# Theme (src/theme.rs neon) + creature shades for value contrast on the dark ground.
GROUND = (0x0B, 0x0E, 0x1A)
BODY = (0x1F, 0x8F, 0xA8)
BODY_L = (0x45, 0xC8, 0xDC)
BODY_D = (0x14, 0x5E, 0x74)
OUT = (0x08, 0x12, 0x1E)
BELLY = (0xBF, 0xEE, 0xF7)
BELLY_D = (0x8F, 0xC9, 0xDD)
BRAND = (0xFF, 0x2E, 0x97)
PINK_L = (0xFF, 0x6C, 0xB0)
ACCENT = (0x00, 0xE5, 0xFF)
CYAN_L = (0x7D, 0xF3, 0xFF)
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

GHOST_XY = (35, 5)
GHOST = [
    "..yyy.yy.",
    ".yyyyyyy.",
    "yyeyyeyy.",
    "yyyyyyyyy",
    "yyyyyyyyy",
    ".yy.yy.y.",
    "y.y..y.y.",
]
# tail tip -> ghost arc, lit while zapping
ZAP_CELLS = [(43, 15), (43, 14), (42, 13), (41, 12)]
STAR_CELLS = [(43, 3), (45, 8), (38, 4), (45, 11)]


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
        """Outlined ball with an optional light crescent (top-left) and shade (bottom-right)."""
        self.ellipse(cx, cy, r + 1, r + 1, OUT)
        self.ellipse(cx, cy, r, r, fill)
        if shade is not None:  # inner offset ring, bottom-right
            for y in range(int(cy - r), int(cy + r) + 1):
                for x in range(int(cx - r), int(cx + r) + 1):
                    if ((x - cx + 1.5) / (r - 1)) ** 2 + ((y - cy + 1.5) / (r - 1)) ** 2 > 1.0:
                        if self.px.get((x, y)) == fill:
                            self.put(x, y, shade)
        if light is not None:  # small crescent, top-left
            for y in range(int(cy - r), int(cy + r) + 1):
                for x in range(int(cx - r), int(cx + r) + 1):
                    if ((x - cx - 1.5) / (r - 1)) ** 2 + ((y - cy - 1.5) / (r - 1)) ** 2 > 1.0:
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


TAIL = [  # zigzag bolt tail hugging the right hip, bottom to top; tip charges and fires
    (30, 40), (31, 40), (31, 39), (32, 39),
    (32, 38), (33, 38), (33, 37), (34, 37),
    (34, 36), (35, 36), (35, 35), (36, 35),
    (36, 34), (37, 34), (37, 33), (38, 33),
    (38, 32), (39, 32), (39, 31), (40, 31),
    (40, 30), (41, 30), (41, 29), (42, 29),
    (42, 28), (43, 28), (43, 27), (44, 27),
    (44, 26), (45, 26), (45, 25), (46, 25),
    (46, 24), (46, 23), (45, 22), (45, 21),
    (44, 20), (44, 19), (43, 18), (43, 17),
]


def draw(t: float) -> Grid:
    g = Grid()
    frac = gauge_frac(t)
    zapping = 0.68 <= t < 0.80
    happy = 0.80 <= t < 0.95
    bob = -1 if math.sin(2 * math.pi * t) > 0 else 0
    y = bob

    g.rect(12, 45, 32, 45, (0x14, 0x22, 0x2E, 255))  # ground shadow

    # tail: pink zigzag rising to the right, tip charges with pressure
    charge = BAD if frac >= 0.85 else (WARN if frac >= 0.7 else PINK_L)
    tip = TAIL[-1]
    if frac >= 0.99 and not zapping:
        g.put(tip[0], tip[1] + y, WHITE)  # fully charged flash

    # feet
    g.ball(15, 43 + y, 2, BODY, shade=BODY_D)
    g.ball(28, 43 + y, 2, BODY, shade=BODY_D)

    # ears behind the head: left tall + pointy, right notched (it hunts orphans for a living)
    for x0, x1, yy in [(12, 14, 4), (12, 15, 5), (11, 16, 6), (11, 16, 7), (11, 16, 8)]:
        g.rect(x0, yy + y, x1, yy + y, OUT)
        g.rect(x0 + 1, yy + y, x1 - 1, yy + y, BODY)
    g.rect(13, 4 + y, 13, 7 + y, PINK_L)
    for x0, x1, yy in [(27, 29, 3), (26, 30, 4), (26, 31, 5), (26, 31, 6), (26, 31, 7), (26, 31, 8)]:
        g.rect(x0, yy + y, x1, yy + y, OUT)
        g.rect(x0 + 1, yy + y, x1 - 1, yy + y, BODY)
    g.rect(29, 4 + y, 29, 6 + y, PINK_L)
    g.rect(29, 6 + y, 31, 7 + y, GROUND + (255,))  # notch bite out of the right ear

    # body
    g.ellipse(21, 37 + y, 10, 8, OUT)
    g.ellipse(21, 37 + y, 9, 7, BODY)
    g.ellipse(20, 37 + y, 7, 5, BELLY)
    # arms: tiny nubs
    g.ball(10, 35 + y, 2, BODY, shade=BODY_D)
    g.ball(32, 35 + y, 2, BODY, shade=BODY_D)

    # tail root sits on the body's right hip, so the bolt grows out of it
    for i, (x, yy) in enumerate(TAIL):
        g.put(x, yy + y, BRAND if i < len(TAIL) - 6 else charge)

    # belly meter: the gauge IS its tummy
    g.rect(15, 36 + y, 27, 37 + y, INK)
    w = round(12 * frac)
    if w:
        c = gauge_color(frac)
        g.rect(15, 36 + y, 15 + w - 1, 37 + y, c)
        g.put(15, 36 + y, (min(255, c[0] + 60), min(255, c[1] + 60), min(255, c[2] + 60), 255))

    # head: big round, light crescent
    g.ball(21, 19 + y, 12, BODY, shade=BODY_D, light=BODY_L)

    # face
    blink = 0.44 <= t < 0.50
    squint = frac >= 0.85 or zapping
    for ex in (13, 24):
        if blink and not happy:
            g.rect(ex, 19 + y, ex + 4, 19 + y, INK)
            continue
        # white + cyan iris + pupil + sparkle, pokemon-scale
        g.ellipse(ex + 2, 19 + y, 2, 3, WHITE)
        g.ellipse(ex + 2, 20 + y, 1, 2, ACCENT if not squint else CYAN_L)
        g.put(ex + 2 + (1 if squint else 0), 20 + y, INK if squint else INK)
        g.put(ex + 1, 18 + y, WHITE)  # sparkle
    if squint:
        # angry brows
        g.rect(12, 15 + y, 14, 15 + y, INK)
        g.rect(13, 14 + y, 14, 14 + y, INK)
        g.rect(25, 14 + y, 26, 14 + y, INK)
        g.rect(25, 15 + y, 27, 15 + y, INK)
    # mouth: smirk when working, open grin when happy, flat frown under pressure
    if happy:
        g.rect(19, 25 + y, 24, 26 + y, INK)
        g.rect(20, 26 + y, 23, 26 + y, BAD)  # tongue
    elif squint:
        g.rect(19, 26 + y, 23, 26 + y, INK)
        g.put(19, 25 + y, INK)
        g.put(23, 25 + y, INK)
    else:
        g.rect(19, 25 + y, 22, 25 + y, INK)
        g.put(22, 24 + y, INK)  # smirk tip
        g.put(20, 26 + y, WHITE)  # little fang
    # cheek gems — teal-cyan, not red cheeks
    g.rect(10, 24 + y, 11, 25 + y, ACCENT)
    g.rect(29, 24 + y, 30, 25 + y, ACCENT)

    # the zap: tail tip -> ghost, impact stars
    if zapping:
        for x, yy in ZAP_CELLS:
            g.put(x, yy + y, BRAND)
        g.put(*ZAP_CELLS[0], WHITE)
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


def draw_icon() -> Grid:
    """App icon: just the creature's head, way simpler than the mascot. Reads at 16px."""
    g = Grid()
    # ears
    for x0, x1, yy in [(10, 14, 4), (10, 15, 5), (9, 16, 6), (9, 17, 7), (9, 18, 8), (9, 19, 9)]:
        g.rect(x0, yy, x1, yy, OUT)
        g.rect(x0 + 1, yy, x1 - 1, yy, BODY)
    g.rect(11, 5, 11, 9, PINK_L)
    for x0, x1, yy in [(33, 37, 4), (32, 38, 5), (31, 39, 6), (31, 40, 7), (31, 41, 8), (31, 42, 9)]:
        g.rect(x0, yy, x1, yy, OUT)
        g.rect(x0 + 1, yy, x1 - 1, yy, BODY)
    g.rect(38, 5, 38, 9, PINK_L)
    # head ball
    g.ball(24, 26, 17, BODY, shade=BODY_D, light=BODY_L)
    # eyes: big, sparkly
    for ex in (14, 27):
        g.ellipse(ex + 2, 25, 3, 5, WHITE)
        g.ellipse(ex + 2, 26, 2, 3, ACCENT)
        g.put(ex + 2, 27, INK)
        g.put(ex + 1, 23, WHITE)
    # smile + gems
    g.rect(21, 34, 27, 34, INK)
    g.put(21, 33, INK)
    g.put(27, 33, INK)
    g.put(23, 35, WHITE)
    g.rect(6, 32, 7, 33, ACCENT)
    g.rect(40, 32, 41, 33, ACCENT)
    return g


def rounded_bg(size: int, radius: int) -> Image.Image:
    from PIL import ImageDraw

    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=(*GROUND, 255))
    return img


def svg_frame() -> str:
    """Vector canvas + static frame (t=0.15) as crisp rects + SMIL groups for zap/poof."""

    def hexs(c: tuple[int, ...]) -> str:
        return f"#{c[0]:02x}{c[1]:02x}{c[2]:02x}"

    def anim(values: str, key_times: str) -> str:
        return (
            f'<animate attributeName="opacity" values="{values}" keyTimes="{key_times}" '
            'dur="3s" repeatCount="indefinite"/>'
        )

    cells = [
        f'<rect x="{x}" y="{yy}" width="1" height="1" fill="{hexs(c)}"/>'
        for (x, yy), c in sorted(draw(0.15).px.items())
    ]
    out = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 48 48" '
        'shape-rendering="crispEdges" role="img" aria-label="aimon mascot: a pixel guardian '
        'creature zaps an orphan-process ghost with its bolt tail before its belly gauge maxes out">',
        "<!-- Generated by assets/gen_mascot.py -->",
        f'<rect width="48" height="48" rx="10" fill="{hexs(GROUND)}"/>',
        *cells,
        '<g fill="#ff2e97" opacity="0">'
        + "".join(f'<rect x="{x}" y="{yy}" width="1" height="1"/>' for x, yy in ZAP_CELLS)
        + anim("0;0;1;1;0;0", "0;0.68;0.70;0.78;0.80;1")
        + "</g>",
        '<g fill="#f2f8ff" opacity="0">'
        + "".join(f'<rect x="{x}" y="{yy}" width="1" height="1"/>' for x, yy in STAR_CELLS)
        + anim("0;0;1;0;0", "0;0.70;0.74;0.80;1")
        + "</g>",
        '<g fill="#ff3860" opacity="0"><rect x="37" y="7" width="1" height="1"/>'
        '<rect x="40" y="7" width="1" height="1"/>'
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

    # app icon: creature head, bigger grid share, on the same rounded canvas
    canvas = rounded_bg(512, 96)
    sprite = draw_icon().to_image().resize((GRID * SCALE, GRID * SCALE), Image.NEAREST)
    canvas.alpha_composite(sprite, (16, 16))
    canvas.convert("RGB").save(out / "icon-512.png", optimize=True)
    canvas.resize((32, 32), Image.LANCZOS).save(out / "icon-32.png", optimize=True)
    print("icon-512.png, icon-32.png")


if __name__ == "__main__":
    main()

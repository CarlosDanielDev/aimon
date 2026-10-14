#!/usr/bin/env python3
"""aimon mascot: a minimal capsule guard whose belly gauge rises with memory pressure until
it zaps the orphan-process ghost. Claude-Code-style simplicity: one shape, dot eyes, one
accent detail. Vector primitives only, supersampled 3x, exported as an animated GIF
(app-directory logo, <=2MB) plus a static PNG.

Run: /tmp/aimon-venv/bin/python assets/gen_mascot.py
"""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

# Theme (src/theme.rs neon)
GROUND = (0x0B, 0x0E, 0x1A)
PANEL = (0x14, 0x1A, 0x2E)
BRAND = (0xFF, 0x2E, 0x97)
ACCENT = (0x00, 0xE5, 0xFF)
OK = (0x39, 0xFF, 0x14)
WARN = (0xFF, 0xB0, 0x00)
BAD = (0xFF, 0x38, 0x60)
MUTED = (0x8A, 0x98, 0xC4)

SIZE = 512
S = 3  # supersample
FPS = 25
FRAMES = 75  # 3s loop

GHOST = (410, 138)  # center of the orphan ghost


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def ease(t: float) -> float:
    return t * t * (3 - 2 * t)


def gauge_frac(t: float) -> float:
    """Pressure rises while the orphan runs, collapses when the zap lands."""
    if t < 0.68:
        return lerp(0.12, 1.0, ease(t / 0.68))
    if t < 0.85:
        return lerp(1.0, 0.12, ease((t - 0.68) / 0.17))
    return 0.12


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


def draw_frame(t: float) -> Image.Image:
    img = Image.new("RGBA", (SIZE * S, SIZE * S), GROUND + (255,))
    d = ImageDraw.Draw(img, "RGBA")

    def rr(box, r, **kw):
        d.rounded_rectangle([c * S for c in box], radius=r * S, **kw)

    frac = gauge_frac(t)
    zapping = 0.68 <= t < 0.80

    gx, gy = GHOST[0], GHOST[1] + 5 * math.sin(2 * math.pi * t)  # bob
    ga = ghost_alpha(t)

    # --- bolt: antenna tip -> ghost ---
    if zapping:
        p = (t - 0.68) / 0.12
        pts = [(256, 108), (310, 122), (362, 114), (gx, gy + 12)]
        d.line(
            [(x * S, y * S) for x, y in pts],
            fill=BRAND + (int(255 * min(1, 1.3 - p)),),
            width=7 * S,
            joint="curve",
        )

    # --- ghost ---
    if ga > 0:
        g = int(235 * ga)
        body = WARN + (g,)
        r = 26
        d.pieslice([(gx - r) * S, (gy - r) * S, (gx + r) * S, (gy + r) * S], 180, 360, fill=body)
        d.rectangle([(gx - r) * S, gy * S, (gx + r) * S, (gy + 16) * S], fill=body)
        zig = [(gx - r + i * r / 2, gy + (16 if i % 2 == 0 else 8)) for i in range(5)]
        zig += [(gx + r, gy + 16), (gx - r, gy + 16)]
        d.polygon([(x * S, y * S) for x, y in zig], fill=body)
        if zapping:  # X eyes
            for ex in (gx - 10, gx + 10):
                d.line(
                    [(ex - 4) * S, (gy - 9) * S, (ex + 4) * S, (gy - 1) * S],
                    fill=BAD + (g,),
                    width=3 * S,
                )
                d.line(
                    [(ex + 4) * S, (gy - 9) * S, (ex - 4) * S, (gy - 1) * S],
                    fill=BAD + (g,),
                    width=3 * S,
                )
        else:
            for ex in (gx - 10, gx + 10):
                d.ellipse(
                    [(ex - 4) * S, (gy - 10) * S, (ex + 4) * S, (gy - 2) * S],
                    fill=GROUND + (g,),
                )

    # --- poof ring ---
    if 0.76 <= t < 0.92:
        p = (t - 0.76) / 0.16
        r = lerp(24, 62, p)
        d.ellipse(
            [(gx - r) * S, (gy - r) * S, (gx + r) * S, (gy + r) * S],
            outline=MUTED + (int(220 * (1 - p)),),
            width=max(1, int(4 * (1 - p))) * S,
        )

    # --- antenna ---
    d.line([256 * S, 152 * S, 256 * S, 108 * S], fill=BRAND, width=5 * S)
    d.ellipse(
        [(256 - 10) * S, 98 * S, (256 + 10) * S, 118 * S],
        fill=BRAND if zapping else ACCENT,
    )

    # --- body: one capsule ---
    rr((166, 150, 346, 392), 70, fill=PANEL, outline=BRAND, width=6)

    # --- eyes: two dots, a blink, a glance ---
    look = 4 * math.sin(2 * math.pi * t * 0.5)
    if 0.60 <= t < 0.75:
        look = 6  # watch the ghost
    blink = 1.0
    if 0.44 <= t < 0.50:
        blink = abs(math.cos(math.pi * (t - 0.44) / 0.06))
    for ex in (222, 290):
        ry = 12 * blink
        d.ellipse(
            [(ex - 11) * S, (208 - ry) * S, (ex + 11) * S, (208 + ry) * S],
            fill=ACCENT,
        )
        d.ellipse(
            [(ex - 4 + look) * S, (208 - 5 * blink) * S, (ex + 4 + look) * S, (208 + 5 * blink) * S],
            fill=GROUND,
        )

    # --- belly gauge ---
    rr((196, 330, 316, 348), 9, fill=GROUND, outline=MUTED + (110,), width=2)
    if frac > 0.04:
        rr(
            (200, 334, lerp(200, 312, frac), 344),
            5,
            fill=gauge_color(frac),
        )

    # --- wordmark ---
    d.text((256 * S, 462 * S), "aimon", font=font(40), anchor="mm", fill=BRAND)

    return img.resize((SIZE, SIZE), Image.LANCZOS)


_font_cache: dict[int, ImageFont.FreeTypeFont] = {}


def font(px: int) -> ImageFont.FreeTypeFont:
    if px not in _font_cache:
        _font_cache[px] = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", px * S)
    return _font_cache[px]


def main() -> None:
    out = Path(__file__).parent
    frames = [draw_frame(i / FRAMES).convert("P", palette=Image.ADAPTIVE, colors=96) for i in range(FRAMES)]
    gif = out / "mascot.gif"
    frames[0].save(
        gif,
        save_all=True,
        append_images=frames[1:],
        duration=int(1000 / FPS),
        loop=0,
        optimize=True,
        disposal=2,
    )
    draw_frame(0.15).convert("RGB").save(out / "mascot-512.png", optimize=True)
    kb = gif.stat().st_size / 1024
    print(f"{gif.name}: {kb:.0f} KB, {FRAMES} frames @ {FPS} fps")


if __name__ == "__main__":
    main()

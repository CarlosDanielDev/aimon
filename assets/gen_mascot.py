#!/usr/bin/env python3
"""aimon mascot: a neon guard robot that watches its RAM gauge rise and zaps the orphan
process ghost before pressure goes critical. Vector primitives only, supersampled 4x,
exported as an animated GIF (app-directory logo, <=2MB) plus a static PNG.

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
T_LOOP = 3.0  # seconds
FPS = 25
FRAMES = int(T_LOOP * FPS)

GHOST = (420, 128)  # center of the orphan ghost


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def ease(t: float) -> float:
    return t * t * (3 - 2 * t)


def gauge_frac(t: float) -> float:
    """Pressure rises while the orphan runs, collapses when the zap lands."""
    if t < 0.68:
        return lerp(0.35, 0.92, ease(t / 0.68))
    if t < 0.85:
        return lerp(0.92, 0.35, ease((t - 0.68) / 0.17))
    return 0.35


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


def rr(d: ImageDraw.ImageDraw, box: tuple[float, float, float, float], r: float, **kw) -> None:
    d.rounded_rectangle([c * S for c in box], radius=r * S, **kw)


def draw_frame(t: float) -> Image.Image:
    img = Image.new("RGBA", (SIZE * S, SIZE * S), GROUND + (255,))
    d = ImageDraw.Draw(img, "RGBA")
    rr(d, (0, 0, SIZE, SIZE), 96, fill=GROUND)

    frac = gauge_frac(t)
    zapping = 0.68 <= t < 0.80

    # --- ghost (behind the arm) ---
    ga = ghost_alpha(t)
    gx, gy = GHOST[0], GHOST[1] + 5 * math.sin(2 * math.pi * t)  # bob
    if ga > 0:
        g = int(235 * ga)
        body = WARN + (g,)
        r = 30
        d.pieslice([(gx - r) * S, (gy - r) * S, (gx + r) * S, (gy + r) * S], 180, 360, fill=body)
        d.rectangle([(gx - r) * S, gy * S, (gx + r) * S, (gy + 20) * S], fill=body)
        zig = []
        for i in range(5):
            zig += [(gx - r + i * r / 2, gy + (20 if i % 2 == 0 else 11))]
        zig += [(gx + r, gy + 20), (gx - r, gy + 20)]
        d.polygon([(x * S, y * S) for x, y in zig], fill=body)
        if zapping:  # X eyes
            for ex in (gx - 12, gx + 12):
                for (x1, y1, x2, y2) in (
                    (ex - 5, gy - 11, ex + 5, gy - 1),
                    (ex + 5, gy - 11, ex - 5, gy - 1),
                ):
                    d.line([x1 * S, y1 * S, x2 * S, y2 * S], fill=BAD + (g,), width=3 * S)
        else:
            for ex in (gx - 12, gx + 12):
                d.ellipse(
                    [(ex - 5) * S, (gy - 12) * S, (ex + 5) * S, (gy - 2) * S],
                    fill=GROUND + (g,),
                )
        d.text(
            ((gx) * S, (gy + 44) * S),
            "orphan",
            font=font(20),
            anchor="mm",
            fill=MUTED + (int(200 * ga),),
        )

    # --- antenna ---
    tip_y = 84
    d.line([256 * S, 122 * S, 256 * S, tip_y * S], fill=BRAND, width=5 * S)
    glow = 22 if zapping else 0
    if glow:
        d.ellipse(
            [(256 - glow) * S, (tip_y - glow) * S, (256 + glow) * S, (tip_y + glow) * S],
            fill=BRAND + (90,),
        )
    d.ellipse(
        [(256 - 10) * S, (tip_y - 10) * S, (256 + 10) * S, (tip_y + 10) * S],
        fill=ACCENT if not zapping else BRAND,
    )

    # --- bolt: antenna tip -> ghost, arcing over the head ---
    if zapping:
        p = (t - 0.68) / 0.12
        pts = [(256, tip_y)]
        for i, (mx, my) in enumerate([(304, 96), (352, 88), (392, 98)]):
            jitter = 9 * math.sin(i * 9 + p * 12)
            pts.append((mx + jitter, my + jitter * 0.4))
        pts.append((gx, gy + 14))
        d.line(
            [(x * S, y * S) for x, y in pts],
            fill=BRAND + (int(255 * min(1, 1.3 - p)),),
            width=7 * S,
            joint="curve",
        )
        d.line(
            [(x * S, y * S) for x, y in pts],
            fill=(255, 255, 255, int(200 * min(1, 1.3 - p))),
            width=2 * S,
            joint="curve",
        )

    # --- head ---
    rr(d, (146, 118, 366, 268), 44, fill=PANEL, outline=BRAND, width=5)
    rr(d, (128, 168, 150, 216), 10, outline=ACCENT, width=4)  # ears
    rr(d, (362, 168, 384, 216), 10, outline=ACCENT, width=4)
    rr(d, (166, 148, 346, 238), 26, fill=GROUND, outline=MUTED + (120,), width=2)  # visor

    # --- eyes ---
    look = 12 * math.sin(2 * math.pi * t * 0.5)
    if 0.60 <= t < 0.75:
        look = 14  # watch the ghost
    blink = 1.0
    if 0.44 <= t < 0.50:
        blink = abs(math.cos(math.pi * (t - 0.44) / 0.06))
    for ex in (256 - 36, 256 + 36):
        ry = 20 * blink
        d.ellipse(
            [(ex - 13) * S, (192 - ry) * S, (ex + 13) * S, (192 + ry) * S],
            fill=ACCENT,
        )
        d.ellipse(
            [(ex - 5 + look) * S, (192 - 7 * blink) * S, (ex + 5 + look) * S, (192 + 7 * blink) * S],
            fill=GROUND,
        )

    # --- body + gauge panel ---
    rr(d, (166, 278, 346, 404), 34, fill=PANEL, outline=MUTED + (120,), width=3)
    rr(d, (148, 292, 174, 340), 12, fill=PANEL, outline=BRAND, width=4)  # arms
    rr(d, (338, 292, 364, 340), 12, fill=PANEL, outline=BRAND, width=4)
    rr(d, (198, 404, 232, 434), 10, fill=PANEL, outline=MUTED + (120,), width=3)
    rr(d, (280, 404, 314, 434), 10, fill=PANEL, outline=MUTED + (120,), width=3)

    d.text((186 * S, 302 * S), "RAM", font=font(17), anchor="lm", fill=MUTED)
    tx0, tx1, ty0, ty1 = 232, 328, 296, 312
    rr(d, (tx0, ty0, tx1, ty1), 8, fill=GROUND, outline=MUTED + (120,), width=2)
    if frac > 0.02:
        rr(
            d,
            (tx0 + 2, ty0 + 2, lerp(tx0 + 2, tx1 - 2, frac), ty1 - 2),
            6,
            fill=gauge_color(frac),
        )
    d.text((186 * S, 330 * S), "swap", font=font(17), anchor="lm", fill=MUTED)
    rr(d, (232, 324, 328, 340), 8, fill=GROUND, outline=MUTED + (120,), width=2)
    rr(d, (234, 326, lerp(234, 326, 0.4), 338), 6, fill=ACCENT + (170,))

    # status brow: a forehead dot that reads the pressure
    led = gauge_color(frac)
    d.ellipse([(256 - 7) * S, 129 * S, (256 + 7) * S, 143 * S], fill=led)

    # --- poof ring ---
    if 0.76 <= t < 0.92:
        p = (t - 0.76) / 0.16
        r = lerp(28, 72, p)
        d.ellipse(
            [(gx - r) * S, (gy - r) * S, (gx + r) * S, (gy + r) * S],
            outline=MUTED + (int(220 * (1 - p)),),
            width=max(1, int(5 * (1 - p))) * S,
        )

    # --- poof X (the orphan is gone) ---
    if 0.80 <= t < 0.92:
        a = int(255 * (1 - (t - 0.80) / 0.12))
        for (x1, y1, x2, y2) in ((-16, -16, 16, 16), (16, -16, -16, 16)):
            d.line(
                [(gx + x1) * S, (gy + y1) * S, (gx + x2) * S, (gy + y2) * S],
                fill=BAD + (a,),
                width=5 * S,
            )

    # --- wordmark ---
    d.text((256 * S, 468 * S), "aimon", font=font(40), anchor="mm", fill=BRAND)

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

#!/usr/bin/env python3
"""Generate per-percent battery icons for razer-tray.

Output: 202 PNGs in icons/ named bat_0.png ... bat_100.png (and their
bat_*_c.png charging counterparts). Each icon is rendered at 256x256
and downsampled to 64x64 with LANCZOS for clean edges in the tray.

The visual idea (translucent disk + colored arc + centered percent
number) is generic battery iconography. This script is an independent
implementation written for this project; license: MIT.
"""

from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

OUTPUT_DIR = Path(__file__).resolve().parent.parent / "icons"

RENDER_SIZE = 256
FINAL_SIZE = 64
BORDER_FRAC = 1 / 16

FONT_PATH = "DejaVuSans-Bold.ttf"
FONT_FRAC_NORMAL = 0.56
FONT_FRAC_THREE_DIGIT = 0.42

BACKGROUND_FILL = (0, 0, 0, 140)
OUTLINE = (255, 255, 255, 48)
TEXT_COLOR = (255, 255, 255, 255)
CHARGING_ARC = (0, 220, 255, 230)


def arc_color(level: int) -> tuple:
    if level >= 50:
        return (60, 220, 90, 230)
    if level >= 20:
        return (255, 200, 0, 230)
    return (230, 50, 50, 245)


def render_icon(level: int, charging: bool) -> Image.Image:
    canvas = Image.new("RGBA", (RENDER_SIZE, RENDER_SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(canvas)
    border = int(RENDER_SIZE * BORDER_FRAC)
    last = RENDER_SIZE - 1

    draw.ellipse(
        (0, 0, last, last),
        fill=BACKGROUND_FILL,
        outline=OUTLINE,
        width=border,
    )

    arc_fill = CHARGING_ARC if charging else arc_color(level)
    draw.pieslice(
        (0, 0, last, last),
        start=-90,
        end=-90 + int(360 * level / 100),
        fill=arc_fill,
    )

    draw.ellipse(
        (border, border, last - border, last - border),
        fill=BACKGROUND_FILL,
    )

    font_size = int(
        RENDER_SIZE * (FONT_FRAC_THREE_DIGIT if level >= 100 else FONT_FRAC_NORMAL)
    )
    font = ImageFont.truetype(FONT_PATH, font_size)
    text = str(level)
    bbox = draw.textbbox((0, 0), text, font=font)
    text_w = bbox[2] - bbox[0]
    text_h = bbox[3] - bbox[1]
    pos = (
        (RENDER_SIZE - text_w) / 2 - bbox[0],
        (RENDER_SIZE - text_h) / 2 - bbox[1] - int(0.04 * RENDER_SIZE),
    )
    draw.text(pos, text, font=font, fill=TEXT_COLOR)

    return canvas.resize((FINAL_SIZE, FINAL_SIZE), Image.Resampling.LANCZOS)


def main() -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    count = 0
    for level in range(0, 101):
        for charging in (False, True):
            suffix = "_c" if charging else ""
            path = OUTPUT_DIR / f"bat_{level}{suffix}.png"
            render_icon(level, charging).save(path, optimize=True)
            count += 1
    print(f"Generated {count} icons in {OUTPUT_DIR}")


if __name__ == "__main__":
    main()

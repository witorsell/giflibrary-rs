#!/usr/bin/env python3
"""Composites an iFunny/ESMBot-style caption bar onto every frame of a webp
and re-encodes the result. Usage: caption_gif.py <in.webp> <out.webp> <caption text>
"""
import sys
from PIL import Image, ImageDraw, ImageFont

FONT_PATH = "/usr/share/fonts/truetype/msttcorefonts/arialbd.ttf"
MIN_FONT_SIZE = 14
MAX_LINES_BEFORE_SHRINK = 2
WIDTH_FRACTION = 0.92
VERTICAL_PADDING = 20
LINE_SPACING_FRACTION = 1.15


def wrap_text(draw, text, font, max_width):
    words = text.split()
    if not words:
        return []
    lines = []
    current = words[0]
    for word in words[1:]:
        candidate = f"{current} {word}"
        if draw.textlength(candidate, font=font) <= max_width:
            current = candidate
        else:
            lines.append(current)
            current = word
    lines.append(current)
    return lines


def fit_caption(draw, text, image_width):
    font_size = max(int(image_width * 0.09), MIN_FONT_SIZE)
    max_width = image_width * WIDTH_FRACTION
    while True:
        font = ImageFont.truetype(FONT_PATH, font_size)
        lines = wrap_text(draw, text, font, max_width)
        if len(lines) <= MAX_LINES_BEFORE_SHRINK or font_size <= MIN_FONT_SIZE:
            return font, lines
        font_size -= 2


def caption_frame(frame, lines, font, bar_height):
    w, h = frame.size
    canvas = Image.new("RGB", (w, h + bar_height), "white")
    canvas.paste(frame, (0, bar_height))
    draw = ImageDraw.Draw(canvas)
    line_height = font.size * LINE_SPACING_FRACTION
    total_text_height = line_height * len(lines)
    y = (bar_height - total_text_height) / 2
    for line in lines:
        line_width = draw.textlength(line, font=font)
        x = (w - line_width) / 2
        draw.text((x, y), line, font=font, fill="black")
        y += line_height
    return canvas


def main():
    if len(sys.argv) != 4:
        print("usage: caption_gif.py <input> <output> <caption text>", file=sys.stderr)
        sys.exit(1)

    in_path, out_path, caption = sys.argv[1], sys.argv[2], sys.argv[3]

    src = Image.open(in_path)
    n_frames = getattr(src, "n_frames", 1)

    probe = Image.new("RGB", src.size)
    probe_draw = ImageDraw.Draw(probe)
    font, lines = fit_caption(probe_draw, caption, src.size[0])
    line_height = font.size * LINE_SPACING_FRACTION
    bar_height = int(line_height * len(lines) + VERTICAL_PADDING * 2)

    frames = []
    durations = []
    for i in range(n_frames):
        src.seek(i)
        frame = src.convert("RGB")
        frames.append(caption_frame(frame, lines, font, bar_height))
        durations.append(src.info.get("duration", 100))

    if n_frames > 1:
        frames[0].save(
            out_path,
            format="WEBP",
            save_all=True,
            append_images=frames[1:],
            duration=durations,
            loop=src.info.get("loop", 0),
            lossless=False,
            quality=90,
        )
    else:
        frames[0].save(out_path, format="WEBP", lossless=False, quality=90)


if __name__ == "__main__":
    main()

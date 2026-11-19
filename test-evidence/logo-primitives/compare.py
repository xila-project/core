#!/usr/bin/env python3
"""Create native PNGs, review sheets and RGB differences from two capture runs."""

import argparse
import csv
from itertools import product
from pathlib import Path

from PIL import Image, ImageDraw


SIZES = (32, 64, 128, 160)
STYLES = ("filled", "outline")
BACKGROUNDS = ("light", "dark")
FORMATS = ("rgb565", "xrgb8888")


def decode(path, size, pixel_format):
    raw = path.read_bytes()
    stride, remainder = divmod(len(raw), size)
    assert remainder == 0
    if pixel_format == "xrgb8888":
        return Image.frombytes("RGB", (size, size), raw, "raw", "BGRX", stride)

    image = Image.new("RGB", (size, size))
    pixels = []
    for y, x in product(range(size), repeat=2):
        offset = y * stride + x * 2
        value = int.from_bytes(raw[offset : offset + 2], "little")
        red, green, blue = value >> 11, (value >> 5) & 63, value & 31
        pixels.append(
            ((red << 3) | (red >> 2), (green << 2) | (green >> 4), (blue << 3) | (blue >> 2))
        )
    image.putdata(pixels)
    return image


def compare(old, new):
    differences = [
        tuple(abs(a - b) for a, b in zip(before, after, strict=True))
        for before, after in zip(old.getdata(), new.getdata(), strict=True)
    ]
    maxima = [max(pixel) for pixel in differences]
    count = len(differences)
    diff = Image.new("RGB", old.size)
    diff.putdata([tuple(min(channel * 4, 255) for channel in pixel) for pixel in differences])
    stats = [
        max(maxima),
        f"{sum(map(sum, differences)) / (count * 3):.3f}",
        *(f"{100 * sum(value > threshold for value in maxima) / count:.3f}"
          for threshold in (8, 16, 32)),
    ]
    return diff, stats


def run(baseline, prototype, output):
    image_dir = output / "images"
    image_dir.mkdir(parents=True, exist_ok=True)
    with (output / "pixels.csv").open("w", newline="") as report:
        rows = csv.writer(report, lineterminator="\n")
        rows.writerow(
            ("size", "style", "background", "format", "max", "mean", "pct_gt8", "pct_gt16", "pct_gt32")
        )
        for style, background, pixel_format in product(STYLES, BACKGROUNDS, FORMATS):
            sheet = Image.new("RGB", (552, 524), "#303034")
            draw = ImageDraw.Draw(sheet)
            for x, label in zip((56, 224, 392), ("Rasterizer", "Primitives", "Difference x4"), strict=True):
                draw.text((x + 6, 6), label, fill="white")
            y = 23
            for size in SIZES:
                name = f"{size}-{style}-{background}-{pixel_format}"
                old = decode(baseline / f"{name}.raw", size, pixel_format)
                new = decode(prototype / f"{name}.raw", size, pixel_format)
                diff, metrics = compare(old, new)
                rows.writerow((size, style, background, pixel_format, *metrics))
                scale = 2 if size <= 64 else 1
                side = size * scale
                draw.text((5, y + side // 2 - 6), f"{size} px", fill="white")
                for x, kind, image in zip((56, 224, 392), ("old", "new", "diff"), (old, new, diff), strict=True):
                    image.save(image_dir / f"{name}-{kind}.png")
                    sheet.paste(image.resize((side, side), Image.Resampling.NEAREST), (x + (160 - side) // 2, y))
                y += side + 6
            sheet.save(output / f"{style}-{background}-{pixel_format}.png")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("prototype", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    run(args.baseline, args.prototype, args.output)

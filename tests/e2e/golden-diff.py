#!/usr/bin/env python3
"""Pixel compare of a screenshot against its approved golden (spec README
"screenshot", build plan §3.5).

Usage: golden-diff.py <golden.png> <shot.png> [--masks masks.json]
                      [--out diff.png] [--threshold 0.005] [--tolerance 32]

A pixel differs when, for every shift of the shot by at most 1 px, some
channel differs by more than `--tolerance`; the compare passes when the
share of differing pixels outside the masks is at most `--threshold` (0.5 % by default). `masks.json` = a list of
{"x", "y", "w", "h", "why"} rectangles in px (terminal contents, clock /
"ago" texts, the running LED's pulse). `--out` writes the differing pixels
in red over a dimmed copy of the shot. Exit 0 = within threshold, 1 = not,
2 = missing file or a size mismatch.
"""
import argparse
import json
import sys

from PIL import Image, ImageChops, ImageDraw

ap = argparse.ArgumentParser()
ap.add_argument("golden")
ap.add_argument("shot")
ap.add_argument("--masks")
ap.add_argument("--out")
ap.add_argument("--threshold", type=float, default=0.005)
ap.add_argument("--tolerance", type=int, default=32)
a = ap.parse_args()

try:
    g = Image.open(a.golden).convert("RGB")
    s = Image.open(a.shot).convert("RGB")
except OSError as e:
    print(f"golden-diff: {e}")
    sys.exit(2)
if g.size != s.size:
    print(f"golden-diff: size {s.size} != golden {g.size}")
    sys.exit(2)

masks = json.load(open(a.masks)) if a.masks else []
mask = Image.new("L", g.size, 255)  # 255 = compared
d = ImageDraw.Draw(mask)
for m in masks:
    d.rectangle([m["x"], m["y"], m["x"] + m["w"] - 1, m["y"] + m["h"] - 1], fill=0)

def maxdiff(shot):
    """Per-pixel largest channel difference between the golden and `shot`."""
    r, gg, b = ImageChops.difference(g, shot).split()
    return ImageChops.lighter(ImageChops.lighter(r, gg), b)


# A pixel matches if the shot has it within 1 px (a split that lands one
# pixel over between runs is not a regression): the smallest difference
# over the 9 shifts of the shot.
mx = None
for dx in (-1, 0, 1):
    for dy in (-1, 0, 1):
        md = maxdiff(ImageChops.offset(s, dx, dy))
        mx = md if mx is None else ImageChops.darker(mx, md)
hot = mx.point(lambda v: 255 if v > a.tolerance else 0)
hot = ImageChops.multiply(hot, mask)
n_hot = hot.histogram()[255]
n_all = mask.histogram()[255]
share = n_hot / max(n_all, 1)
ok = share <= a.threshold
print(f"golden-diff: {n_hot} of {n_all} compared pixels differ ({share:.4%}, threshold {a.threshold:.2%}) → {'ok' if ok else 'FAIL'}")
if a.out:
    base = Image.blend(s, Image.new("RGB", s.size, (0, 0, 0)), 0.6)
    base.paste((255, 0, 0), mask=hot)
    shade = Image.new("RGB", s.size, (0, 0, 80))
    base.paste(shade, mask=ImageChops.invert(mask).point(lambda v: 90 if v else 0))
    base.save(a.out)
sys.exit(0 if ok else 1)

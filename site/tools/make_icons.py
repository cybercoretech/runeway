#!/usr/bin/env python3
"""Builds the site's icons from Runeway's real sprite data.

Run from the repo root after `cargo run --example export_sprites`:
    python3 site/tools/make_icons.py
Writes site/assets/favicon.svg, then favicon.ico (16/32/48) and
apple-touch-icon.png (180) with rsvg-convert and ImageMagick.
"""
import json
import pathlib
import subprocess
import tempfile

ASSETS = pathlib.Path("site/assets")
data = json.loads((ASSETS / "sprites.json").read_text())
palette = data["palette"]
runner = data["sprites"]["rune_stand"]

# 16x16 grid: the 12x12 rune-runner centred on the night-sky ground.
size, offset = 16, 2
rects = [f'<rect width="{size}" height="{size}" fill="{palette[0]}"/>']
for i, ch in enumerate(runner["px"]):
    index = int(ch, 16)
    if index:
        x, y = i % runner["w"] + offset, i // runner["w"] + offset + 1
        rects.append(f'<rect x="{x}" y="{y}" width="1" height="1" fill="{palette[index]}"/>')
svg = (
    f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" '
    f'shape-rendering="crispEdges">{"".join(rects)}</svg>\n'
)
(ASSETS / "favicon.svg").write_text(svg)

with tempfile.TemporaryDirectory() as tmp:
    pngs = []
    for px in (16, 32, 48):
        out = pathlib.Path(tmp) / f"icon-{px}.png"
        subprocess.run(["rsvg-convert", "-w", str(px), "-h", str(px), str(ASSETS / "favicon.svg"), "-o", str(out)], check=True)
        pngs.append(str(out))
    subprocess.run(["magick", *pngs, str(ASSETS / "favicon.ico")], check=True)
subprocess.run(["rsvg-convert", "-w", "180", "-h", "180", str(ASSETS / "favicon.svg"), "-o", str(ASSETS / "apple-touch-icon.png")], check=True)
print("wrote favicon.svg, favicon.ico, apple-touch-icon.png")

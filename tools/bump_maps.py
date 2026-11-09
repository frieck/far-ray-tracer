#!/usr/bin/env python3
"""Build the bump map examples: two maps in assets/scene-data/images and assets/scenes/bump-maps.far.

- tiles-height.png: a height map of square tiles with rounded edges and sunken grout.
- hammered-normal.png: a tangent-space normal map (OpenGL, green up) of hammered metal, made from
  random round dents. Both repeat seamlessly.
"""
import json
import math
import random
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
IMAGES = ROOT / "assets" / "scene-data" / "images"
SIZE = 512


def write_png(path, pixel):
    rows = bytearray()
    for y in range(SIZE):
        rows.append(0)
        for x in range(SIZE):
            rows.extend(max(0, min(255, round(c * 255))) for c in pixel(x, y))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b""))
    path.write_bytes(png)
    print(f"{path}: {len(png)} bytes")


def tiles(x, y):
    """Height of 4 x 4 tiles: flat tops, a rounded bevel, sunken grout."""
    tile = SIZE / 4
    edge = min(x % tile, tile - x % tile, y % tile, tile - y % tile)
    grout, bevel = 5, 18
    if edge < grout:
        return 0.0
    t = min(1.0, (edge - grout) / bevel)
    return 0.15 + 0.85 * math.sin(t * math.pi / 2)


random.seed(2008)
DENTS = [(random.uniform(0, SIZE), random.uniform(0, SIZE), random.uniform(14, 34), random.uniform(0.5, 1.0))
         for _ in range(170)]


def hammered(x, y):
    """Height of overlapping round dents, wrapped so the map repeats."""
    h = 0.0
    for cx, cy, radius, depth in DENTS:
        dx = (x - cx + SIZE / 2) % SIZE - SIZE / 2
        dy = (y - cy + SIZE / 2) % SIZE - SIZE / 2
        d2 = (dx * dx + dy * dy) / (radius * radius)
        if d2 < 1:
            h = min(h, -depth * (1 - d2) * radius / 34)
    return h


def main():
    write_png(IMAGES / "tiles-height.png", lambda x, y: (tiles(x, y),) * 3)

    heights = [[hammered(x, y) for x in range(SIZE)] for y in range(SIZE)]
    slope = 6.0

    def normal(x, y):
        dx = (heights[y][(x + 1) % SIZE] - heights[y][(x - 1) % SIZE]) / 2
        drow = (heights[(y + 1) % SIZE][x] - heights[(y - 1) % SIZE][x]) / 2
        # Image up is against the rows, so the up slope is -drow; the normal leans against slopes.
        nx, ny, nz = -dx * slope, drow * slope, 1.0
        length = math.sqrt(nx * nx + ny * ny + nz * nz)
        return (0.5 + 0.5 * nx / length, 0.5 + 0.5 * ny / length, 0.5 + 0.5 * nz / length)

    write_png(IMAGES / "hammered-normal.png", normal)

    def material(color, preset, roughness, bump, **extra):
        return {"color": color, "surface": {"preset": preset, "roughness": roughness}, "bump": bump, **extra}

    tiles_bump = {"map": "tiles-height.png", "kind": "height", "strength": 0.035}
    hammered_bump = {"map": "hammered-normal.png", "kind": "normal", "strength": 1.0}
    scene = {
        "camera": {"position": [0, 1.7, -6.2], "target": [0, 0.55, 0], "zoom": 1.25},
        "environment": {"path": "../../environments/studio.hdr", "intensity": 0.75, "rotation": 150, "visible": True},
        "lights": [{"type": "rectangle", "position": [-4, 5, -3], "edge_u": [2, 0, 0], "edge_v": [0, 0, 2],
                    "color": [1, 0.95, 0.88], "intensity": 7}],
        "objects": [
            {"type": "ground", "position": [0, -0.5, 0],
             "material": material([0.62, 0.6, 0.57], "ceramic", 0.3, tiles_bump, scale_u=0.222, scale_v=0.222)},
            {"type": "sphere", "center": [-1.9, 0.6, 0.4], "radius": 1.1,
             "material": material([1.0, 0.77, 0.36], "metal", 0.22, hammered_bump, scale_u=0.33, scale_v=0.5)},
            {"type": "box", "center": [0.5, 0.15, 0.9], "size": [1.3, 1.3, 1.3], "rotation": [0, 28, 0],
             "material": material([0.2, 0.42, 0.62], "ceramic", 0.2, tiles_bump)},
            {"type": "torus", "center": [2.3, -0.12, -0.4], "major_radius": 0.75, "minor_radius": 0.32,
             "rotation": [0, 20, 0], "material": material([0.92, 0.93, 0.95], "metal", 0.12, hammered_bump,
                                                           scale_u=0.25, scale_v=0.5)},
        ],
    }
    out = ROOT / "assets" / "scenes" / "materials" / "bump-maps.far"
    out.write_text(json.dumps(scene, indent=1) + "\n", encoding="utf-8")
    print(f"{out}: {len(scene['objects'])} objects")


main()

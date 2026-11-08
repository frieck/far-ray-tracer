#!/usr/bin/env python3
"""Build the displacement example: assets/scene-data/images/cobbles-height.png and
assets/scenes/displacement.far.

The map is a seamless height field of rounded stones: Voronoi cells, high in the middle and
falling into the joints between them.
"""
import json
import math
import random
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SIZE = 384
random.seed(2008)
STONES = [(random.uniform(0, SIZE), random.uniform(0, SIZE), random.uniform(0.75, 1.0)) for _ in range(46)]


def height(x, y):
    """Distance to the nearest joint, rounded into a stone, wrapped so the map repeats."""
    nearest = []
    for sx, sy, top in STONES:
        dx = (x - sx + SIZE / 2) % SIZE - SIZE / 2
        dy = (y - sy + SIZE / 2) % SIZE - SIZE / 2
        nearest.append((dx * dx + dy * dy, top))
    nearest.sort()
    (d1, top), (d2, _) = nearest[0], nearest[1]
    edge = (math.sqrt(d2) - math.sqrt(d1)) / 2  # Distance to the joint between the two stones.
    t = min(1.0, max(0.0, (edge - 1.5) / 16))
    return top * math.sin(t * math.pi / 2) ** 0.7


def write_png(path, value):
    rows = bytearray()
    for y in range(SIZE):
        rows.append(0)
        for x in range(SIZE):
            rows.extend([max(0, min(255, round(value(x, y) * 255)))] * 3)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 2, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b""))
    print(f"{path}: {path.stat().st_size} bytes")


write_png(ROOT / "assets" / "scene-data" / "images" / "cobbles-height.png", height)

stone = {"color": [0.55, 0.52, 0.48], "surface": {"preset": "ceramic", "roughness": 0.55}}
scene = {
    "camera": {"position": [0, 2.1, -6.4], "target": [0, 0.5, 0.4], "zoom": 1.25},
    "environment": {"path": "../environments/studio.hdr", "intensity": 0.75, "rotation": 150, "visible": True},
    "lights": [{"type": "rectangle", "position": [-5, 4.5, -2], "edge_u": [2, 0, 0], "edge_v": [0, 0, 2],
                "color": [1, 0.94, 0.85], "intensity": 9}],
    "objects": [
        # The ground's relief is traced by marching rays over the map.
        {"type": "ground", "position": [0, -0.5, 0],
         "material": {**stone, "scale_u": 0.16, "scale_v": 0.16,
                      "displacement": {"map": "cobbles-height.png", "height": 0.12}}},
        # The same map on objects, which become displaced meshes.
        {"type": "sphere", "center": [-1.6, 0.65, 0.6], "radius": 1.0,
         "material": {"color": [0.78, 0.35, 0.2], "surface": {"preset": "plastic", "roughness": 0.35},
                      "scale_u": 0.5, "scale_v": 0.5,
                      "displacement": {"map": "cobbles-height.png", "height": 0.1, "detail": 160}}},
        {"type": "box", "center": [0.9, 0.15, 1.0], "size": [1.2, 1.3, 1.2], "rotation": [0, 30, 0],
         "material": {"color": [0.6, 0.62, 0.66], "surface": {"preset": "ceramic", "roughness": 0.4},
                      "displacement": {"map": "cobbles-height.png", "height": 0.07, "detail": 160}}},
        # For comparison: the same map as a bump only. The outline stays smooth.
        {"type": "sphere", "center": [2.5, 0.05, -0.6], "radius": 0.55,
         "material": {"color": [0.78, 0.35, 0.2], "surface": {"preset": "plastic", "roughness": 0.35},
                      "scale_u": 0.5, "scale_v": 0.5,
                      "bump": {"map": "cobbles-height.png", "kind": "height", "strength": 0.055}}},
    ],
}
out = ROOT / "assets" / "scenes" / "displacement.far"
out.write_text(json.dumps(scene, indent=1) + "\n", encoding="utf-8")
print(f"{out}: {len(scene['objects'])} objects")

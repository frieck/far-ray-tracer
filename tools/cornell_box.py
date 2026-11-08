#!/usr/bin/env python3
"""Build assets/scenes/cornell-box.far, the classic test of indirect light.

A white room with a red wall on the left and a green wall on the right, lit by one
panel in the ceiling. The colors approximate the reflectances measured on the real
box at Cornell University. Light reaches the ceiling and tints the blocks only by
bouncing, so the scene shows at a glance whether indirect light works.
"""
import json
from pathlib import Path

H = 2.75          # half the side of the room
DEPTH = 5.5
WHITE = {"color": [0.73, 0.73, 0.73], "specular": 0}
RED = {"color": [0.63, 0.065, 0.05], "specular": 0}
GREEN = {"color": [0.14, 0.45, 0.091], "specular": 0}
INSIDE = [0.0, 0.0, DEPTH / 2]


def sub(a, b):
    return [x - y for x, y in zip(a, b)]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def wall(points, material):
    """A plane facing the inside of the room. Planes show only the side their
    normal, (p3 - p0) x (p1 - p0), points to, so the order is fixed here."""
    normal = cross(sub(points[3], points[0]), sub(points[1], points[0]))
    to_inside = sub(INSIDE, points[0])
    if sum(n * t for n, t in zip(normal, to_inside)) < 0:
        points = points[::-1]
    return {"type": "plane", "points": points, "material": material}


x0, x1, y0, y1, z0, z1 = -H, H, -H, H, 0.0, DEPTH
objects = [
    wall([[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]], WHITE),  # floor
    wall([[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]], WHITE),  # ceiling
    wall([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]], WHITE),  # back
    wall([[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]], RED),    # left
    wall([[x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]], GREEN),  # right
    # Tall block at the back left and short block at the front right, as in the original.
    {"type": "box", "center": [-0.95, y0 + 1.65, 3.5], "size": [1.65, 3.3, 1.65], "rotation": [0, 17, 0], "material": WHITE},
    {"type": "box", "center": [0.95, y0 + 0.825, 1.85], "size": [1.65, 1.65, 1.65], "rotation": [0, -17, 0], "material": WHITE},
]

scene = {
    "camera": {"position": [0, 0, -9.6], "target": [0, 0, 0], "zoom": 2.72},
    "lights": [{
        "type": "rectangle", "position": [0, y1 - 0.01, DEPTH / 2],
        "edge_u": [1.3, 0, 0], "edge_v": [0, 0, 1.05],
        "color": [1.0, 0.9, 0.75], "intensity": 85,
    }],
    "objects": objects,
}
out = Path(__file__).resolve().parent.parent / "assets" / "scenes" / "cornell-box.far"
out.write_text(json.dumps(scene, indent=1) + "\n", encoding="utf-8")
print(f"{out}: {len(objects)} objects")

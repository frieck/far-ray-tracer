#!/usr/bin/env python3
"""Build assets/scenes/techniques/bokeh.far: shaped bokeh from a string of lights.

A gold sphere sits in focus on a dark glossy floor. Behind it, far out of focus, hang strings
of small glowing bulbs; through a six-bladed aperture each becomes a hexagon. Change Aperture
shape in Camera > Depth of field to see round or other shapes. The bulbs are small, so the
discs need many passes to fill in: 512 or more, with Denoise. AgX Punchy suits it.
"""
import json
import math
import random
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GROUND = -1.0
WARM = [[1.0, 0.72, 0.38], [1.0, 0.6, 0.3], [1.0, 0.85, 0.55]]
ACCENT = [[0.45, 0.65, 1.0], [1.0, 0.45, 0.35]]


def bulbs():
    """Two strings sagging between posts, far behind the focus plane."""
    random.seed(7)
    lights = []
    for z, height, sag, span, count in [(22, 2.4, 1.0, 11, 11), (30, 4.6, 1.2, 15, 12)]:
        for i in range(count):
            t = i / (count - 1)
            x = -span + 2 * span * t + random.uniform(-0.2, 0.2)
            y = height - sag * 4 * t * (1 - t)
            color = random.choice(ACCENT) if random.random() < 0.2 else random.choice(WARM)
            lights.append({"type": "sphere", "center": [round(x, 2), round(y, 2), z], "radius": 0.18,
                           "material": {"color": [0, 0, 0], "specular": 0,
                                        "emission": {"color": color, "strength": 14}}})
    return lights


scene = {
    "camera": {"position": [0, 0.2, -3], "target": [0, 0.15, 4], "zoom": 1.2,
               "aperture": 0.9, "focus_distance": 6.9, "blades": 6, "blade_rotation": 0},
    # The studio, dim and hidden behind the black background, gives the metal something to
    # reflect.
    "environment": {"path": "../../environments/studio.hdr", "intensity": 0.3, "rotation": 200, "visible": False},
    "lights": [
        {"type": "rectangle", "position": [-4.5, 4.5, 1.5], "edge_u": [0, 0, 2], "edge_v": [-1.6, -0.6, 0],
         "color": [1, 0.92, 0.82], "intensity": 9},
    ],
    "objects": [
        {"type": "ground", "position": [0, GROUND, 0],
         "material": {"color": [0.06, 0.06, 0.07], "surface": {"preset": "plastic", "roughness": 0.18}}},
        {"type": "sphere", "center": [0, GROUND + 1.0, 4], "radius": 1.0,
         "material": {"color": [1.0, 0.78, 0.4], "surface": {"preset": "metal", "roughness": 0.15}}},
        {"type": "sphere", "center": [-1.9, GROUND + 0.45, 3.2], "radius": 0.45,
         "material": {"color": [0.75, 0.1, 0.12], "surface": {"preset": "plastic", "roughness": 0.3, "clearcoat": 1.0}}},
    ] + bulbs(),
}

path = ROOT / "assets" / "scenes" / "techniques" / "bokeh.far"
path.write_text(json.dumps(scene, indent=1) + "\n", encoding="utf-8")
print(f"{path}: {len(scene['objects'])} objects")

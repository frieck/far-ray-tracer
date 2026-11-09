#!/usr/bin/env python3
"""Build assets/scenes/techniques/light-shafts.far: sunlight through a window into a hazy room.

The scene's fog fills its bounds, the room, so beams form only inside, where the window's bars
cut them into shafts. A strong rectangular light outside plays the sun. A sphere of denser,
warmer smoke sits in the beams, and forward scattering (anisotropy 0.5) makes the shafts glow
brighter when looking toward the light, as dust in sunlight does.
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PLASTER = {"color": [0.3, 0.29, 0.27], "surface": {"preset": "plastic", "roughness": 0.85}}
WOOD = {"color": [0.3, 0.19, 0.11], "surface": {"preset": "plastic", "roughness": 0.5}}

# Room: x from -4 to 4, y from 0 to 6, z from 0 to 12; the camera looks in from the open front.
X0, X1, H, Z1 = -4.0, 4.0, 6.0, 12.0
WINDOW = (2.0, 5.2, 3.0, 9.0)  # y low, y high, z low, z high
T = 0.2  # wall thickness


def box(center, size, material):
    return {"type": "box", "center": center, "size": size, "material": material}


def left_wall():
    y0, y1, z0, z1 = WINDOW
    x = X0 - T / 2
    parts = [
        box([x, y0 / 2, Z1 / 2], [T, y0, Z1], PLASTER),
        box([x, (y1 + H) / 2, Z1 / 2], [T, H - y1, Z1], PLASTER),
        box([x, (y0 + y1) / 2, z0 / 2], [T, y1 - y0, z0], PLASTER),
        box([x, (y0 + y1) / 2, (z1 + Z1) / 2], [T, y1 - y0, Z1 - z1], PLASTER),
    ]
    # Bars: two upright and one across.
    for z in (z0 + (z1 - z0) / 3, z0 + 2 * (z1 - z0) / 3):
        parts.append(box([x, (y0 + y1) / 2, z], [T, y1 - y0, 0.14], WOOD))
    parts.append(box([x, (y0 + y1) / 2, (z0 + z1) / 2], [T, 0.14, z1 - z0], WOOD))
    return parts


scene = {
    "camera": {"position": [3.4, 1.8, -1.5], "target": [-2.2, 2.4, 6.5], "zoom": 0.95},
    "fog": {"density": 0.08, "color": [0.9, 0.9, 0.9], "anisotropy": 0.5},
    "lights": [
        # The sun, outside and above the window, shining down through it.
        {"type": "rectangle", "position": [-21.0, 22.0, 6.0], "edge_u": [0, 0, 1.2],
         "edge_v": [-0.77, -0.65, 0], "color": [1.0, 0.86, 0.66], "intensity": 14000},
    ],
    "objects": [
        {"type": "ground", "position": [0, 0, 0], "material": WOOD},
        box([X1 + T / 2, H / 2, Z1 / 2], [T, H, Z1], PLASTER),
        box([0, H / 2, Z1 + T / 2], [8 + 2 * T, H, T], PLASTER),
        box([0, H + T / 2, Z1 / 2], [8 + 2 * T, T, Z1], PLASTER),
        *left_wall(),
        {"type": "sphere", "center": [0.4, 0.9, 6.5], "radius": 0.9,
         "material": {"color": [0.9, 0.9, 0.92], "surface": {"preset": "ceramic", "roughness": 0.2}}},
        box([1.8, 0.7, 9.0], [1.4, 1.4, 1.4], {"color": [0.25, 0.35, 0.6], "surface": {"preset": "plastic", "roughness": 0.4}}),
        # A cloud of warmer smoke in the beams.
        {"type": "volume", "center": [-1.4, 1.4, 4.0], "radius": 1.0, "density": 0.9,
         "color": [0.95, 0.75, 0.55], "anisotropy": 0.3},
    ],
}

path = ROOT / "assets" / "scenes" / "techniques" / "light-shafts.far"
path.write_text(json.dumps(scene, indent=1) + "\n")
print(path)

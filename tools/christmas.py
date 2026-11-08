#!/usr/bin/env python3
"""Build assets/scenes/christmas.far: gifts, ornaments and a small tree by a warm window."""
import json
import math
import random
from pathlib import Path

random.seed(2008)
FLOOR = -2.0
objects, lights = [], []


def surface(color, preset, roughness, **extra):
    return {"color": color, "surface": {"preset": preset, "roughness": roughness, **extra}}


GOLD = surface([1.0, 0.76, 0.33], "metal", 0.18)
SILVER = surface([0.92, 0.93, 0.95], "metal", 0.08)
GLASS = {"color": [0.97, 0.99, 1.0], "diffuse": 0, "specular": 0, "refraction": 1, "ior": 1.5}


def sphere(center, radius, material):
    objects.append({"type": "sphere", "center": center, "radius": radius, "material": material})


def box(center, size, material, yaw=0.0):
    objects.append({"type": "box", "center": center, "size": size, "rotation": [0, yaw, 0], "material": material})


def ornament(x, z, radius, material, cap_yaw=0.0):
    """A bauble resting on the floor, with its metal cap and hanging loop."""
    y = FLOOR + radius
    sphere([x, y, z], radius, material)
    top = y + radius
    objects.append({"type": "cylinder", "center": [x, top + 0.04, z], "radius": radius * 0.22,
                    "height": radius * 0.2, "material": GOLD})
    objects.append({"type": "torus", "center": [x, top + 0.17, z], "major_radius": radius * 0.12,
                    "minor_radius": radius * 0.03, "rotation": [90, cap_yaw, 0], "material": GOLD})


def gift(x, z, size, color, ribbon, yaw, base=FLOOR):
    """A wrapped box with two crossing ribbons and a bow."""
    w, h, d = size
    cy = base + h / 2
    paper = surface(color, "plastic", 0.45)
    band = surface(ribbon, "metal", 0.3)
    box([x, cy, z], [w, h, d], paper, yaw)
    box([x, cy, z], [w + 0.02, h + 0.02, 0.16], band, yaw)
    box([x, cy, z], [0.16, h + 0.02, d + 0.02], band, yaw)
    top = base + h + 0.01
    for angle in (yaw + 35, yaw - 35):
        objects.append({"type": "torus", "center": [x, top + 0.14, z], "major_radius": 0.16,
                        "minor_radius": 0.045, "rotation": [90, angle, 0], "material": band})
    return base + h


# Room: oak floor, deep green wall, warm window light.
objects.append({"type": "plane", "points": [[-30, FLOOR, -10], [30, FLOOR, -10], [30, FLOOR, 40], [-30, FLOOR, 40]],
                "material": {"color": [0.62, 0.42, 0.26], "texture": "wood.tga", "scale_u": 1.2, "scale_v": 1.2,
                             "surface": {"preset": "plastic", "roughness": 0.35}}})
objects.append({"type": "plane", "points": [[-30, FLOOR, 18], [30, FLOOR, 18], [30, 20, 18], [-30, 20, 18]],
                "material": surface([0.42, 0.32, 0.24], "ceramic", 0.85)})

# Tree: stacked cones on a trunk, decorated with baubles and warm bulbs.
TX, TZ = 2.6, 12.0
NEEDLES = surface([0.05, 0.22, 0.1], "plastic", 0.7)
objects.append({"type": "cylinder", "center": [TX, FLOOR + 0.45, TZ], "radius": 0.35, "height": 0.9,
                "material": surface([0.3, 0.17, 0.08], "plastic", 0.8)})
tiers = [(0.9, 2.6, 3.0), (2.3, 2.1, 2.6), (3.6, 1.6, 2.2), (4.7, 1.1, 1.8)]
for base, radius, height in tiers:
    objects.append({"type": "cone", "center": [TX, FLOOR + base + height / 2, TZ], "radius": radius,
                    "height": height, "material": NEEDLES})
tip = FLOOR + tiers[-1][0] + tiers[-1][2]
sphere([TX, tip + 0.15, TZ], 0.22, surface([1.0, 0.8, 0.35], "metal", 0.05))

bauble_colors = [[0.75, 0.04, 0.05], [1.0, 0.76, 0.33], [0.85, 0.87, 0.9], [0.55, 0.05, 0.12]]
for i, (base, radius, height) in enumerate(tiers):
    count = 7 - i
    for k in range(count):
        a = math.radians(200 + 360 * k / count + i * 23)
        h = 0.3 + 0.12 * (k % 2)
        y = FLOOR + base + h * height
        r = radius * (1 - h) + 0.12
        x, z = TX + r * math.cos(a), TZ + r * math.sin(a)
        color = bauble_colors[(k + i) % len(bauble_colors)]
        sphere([x, y - 0.08, z], 0.17, surface(color, "metal", 0.12))
    # Warm bulbs along the lower rim of each tier, where the next tier does not hide them.
    for k in range(6 - i):
        b = math.radians(205 + 130 * (k + 0.5) / (6 - i) + 9 * i)
        h = 0.1
        rb = radius * (1 - h) + 0.07
        p = [TX + rb * math.cos(b), FLOOR + base + h * height, TZ + rb * math.sin(b)]
        lights.append({"type": "sphere", "position": p, "radius": 0.05, "color": [1.0, 0.72, 0.4], "intensity": 22})

# Gifts under and beside the tree.
top = gift(-2.4, 8.0, [1.9, 1.3, 1.6], [0.6, 0.03, 0.04], [1.0, 0.78, 0.38], 18)
gift(-2.3, 8.0, [1.1, 0.8, 1.0], [0.9, 0.9, 0.86], [0.7, 0.04, 0.05], -12, base=top)
gift(-0.2, 10.4, [1.4, 1.0, 1.4], [0.05, 0.25, 0.13], [0.9, 0.9, 0.92], 40)
gift(4.9, 10.2, [1.2, 0.9, 1.2], [0.8, 0.62, 0.3], [0.6, 0.03, 0.04], -25)
gift(0.7, 12.8, [1.6, 0.7, 1.0], [0.12, 0.14, 0.4], [1.0, 0.78, 0.38], 10)

# Baubles in the foreground, the focus of the shot.
ornament(0.2, 5.0, 0.6, surface([0.72, 0.03, 0.04], "metal", 0.1), 20)
ornament(-0.95, 5.6, 0.42, GOLD, -30)
ornament(1.3, 5.9, 0.45, GLASS, 60)
ornament(2.1, 6.9, 0.38, SILVER, 10)
ornament(-1.2, 4.5, 0.25, surface([0.05, 0.3, 0.14], "metal", 0.15), 0)

# A string of lights along the wall, blurred by the lens into soft discs.
for k in range(26):
    x = -9 + 18 * k / 25
    y = 4.6 - 0.9 * math.cos(math.pi * (x + 9) / 6) ** 2
    lights.append({"type": "sphere", "position": [x, y, 17.85], "radius": 0.06, "color": [1.0, 0.68, 0.32], "intensity": 14})

# Window light from the left and a dim cool fill from the right.
lights.append({"type": "rectangle", "position": [-7, 4.5, 6], "edge_u": [0, 4, 0], "edge_v": [0, 0, 6],
               "color": [1.0, 0.82, 0.6], "intensity": 4})
lights.append({"type": "rectangle", "position": [8, 3, 2], "edge_u": [0, 0, 3], "edge_v": [0, 3, 0],
               "color": [0.55, 0.68, 1.0], "intensity": 1.2})

scene = {
    "camera": {"position": [-0.4, 0.4, -0.6], "target": [0.6, -1.0, 8.0], "zoom": 1.15,
               "aperture": 0.1, "focus_distance": 5.9},
    "environment": {"path": "../environments/studio.hdr", "intensity": 0.5, "rotation": 120, "visible": False},
    "lights": lights,
    "objects": objects,
}
out = Path(__file__).resolve().parent.parent / "assets" / "scenes" / "christmas.far"
out.write_text(json.dumps(scene, indent=1) + "\n", encoding="utf-8")
print(f"{out}: {len(objects)} objects, {len(lights)} lights")

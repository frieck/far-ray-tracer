#!/usr/bin/env python3
"""Build the light and atmosphere examples in assets/scenes/techniques, and their IES files.

- spotlights.far: three stage lights in thin haze: a hard spot, a soft spot and two IES
  fixtures (a narrow downlight and a wall washer) whose beams show in the fog.
- sun-and-sky.far: a courtyard lit only by the physical sky and sun, late in the afternoon.
- smoke.far: a column of smoke and a low cloud bank from noise, and ground fog that thins with
  height, lit by a low sun.

The IES files are made up here, with the shapes of common fixtures, so no manufacturer's data
is copied. They go in assets/scene-data/ies.
"""
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCENES = ROOT / "assets" / "scenes" / "techniques"
IES = ROOT / "assets" / "scene-data" / "ies"


def write_ies(name, title, vertical, horizontal, candela):
    """An LM-63-2002 file with type C photometry; `candela(v, h)` gives the table."""
    lines = ["IESNA:LM-63-2002", f"[TEST] {title}", "[MANUFAC] Far Ray Tracer example, made up", "TILT=NONE",
             f"1 1000 1 {len(vertical)} {len(horizontal)} 1 2 0.1 0.1 0.05", "1 1 30"]
    lines.append(" ".join(f"{v:g}" for v in vertical))
    lines.append(" ".join(f"{h:g}" for h in horizontal))
    for h in horizontal:
        lines.append(" ".join(f"{candela(v, h):.1f}" for v in vertical))
    path = IES / name
    path.write_text("\n".join(lines) + "\n", encoding="ascii")
    print(f"{path}: {len(vertical)} x {len(horizontal)} angles")


def ies_files():
    IES.mkdir(parents=True, exist_ok=True)
    vertical = [i * 2.5 for i in range(73)]  # 0 to 180 degrees
    # A narrow downlight: a bright core, a soft shoulder and a faint halo from the reflector.
    def downlight(v, h):
        core = math.exp(-(v / 14) ** 2)
        halo = 0.12 * math.exp(-((v - 32) / 10) ** 2)
        return 2000 * (core + halo) if v < 90 else 0.0
    write_ies("narrow-downlight.ies", "Narrow downlight", vertical, [0], downlight)
    # A wall washer: thrown to one side (horizontal 0) and cut off sharply on the other.
    horizontal = [h * 15 for h in range(13)]  # 0 to 180, bilateral symmetry
    def washer(v, h):
        if v >= 90:
            return 0.0
        toward = math.cos(math.radians(h))
        aim = 35 * max(toward, 0)
        spread = math.exp(-((v - aim) / (22 + 10 * (1 - abs(toward)))) ** 2)
        return 1500 * spread * (0.35 + 0.65 * max(toward, 0))
    write_ies("wall-washer.ies", "Asymmetric wall washer", vertical, horizontal, washer)


def plastic(color, roughness=0.5, **extra):
    return {"color": color, "surface": {"preset": "plastic", "roughness": roughness, **extra}}


def spotlights():
    return {
        "camera": {"position": [0, 2.2, -7.5], "target": [0, 1.5, 2], "zoom": 1.0},
        "fog": {"density": 0.035, "color": [0.9, 0.9, 0.9], "anisotropy": 0.3, "bounds": [[-6, -0.01, -3], [6, 6, 5]]},
        "lights": [
            # A hard spot on the sphere, from above left.
            {"type": "sphere", "position": [-3.5, 5.2, -0.5], "radius": 0.06, "color": [1, 0.9, 0.75], "intensity": 9000,
             "direction": [0.55, -1, 0.45], "spot_angle": 24, "spot_blend": 0.05},
            # A wide, soft spot from the right, in blue.
            {"type": "sphere", "position": [3.8, 5.0, -1.0], "radius": 0.06, "color": [0.45, 0.6, 1], "intensity": 7000,
             "direction": [-0.6, -1, 0.5], "spot_angle": 50, "spot_blend": 0.6},
            # IES fixtures: a narrow downlight over the pedestal and a wall washer on the back wall.
            {"type": "sphere", "position": [1.4, 5.6, 1.6], "radius": 0.05, "color": [1, 0.85, 0.65], "intensity": 14000,
             "ies": "../../scene-data/ies/narrow-downlight.ies"},
            {"type": "sphere", "position": [-1.0, 5.6, 3.6], "radius": 0.05, "color": [1, 0.75, 0.55], "intensity": 12000,
             "direction": [0, -1, 0.15], "ies": "../../scene-data/ies/wall-washer.ies"},
        ],
        "objects": [
            {"type": "ground", "position": [0, 0, 0], "material": plastic([0.35, 0.35, 0.36], 0.35)},
            {"type": "box", "center": [0, 3, 4.6], "size": [12, 6, 0.2], "material": plastic([0.7, 0.68, 0.65], 0.8)},
            {"type": "sphere", "center": [-1.4, 0.8, 0.8], "radius": 0.8, "material": plastic([0.85, 0.85, 0.85], 0.3)},
            {"type": "cylinder", "center": [1.4, 0.45, 1.6], "radius": 0.55, "height": 0.9, "material": plastic([0.2, 0.2, 0.22], 0.4)},
            {"type": "sphere", "center": [1.4, 1.3, 1.6], "radius": 0.4,
             "material": {"color": [1, 0.78, 0.4], "surface": {"preset": "metal", "roughness": 0.15}}},
        ],
    }


def sun_and_sky():
    walls = [
        {"type": "box", "center": [-4.5, 1.6, 3], "size": [0.4, 3.2, 9], "material": plastic([0.82, 0.72, 0.6], 0.8)},
        {"type": "box", "center": [0, 1.6, 7.3], "size": [9.4, 3.2, 0.4], "material": plastic([0.82, 0.72, 0.6], 0.8)},
    ]
    columns = [{"type": "cylinder", "center": [3.4, 1.6, z], "radius": 0.22, "height": 3.2,
                "material": plastic([0.9, 0.88, 0.84], 0.6)} for z in (0.5, 2.5, 4.5, 6.5)]
    return {
        "camera": {"position": [-1.5, 1.6, -5.5], "target": [0.8, 1.2, 3], "zoom": 0.85},
        # Sao Paulo's latitude, on the autumn equinox, at 4:30 in the afternoon.
        "environment": {"sky": {"turbidity": 3, "time": {"hour": 16.5, "latitude": -23.5, "day": 80}},
                        "intensity": 1, "rotation": 0, "visible": True},
        "lights": [],
        "objects": [
            {"type": "ground", "position": [0, 0, 0], "material": plastic([0.55, 0.5, 0.45], 0.7)},
            *walls, *columns,
            {"type": "sphere", "center": [0, 0.7, 3], "radius": 0.7,
             "material": {"color": [0.95, 0.95, 0.97], "surface": {"preset": "metal", "roughness": 0.05}}},
            {"type": "box", "center": [-1.8, 0.4, 1.2], "size": [0.8, 0.8, 0.8], "rotation": [0, 25, 0],
             "material": plastic([0.15, 0.35, 0.7], 0.4)},
        ],
    }


def smoke():
    return {
        "camera": {"position": [0, 1.6, -8], "target": [0, 2.8, 3], "zoom": 0.85},
        "environment": {"sky": {"elevation": 18, "azimuth": 235, "turbidity": 3.5}, "intensity": 1, "visible": True},
        # Ground fog: dense at the ground, thin above a meter.
        "fog": {"density": 0.1, "color": [0.92, 0.92, 0.95], "anisotropy": 0.4, "falloff": 0.5,
                "bounds": [[-30, 0, -10], [30, 6, 40]]},
        "lights": [],
        "objects": [
            {"type": "ground", "position": [0, 0, 0], "material": plastic([0.3, 0.33, 0.28], 0.8)},
            # A column of smoke from a chimney, billowing and thinning upward.
            {"type": "cylinder", "center": [2.2, 0.9, 4], "radius": 0.45, "height": 1.8, "material": plastic([0.4, 0.2, 0.15], 0.8)},
            {"type": "volume", "center": [2.4, 4.0, 4], "size": [2.4, 4.4, 2.4], "density": 7.0, "color": [0.55, 0.55, 0.56],
             "anisotropy": 0.2, "falloff": 2.5, "noise": {"scale": 0.45, "octaves": 5, "coverage": 0.5, "seed": 3}},
            # Cumulus clouds behind.
            {"type": "volume", "center": [-5, 8, 20], "size": [14, 7, 9], "density": 3.0, "color": [0.97, 0.97, 0.97],
             "anisotropy": 0.6, "noise": {"scale": 2.0, "octaves": 5, "coverage": 0.42, "seed": 11}},
            {"type": "sphere", "center": [-1.2, 0.6, 2.5], "radius": 0.6, "material": plastic([0.85, 0.85, 0.8], 0.4)},
        ],
    }


def main():
    ies_files()
    for name, scene in [("spotlights", spotlights()), ("sun-and-sky", sun_and_sky()), ("smoke", smoke())]:
        path = SCENES / f"{name}.far"
        path.write_text(json.dumps(scene, indent=1) + "\n", encoding="utf-8")
        print(f"{path}: {len(scene['objects'])} objects")


main()

#!/usr/bin/env python3
"""Build assets/scenes/showcase.far: one scene with every technique added to the 2008 program.

Geometry: an infinite ground, analytic solids (cylinder, cone, torus, tube, box) and
displacement, on objects as meshes and on the ground by ray marching.
Materials: GGX metals from polished to rough, glass, a normal map, a height bump map and
color textures (marble, wood).
Light: the studio HDR environment, a warm rectangular light, a small visible bulb, soft
shadows and indirect light (the red wall tints the white sphere).
Lens: a shallow depth of field, focused on the glass sphere.
Render options that the scene cannot turn on: Caustics (the glass focuses light on the
cobbles), Bloom (the bulb glows) and Denoise.
Needs bump-maps.far's and displacement.far's images.
"""
import json
from pathlib import Path

GROUND = -0.5


def surface(color, preset, roughness, **extra):
    return {"color": color, "surface": {"preset": preset, "roughness": roughness}, **extra}


def sphere(x, z, radius, material, lift=0.0):
    return {"type": "sphere", "center": [x, GROUND + radius + lift, z], "radius": radius, "material": material}


objects = [
    # Displacement, traced by ray marching: low cobbles whose joints catch the light.
    {"type": "ground", "position": [0, GROUND, 0],
     "material": surface([0.5, 0.47, 0.42], "ceramic", 0.6, scale_u=0.08, scale_v=0.08,
                         displacement={"map": "cobbles-height.png", "height": 0.018})},
    # Indirect light: a red wall and the white sphere it tints.
    {"type": "box", "center": [-4.1, GROUND + 1.2, 2.7], "size": [0.3, 2.4, 3.2], "rotation": [0, 20, 0],
     "material": surface([0.75, 0.08, 0.06], "plastic", 0.7)},
    sphere(-3.0, 2.1, 0.6, surface([0.85, 0.85, 0.83], "plastic", 0.8)),
    # Normal map: hammered gold.
    sphere(-1.6, 0.9, 0.65, surface([1.0, 0.77, 0.36], "metal", 0.2, scale_u=0.33, scale_v=0.5,
                                    bump={"map": "hammered-normal.png", "kind": "normal", "strength": 1})),
    # Glass on a marble pedestal: refraction, a textured cylinder, and caustics with Caustics on.
    {"type": "cylinder", "center": [0.0, GROUND + 0.15, 0.0], "radius": 0.45, "height": 0.3,
     "material": surface([0.95, 0.93, 0.9], "ceramic", 0.15, texture="marble.tga")},
    {"type": "sphere", "center": [0.0, GROUND + 0.3 + 0.55, 0.0], "radius": 0.55,
     "material": {"color": [0.97, 0.99, 1.0], "surface": {"preset": "glass", "roughness": 0, "ior": 1.5}}},
    # Height bump on a tiled block, with a wooden cone on top (color texture).
    {"type": "box", "center": [0.9, GROUND + 0.4, 3.3], "size": [0.8, 0.8, 0.8], "rotation": [0, 35, 0],
     "material": surface([0.2, 0.42, 0.62], "ceramic", 0.2,
                         bump={"map": "tiles-height.png", "kind": "height", "strength": 0.03})},
    {"type": "cone", "center": [0.9, GROUND + 0.8 + 0.35, 3.3], "radius": 0.32, "height": 0.7,
     "material": surface([0.9, 0.75, 0.55], "plastic", 0.45, texture="wood.tga")},
    # Displacement on an object, kept subtle: its outline is no longer a perfect circle.
    sphere(2.5, 2.5, 0.8, surface([0.62, 0.52, 0.42], "ceramic", 0.55, scale_u=0.5, scale_v=0.5,
                                  displacement={"map": "cobbles-height.png", "height": 0.025, "detail": 192})),
    # Analytic solids: a chrome torus lying in front, slightly out of focus, and a green tube.
    {"type": "torus", "center": [-0.9, GROUND + 0.12, -1.1], "major_radius": 0.38, "minor_radius": 0.12,
     "material": surface([0.92, 0.93, 0.95], "metal", 0.08)},
    {"type": "tube", "center": [3.5, GROUND + 0.45, 1.3], "radius": 0.38, "inner_radius": 0.28, "height": 0.9,
     "material": surface([0.15, 0.55, 0.3], "plastic", 0.3)},
]
# GGX roughness, polished to rough, beside the glass at the same focus.
for i, roughness in enumerate([0.05, 0.2, 0.4, 0.7]):
    objects.append(sphere(1.0 + 0.6 * i, -0.1 + 0.12 * i, 0.26, surface([0.95, 0.64, 0.54], "metal", roughness)))

scene = {
    "camera": {"position": [0.2, 1.2, -4.2], "target": [0.2, -0.05, 1.2], "zoom": 1.05,
               "aperture": 0.07, "focus_distance": 4.4},
    "environment": {"path": "../environments/studio.hdr", "intensity": 0.6, "rotation": 210, "visible": True},
    "lights": [
        # Faces the scene: edge_u x edge_v points toward it.
        {"type": "rectangle", "position": [-5.5, 3.2, -2.5], "edge_u": [0, 1.4, 0], "edge_v": [-0.73, 0, 1.6],
         "color": [1.0, 0.82, 0.6], "intensity": 20},
        # A small bulb, visible in the frame: with Bloom on, it glows.
        {"type": "sphere", "position": [1.45, 0.75, 1.6], "radius": 0.07, "color": [0.45, 0.62, 1.0], "intensity": 40},
    ],
    "objects": objects,
}
out = Path(__file__).resolve().parent.parent / "assets" / "scenes" / "showcase.far"
out.write_text(json.dumps(scene, indent=1) + "\n", encoding="utf-8")
print(f"{out}: {len(objects)} objects")

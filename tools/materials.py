#!/usr/bin/env python3
"""Build the material examples in assets/scenes/materials, and the maps they use.

- glass.far: tinted (absorption depth), frosted (rough) and dispersive glass.
- coatings.far: clear coat over paint, sheen on velvet and anisotropic brushed metal.
- subsurface.far: wax, jade and skin, lit from behind so light shows through them.
- maps.far: roughness, metallic and emission maps, and an alpha cutout.
Each scene shares one studio: a matte floor, the studio HDR and a soft key light.
"""
import json
import math
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
IMAGES = ROOT / "assets" / "scene-data" / "images"
SCENES = ROOT / "assets" / "scenes" / "materials"


def write_png(name, size, pixel):
    rows = bytearray()
    for y in range(size):
        rows.append(0)
        for x in range(size):
            rows.extend(max(0, min(255, round(c * 255))) for c in pixel(x, y))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    path = IMAGES / name
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b""))
    print(f"{path}: {path.stat().st_size} bytes")


def maps():
    # Roughness: polished and rough squares. White keeps the roughness; black is a mirror.
    write_png("checker-roughness.png", 256, lambda x, y: (1.0 if (x // 64 + y // 64) % 2 else 0.05,) * 3)
    # Metallic: metal stripes on a painted surface.
    write_png("stripes-metallic.png", 256, lambda x, y: (1.0 if (x // 32) % 2 == 0 else 0.0,) * 3)

    # Alpha: a grille of round holes. Dark is cut away.
    def grille(x, y):
        cx, cy = x % 32 - 15.5, y % 32 - 15.5
        return (0.0 if cx * cx + cy * cy < 11 * 11 else 1.0,) * 3
    write_png("grille-alpha.png", 256, grille)

    # Emission: glowing bands that fade at their edges.
    def bands(x, y):
        d = abs((y % 64) - 31.5) / 31.5
        glow = max(0.0, 1.0 - d * 4) ** 2
        return (glow,) * 3
    write_png("bands-emission.png", 256, bands)


def surface(color, preset, **values):
    return {"color": color, "surface": {"preset": preset, **values}}


def studio(objects, lights=None, camera=None):
    """A scene in the shared studio, with these objects resting on the floor."""
    floor = {"type": "ground", "position": [0, -0.5, 0],
             "material": surface([0.72, 0.72, 0.7], "plastic", roughness=0.6)}
    return {
        "camera": camera or {"position": [0, 1.15, -5.0], "target": [0, 0.2, 0.6], "zoom": 1.15},
        "environment": {"path": "../../environments/studio.hdr", "intensity": 0.55, "rotation": 160, "visible": True},
        "lights": lights or [{"type": "rectangle", "position": [-4.2, 3.8, -2.4], "edge_u": [0, 1.2, 0],
                              "edge_v": [-0.55, 0, 1.2], "color": [1, 0.94, 0.86], "intensity": 22}],
        "objects": [floor] + objects,
    }


def on_floor(kind, x, z, size, material, **extra):
    """A sphere of radius `size`, or another object, resting on the floor at x, z."""
    if kind == "sphere":
        return {"type": "sphere", "center": [x, -0.5 + size, z], "radius": size, "material": material}
    return {"type": kind, "material": material, **extra}


def glass():
    return studio([
        # Tinted by depth: thin edges stay pale, the thick middle turns deep green.
        on_floor("sphere", -1.7, 0.6, 0.7, surface([0.25, 0.75, 0.4], "glass", absorption_depth=0.5)),
        # Frosted: rough glass blurs what is behind it.
        on_floor("sphere", 0.0, 0.6, 0.7, surface([1, 1, 1], "glass", roughness=0.18)),
        # Dispersive: edges and caustics split into a spectrum. A low Abbe number is a strong
        # spread, far beyond real glass, to make it easy to see.
        {"type": "box", "center": [1.7, 0.2, 0.6], "size": [0.9, 1.4, 0.9], "rotation": [0, 45, 0],
         "material": surface([1, 1, 1], "glass", ior=1.6, abbe=8)},
    ])


def coatings():
    return studio([
        # Car paint: a clear coat over a red base, sharp reflections over a soft color.
        on_floor("sphere", -1.8, 0.7, 0.6, surface([0.7, 0.05, 0.05], "plastic", roughness=0.45,
                                                   clearcoat=1.0, clearcoat_roughness=0.02)),
        # Velvet: sheen lights the edges.
        on_floor("sphere", -0.6, 0.7, 0.6, surface([0.25, 0.04, 0.3], "plastic", roughness=1.0, sheen=1.0,
                                                   sheen_color=[0.9, 0.7, 1.0])),
        # Brushed metal: highlights stretch around the sphere, following its texture direction.
        on_floor("sphere", 0.6, 0.7, 0.6, surface([0.92, 0.92, 0.94], "metal", roughness=0.5, anisotropy=1.0)),
        # A brushed disc, like the bottom of a pan.
        {"type": "cylinder", "center": [1.85, -0.1, 0.7], "radius": 0.6, "height": 0.8, "rotation": [0, 0, 0],
         "material": surface([0.95, 0.8, 0.6], "metal", roughness=0.5, anisotropy=1.0, anisotropy_rotation=90)},
    ])


def subsurface():
    # Light from behind, so it glows through the thin parts.
    # Above the top of the frame, facing down toward the objects and the camera.
    lights = [{"type": "rectangle", "position": [-1.4, 5.6, 3.5], "edge_u": [2.8, 0, 0], "edge_v": [0, -0.5, 0.95],
               "color": [1, 0.9, 0.8], "intensity": 22},
              {"type": "rectangle", "position": [-4.2, 3.8, -2.4], "edge_u": [0, 1.2, 0],
               "edge_v": [-0.55, 0, 1.2], "color": [1, 0.94, 0.86], "intensity": 6}]
    return studio([
        # Candle wax.
        {"type": "cylinder", "center": [-1.6, 0.25, 0.7], "radius": 0.45, "height": 1.5,
         "material": surface([0.95, 0.88, 0.72], "plastic", roughness=0.4, subsurface=1.0, subsurface_radius=0.6)},
        # Jade.
        on_floor("sphere", 0.0, 0.7, 0.6, surface([0.35, 0.8, 0.5], "ceramic", roughness=0.1, subsurface=1.0,
                                                  subsurface_radius=0.4)),
        # Skin-like: red light travels furthest.
        on_floor("sphere", 1.5, 0.7, 0.55, surface([0.85, 0.6, 0.5], "plastic", roughness=0.5, subsurface=0.9,
                                                   subsurface_radius=0.3)),
        # The same jade without subsurface, for comparison.
        on_floor("sphere", 2.7, 1.2, 0.35, surface([0.35, 0.8, 0.5], "ceramic", roughness=0.1)),
    ], lights=lights)


def maps_scene():
    gold = [1.0, 0.8, 0.45]
    return studio([
        # Roughness map: polished and rough squares on one metal.
        on_floor("sphere", -1.7, 0.7, 0.65, {**surface([0.95, 0.95, 0.97], "metal", roughness=0.5),
                                              "roughness_map": "checker-roughness.png", "scale_u": 0.5, "scale_v": 0.5}),
        # Metallic map: gold stripes on a red plastic box.
        {"type": "box", "center": [-0.3, 0.05, 0.9], "size": [1.0, 1.1, 1.0], "rotation": [0, 30, 0],
         "material": {**surface(gold, "metal", roughness=0.25), "metallic_map": "stripes-metallic.png"}},
        # Emission: a torus with glowing bands.
        {"type": "torus", "center": [1.2, -0.15, 0.4], "major_radius": 0.55, "minor_radius": 0.2,
         "rotation": [70, 0, 0], "material": {**surface([0.1, 0.1, 0.12], "plastic", roughness=0.4),
                                              "emission": {"color": [1, 0.6, 0.3], "strength": 6,
                                                           "map": "bands-emission.png"}}},
        # Alpha cutout: a grille with round holes, casting a dotted shadow.
        {"type": "box", "center": [2.6, 0.45, 1.3], "size": [1.2, 1.9, 0.04], "rotation": [0, -30, 0],
         "material": {**surface([0.15, 0.4, 0.75], "plastic", roughness=0.3), "alpha_map": "grille-alpha.png"}},
    ], camera={"position": [0.4, 1.2, -5.0], "target": [0.4, 0.2, 0.7], "zoom": 1.1})


def main():
    maps()
    SCENES.mkdir(exist_ok=True)
    for name, scene in [("glass", glass()), ("coatings", coatings()), ("subsurface", subsurface()),
                        ("maps", maps_scene())]:
        path = SCENES / f"{name}.far"
        path.write_text(json.dumps(scene, indent=1) + "\n", encoding="utf-8")
        print(f"{path}: {len(scene['objects'])} objects")


main()

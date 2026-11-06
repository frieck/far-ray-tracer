#!/usr/bin/env python3
"""Creates Far Ray Tracer .frt scenes without the original program.

As a library:

    from frt_gen import sphere, plane, mesh, save

    save("assets/scenes/mine.frt",
        spheres=[
            sphere((0, 6, 2), 0.1, light=True, name="Light"),
            sphere((0, 0, 5), 1.5, color=(0.9, 0.2, 0.2), refl=0.3),
        ],
        planes=[plane((-8, -2, 0), (8, -2, 0), (8, -2, 16), (-8, -2, 16), refl=0.2)],
    )

To create the example scene:

    python3 tools/frt_gen.py assets/scenes/example.frt

Scene rules:
  - The camera is fixed at (0, 0, -5) and looks toward +z.
  - Only spheres with light=True emit light. Without one the image is black.
  - With the KD-Tree, a scene must fit in x -14..14, y -6..8 and z -6..30.
  - The order of a plane's points defines its lit side.
  - Textures and meshes are given by file name only and live in
    scene-data/images and scene-data/meshes. Meshes must be .3ds files.

Material properties accepted by sphere(), plane() and mesh():
  color (r, g, b from 0 to 1), refl (reflection), diff (diffuse), spec (specular),
  refr (refraction), ior (index of refraction), drefl (diffuse reflection),
  texture (file name), scale_u and scale_v (texture scale).
"""

import struct
import sys


def _text(value, size):
    data = value.encode("latin-1")
    if len(data) >= size:
        raise ValueError(f"text too long (maximum {size - 1} bytes): {value!r}")
    return data.ljust(size, b"\0")


def material(color=(1, 1, 1), refl=0, diff=1, spec=1, refr=0, ior=1.5, drefl=0,
             texture="", scale_u=1, scale_v=1):
    numbers = struct.pack("<13f", *color, refl, diff, spec, refr, ior, drefl,
                          scale_u, scale_v, 1 / scale_u, 1 / scale_v)
    return numbers + _text(texture, 1000)


def sphere(center, radius, light=False, name="Sphere", **mat):
    return struct.pack("<4f?", radius, *center, light) + _text(name, 200) + b"\0" * 3 + material(**mat)


def plane(p1, p2, p3, p4, name="Plane", **mat):
    return struct.pack("<12f?", *p1, *p2, *p3, *p4, False) + _text(name, 200) + b"\0" * 3 + material(**mat)


def mesh(file, pos, size, name="Mesh", **mat):
    return (_text(file, 1000) + _text(name, 200)
            + struct.pack("<4f?", *pos, size, False) + b"\0" * 3 + material(**mat))


def save(path, spheres=(), planes=(), meshes=()):
    with open(path, "wb") as output:
        output.write(struct.pack("<3i", len(spheres), len(planes), len(meshes)))
        for item in (*spheres, *planes, *meshes):
            output.write(item)


def example(path):
    save(path,
        spheres=[
            sphere((0, 6, 2), 0.1, light=True, name="Light"),
            sphere((-1.5, -0.5, 5), 1.5, name="Red", color=(0.9, 0.2, 0.2), refl=0.3),
            sphere((1.8, -0.8, 4), 1.2, name="Blue", color=(0.2, 0.5, 0.9), refl=0.6),
        ],
        planes=[plane((-8, -2, 0), (8, -2, 0), (8, -2, 16), (-8, -2, 16),
                      name="Floor", color=(0.8, 0.8, 0.8), refl=0.2)],
    )


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: frt_gen.py output.frt")
    example(sys.argv[1])
    print(f"example scene saved to {sys.argv[1]}")

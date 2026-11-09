#!/usr/bin/env python3
"""Build the example models in assets/scene-data/meshes and the scene that shows them.

- vase.obj with vase.mtl and vase-stripes.png: a turned vase with texture coordinates and
  smooth normals, glazed ceramic with a striped color map.
- table.glb: a small glTF table. One leg mesh is placed four times by nodes under a moved and
  turned parent, the top has a checker texture stored inside the file, and a glass bowl
  (KHR_materials_transmission) and a glowing bulb (emission) stand on it.
- assets/scenes/techniques/models.far: both models in a studio, placed with position,
  rotation and size.
"""
import json
import math
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MESHES = ROOT / "assets" / "scene-data" / "meshes"
SCENES = ROOT / "assets" / "scenes" / "techniques"


def png_bytes(size, pixel):
    rows = bytearray()
    for y in range(size):
        rows.append(0)
        for x in range(size):
            rows.extend(max(0, min(255, round(c * 255))) for c in pixel(x, y))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b""))


def lathe(profile, segments):
    """A surface of revolution about Y: vertices, normals, texture coordinates and quads."""
    points, normals, uvs = [], [], []
    lengths = [0.0]
    for (r0, y0), (r1, y1) in zip(profile, profile[1:]):
        lengths.append(lengths[-1] + math.hypot(r1 - r0, y1 - y0))
    for i, (r, y) in enumerate(profile):
        # The profile's slope gives the normal's tilt.
        a, b = profile[max(i - 1, 0)], profile[min(i + 1, len(profile) - 1)]
        dr, dy = b[0] - a[0], b[1] - a[1]
        nr, ny = dy, -dr
        length = math.hypot(nr, ny) or 1.0
        nr, ny = nr / length, ny / length
        for j in range(segments + 1):
            angle = 2 * math.pi * j / segments
            c, s = math.cos(angle), math.sin(angle)
            points.append((r * c, y, r * s))
            normals.append((nr * c, ny, nr * s))
            uvs.append((j / segments, lengths[i] / lengths[-1]))
    quads = []
    row = segments + 1
    for i in range(len(profile) - 1):
        for j in range(segments):
            a = i * row + j
            quads.append((a, a + row, a + row + 1, a + 1))
    return points, normals, uvs, quads


def vase():
    profile = [(0.001, 0.0), (0.32, 0.0), (0.42, 0.12), (0.5, 0.35), (0.48, 0.6), (0.34, 0.85), (0.22, 1.02),
               (0.2, 1.15), (0.27, 1.3), (0.25, 1.32), (0.17, 1.17), (0.18, 1.0)]
    points, normals, uvs, quads = lathe(profile, 48)
    lines = ["# A turned vase, made by tools/models.py", "mtllib vase.mtl", "o vase"]
    lines += [f"v {x:.5f} {y:.5f} {z:.5f}" for x, y, z in points]
    lines += [f"vt {u:.5f} {v:.5f}" for u, v in uvs]
    lines += [f"vn {x:.5f} {y:.5f} {z:.5f}" for x, y, z in normals]
    lines.append("usemtl glaze")
    # OBJ counts from 1; faces wind counterclockwise seen from outside.
    lines += ["f " + " ".join(f"{i + 1}/{i + 1}/{i + 1}" for i in reversed(q)) for q in quads]
    (MESHES / "vase.obj").write_text("\n".join(lines) + "\n")
    (MESHES / "vase.mtl").write_text("\n".join([
        "# Glazed ceramic with painted stripes",
        "newmtl glaze",
        "Kd 1 1 1",
        "Ks 0.5 0.5 0.5",
        "Ns 400",
        "map_Kd vase-stripes.png",
    ]) + "\n")

    def stripes(x, y):
        band = (y // 16) % 4
        wave = math.sin(x / 128 * 2 * math.pi * 3 + y / 20) > 0.6
        base = [(0.12, 0.25, 0.55), (0.92, 0.88, 0.78), (0.75, 0.35, 0.15), (0.92, 0.88, 0.78)][band]
        return (0.95, 0.9, 0.7) if wave and band % 2 == 0 else base
    (MESHES / "vase-stripes.png").write_bytes(png_bytes(128, stripes))
    print(f"vase.obj: {len(points)} vertices, {len(quads)} quads")


class Glb:
    """Collects glTF buffers, accessors and meshes into one binary file."""

    def __init__(self):
        self.binary = bytearray()
        self.gltf = {"asset": {"version": "2.0", "generator": "Far Ray Tracer tools/models.py"},
                     "buffers": [], "bufferViews": [], "accessors": [], "meshes": [], "nodes": [],
                     "materials": [], "images": [], "textures": [], "samplers": [{}],
                     "extensionsUsed": ["KHR_materials_transmission", "KHR_materials_ior", "KHR_materials_emissive_strength"]}

    def view(self, data, target=None):
        while len(self.binary) % 4:
            self.binary.append(0)
        view = {"buffer": 0, "byteOffset": len(self.binary), "byteLength": len(data)}
        if target:
            view["target"] = target
        self.binary.extend(data)
        self.gltf["bufferViews"].append(view)
        return len(self.gltf["bufferViews"]) - 1

    def accessor(self, values, kind, component=5126, target=34962):
        flat = [c for v in values for c in (v if isinstance(v, tuple) else (v,))]
        fmt = "<" + ("f" if component == 5126 else "I") * len(flat)
        accessor = {"bufferView": self.view(struct.pack(fmt, *flat), target), "componentType": component,
                    "count": len(values), "type": kind}
        if kind == "VEC3" and component == 5126:
            accessor["min"] = [min(v[i] for v in values) for i in range(3)]
            accessor["max"] = [max(v[i] for v in values) for i in range(3)]
        self.gltf["accessors"].append(accessor)
        return len(self.gltf["accessors"]) - 1

    def mesh(self, name, points, normals, uvs, triangles, material):
        attributes = {"POSITION": self.accessor(points, "VEC3"), "NORMAL": self.accessor(normals, "VEC3")}
        if uvs:
            attributes["TEXCOORD_0"] = self.accessor(uvs, "VEC2")
        indices = self.accessor([i for t in triangles for i in t], "SCALAR", 5125, 34963)
        self.gltf["meshes"].append({"name": name, "primitives": [
            {"attributes": attributes, "indices": indices, "material": material}]})
        return len(self.gltf["meshes"]) - 1

    def material(self, spec):
        self.gltf["materials"].append(spec)
        return len(self.gltf["materials"]) - 1

    def node(self, spec):
        self.gltf["nodes"].append(spec)
        return len(self.gltf["nodes"]) - 1

    def image(self, png):
        self.gltf["images"].append({"bufferView": self.view(png), "mimeType": "image/png"})
        self.gltf["textures"].append({"source": len(self.gltf["images"]) - 1, "sampler": 0})
        return len(self.gltf["textures"]) - 1

    def write(self, path):
        self.gltf["buffers"] = [{"byteLength": len(self.binary)}]
        text = json.dumps(self.gltf, separators=(",", ":")).encode()
        text += b" " * (-len(text) % 4)
        binary = bytes(self.binary) + b"\0" * (-len(self.binary) % 4)
        body = (struct.pack("<II", len(text), 0x4E4F534A) + text + struct.pack("<II", len(binary), 0x004E4942) + binary)
        path.write_bytes(struct.pack("<III", 0x46546C67, 2, 12 + len(body)) + body)


def box(sx, sy, sz):
    """A box from (0, 0, 0)-centered sizes, with a normal and texture coordinates per face."""
    points, normals, uvs, triangles = [], [], [], []
    for axis in range(3):
        for sign in (-1, 1):
            n = [0, 0, 0]
            n[axis] = sign
            u_axis, v_axis = [(1, 2), (2, 0), (0, 1)][axis]
            base = len(points)
            for a, b in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                p = [0.0, 0.0, 0.0]
                p[axis] = sign
                p[u_axis], p[v_axis] = a, b
                points.append((p[0] * sx / 2, p[1] * sy / 2, p[2] * sz / 2))
                normals.append(tuple(float(c) for c in n))
                uvs.append(((a + 1) / 2, (1 - b) / 2))
            quad = (base, base + 1, base + 2, base + 3) if sign > 0 else (base, base + 3, base + 2, base + 1)
            triangles += [(quad[0], quad[1], quad[2]), (quad[0], quad[2], quad[3])]
    return points, normals, uvs, triangles


def sphere(radius, rings=24, segments=48, half=False):
    profile = []
    end = math.pi / 2 if half else math.pi
    for i in range(rings + 1):
        a = math.pi - end * i / rings
        profile.append((radius * math.sin(a) + 1e-4, radius * math.cos(a)))
    points, normals, uvs, quads = lathe(profile, segments)
    normals = [tuple(c / radius for c in p) for p in points]
    triangles = [t for q in quads for t in ((q[0], q[3], q[2]), (q[0], q[2], q[1]))]
    return points, normals, uvs, triangles


def table():
    glb = Glb()
    checker = glb.image(png_bytes(64, lambda x, y: (0.55, 0.36, 0.2) if (x // 8 + y // 8) % 2 else (0.75, 0.55, 0.33)))
    wood = glb.material({"name": "checker wood", "pbrMetallicRoughness": {
        "baseColorTexture": {"index": checker}, "metallicFactor": 0.0, "roughnessFactor": 0.45}})
    steel = glb.material({"name": "steel", "pbrMetallicRoughness": {
        "baseColorFactor": [0.8, 0.8, 0.82, 1], "metallicFactor": 1.0, "roughnessFactor": 0.3}})
    glass = glb.material({"name": "glass", "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1],
        "metallicFactor": 0.0, "roughnessFactor": 0.0}, "extensions": {
        "KHR_materials_transmission": {"transmissionFactor": 1.0}, "KHR_materials_ior": {"ior": 1.5}}})
    glow = glb.material({"name": "bulb", "pbrMetallicRoughness": {"baseColorFactor": [1, 0.9, 0.7, 1]},
        "emissiveFactor": [1.0, 0.75, 0.4], "extensions": {"KHR_materials_emissive_strength": {"emissiveStrength": 4}}})

    top = glb.mesh("top", *box(1.6, 0.08, 1.0), wood)
    leg = glb.mesh("leg", *box(0.07, 0.7, 0.07), steel)
    bowl = glb.mesh("bowl", *sphere(0.22, half=True), glass)
    bulb = glb.mesh("bulb", *sphere(0.09, 12, 24), glow)
    children = [glb.node({"name": "top", "mesh": top, "translation": [0, 0.74, 0]})]
    for x in (-0.72, 0.72):
        for z in (-0.42, 0.42):
            children.append(glb.node({"name": "leg", "mesh": leg, "translation": [x, 0.35, z]}))
    children.append(glb.node({"name": "bowl", "mesh": bowl, "translation": [-0.35, 1.0, 0.05]}))
    children.append(glb.node({"name": "bulb", "mesh": bulb, "translation": [0.45, 0.87, -0.1]}))
    # The parent turns the table a quarter turn about Y; the scene can turn it back.
    root = glb.node({"name": "table", "children": children, "rotation": [0, math.sin(math.pi / 4), 0, math.cos(math.pi / 4)]})
    glb.gltf["scenes"] = [{"nodes": [root]}]
    glb.gltf["scene"] = 0
    glb.write(MESHES / "table.glb")
    print(f"table.glb: {(MESHES / 'table.glb').stat().st_size} bytes")


def scene():
    data = {
        "camera": {"position": [0.5, 2.0, -2.9], "target": [-0.1, 0.5, 0.35], "zoom": 1.45},
        "environment": {"path": "../../environments/studio.hdr", "intensity": 0.45, "rotation": 150, "visible": False},
        "lights": [{"type": "rectangle", "position": [-2.6, 3.0, -1.6], "edge_u": [0, 0, 1.2], "edge_v": [-0.7, -0.6, 0],
                    "color": [1, 0.94, 0.86], "intensity": 14}],
        "objects": [
            {"type": "ground", "position": [0, 0, 0],
             "material": {"color": [0.62, 0.62, 0.6], "surface": {"preset": "plastic", "roughness": 0.6}}},
            {"type": "model", "file": "table.glb", "position": [0.25, 0, 0.6], "rotation": [0, 70, 0]},
            {"type": "model", "file": "vase.obj", "position": [-0.95, 0, 0.0], "size": 0.75},
        ],
    }
    path = SCENES / "models.far"
    path.write_text(json.dumps(data, indent=1) + "\n")
    print(path)


vase()
table()
scene()

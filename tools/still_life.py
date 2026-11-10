#!/usr/bin/env python3
"""Build assets/scenes/still-life.far: a still life on a wooden table by a window, late in the afternoon.

Everything the scene uses is made here, with no downloads:

- Turned (lathe) models written as glTF binaries in assets/scene-data/meshes: a wine glass and a
  decanter, each with its wine as a separate solid; a glazed bowl; a copper cup with a swept
  handle; a brass candlestick; a pear. The oak table top is a rounded profile extruded along X,
  and the linen towel is a cloth grid that lies on the table and falls over its front edge in
  folds.
- Images in assets/scene-data/images: the oak planks (color and height), the linen (color with
  woven stripes, and the weave as a height map) and the pear's skin.
- The scene: a dark room whose only window lets in a low sun through haze, so the glass and
  the wine glow against the light and the beams show; the sky fills the shadows with cool
  light and a candle adds a warm one.

Scale: one scene unit is 10 cm. The table top is at height 0.
"""
import json
import math
import random
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MESHES = ROOT / "assets" / "scene-data" / "meshes"
IMAGES = ROOT / "assets" / "scene-data" / "images"
SCENE = ROOT / "assets" / "scenes" / "still-life.far"
PREFIX = "still-life-"


# ---------------------------------------------------------------- images

def write_png(name, width, height, rows, channels):
    """Writes 8-bit gray (1 channel) or RGB (3) rows of floats in 0..1."""
    raw = bytearray()
    for row in rows:
        raw.append(0)
        raw.extend(max(0, min(255, round(c * 255))) for c in row)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    color_type = 0 if channels == 1 else 2
    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, color_type, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))
    (IMAGES / (PREFIX + name)).write_bytes(png)
    return PREFIX + name


def noise_field(width, height, cells_x, cells_y, octaves, seed):
    """Fractal value noise that tiles across the image, as a list of rows of values around 0..1."""
    rng = random.Random(seed)
    field = [[0.0] * width for _ in range(height)]
    amplitude, total = 1.0, 0.0
    for octave in range(octaves):
        cx, cy = cells_x << octave, cells_y << octave
        lattice = [[rng.random() for _ in range(cx)] for _ in range(cy)]
        # Smoothstep weights for each column and row, computed once per octave.
        columns = []
        for x in range(width):
            fx = x * cx / width
            ix = int(fx)
            t = fx - ix
            columns.append((ix % cx, (ix + 1) % cx, t * t * (3 - 2 * t)))
        for y in range(height):
            fy = y * cy / height
            iy = int(fy)
            t = fy - iy
            s = t * t * (3 - 2 * t)
            row0, row1 = lattice[iy % cy], lattice[(iy + 1) % cy]
            out = field[y]
            for x, (x0, x1, w) in enumerate(columns):
                a = row0[x0] + (row0[x1] - row0[x0]) * w
                b = row1[x0] + (row1[x1] - row1[x0]) * w
                out[x] += amplitude * (a + (b - a) * s)
        total += amplitude
        amplitude *= 0.5
    return [[v / total for v in row] for row in field]


def smoothstep(a, b, x):
    t = max(0.0, min(1.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


def mix(a, b, t):
    return tuple(p + (q - p) * t for p, q in zip(a, b))


# The wood image spans 12 units along the grain (X) and 6 across: four planks of 15 cm.
WOOD_U, WOOD_V, PLANKS = 12.0, 6.0, 4


def wood_images():
    width, height = 1024, 512
    plank_rows = height // PLANKS
    warp = noise_field(width, height, 3, 8, 4, 11)
    figure = noise_field(width, height, 12, 64, 3, 12)
    pores = noise_field(width, height, 64, 256, 1, 13)
    rng = random.Random(14)
    tints = [(0.85 + 0.3 * rng.random(), rng.random() * 9) for _ in range(PLANKS)]
    light, dark = (0.30, 0.17, 0.085), (0.13, 0.065, 0.03)
    color_rows, height_rows = [], []
    for y in range(height):
        plank = y // plank_rows
        local = (y % plank_rows + 0.5) / plank_rows
        tint, offset = tints[plank]
        color_row, height_row = [], []
        edge = min(local, 1 - local) * plank_rows  # pixels from the nearest joint
        for x in range(width):
            # Growth rings cut along the board: stripes along X, bent by slow noise.
            rings = local * 5.5 + offset + 2.2 * warp[y][x]
            ring = rings - math.floor(rings)
            late = smoothstep(0.55, 0.92, ring) * (1 - smoothstep(0.95, 1.0, ring))
            grain = 0.55 * late + 0.25 * figure[y][x] + 0.2 * smoothstep(0.55, 0.9, pores[y][x])
            r, g, b = mix(light, dark, min(1.0, grain))
            joint = smoothstep(0.5, 2.5, edge)
            shade = tint * (0.45 + 0.55 * joint)
            color_row += (r * shade, g * shade, b * shade)
            height_row.append(joint * (0.75 + 0.15 * (1 - late) + 0.1 * (1 - pores[y][x])))
        color_rows.append(color_row)
        height_rows.append(height_row)
    return (write_png("oak.png", width, height, color_rows, 3),
            write_png("oak-height.png", width, height, height_rows, 1))


# The towel: CLOTH_W wide and CLOTH_L long; its image covers it once.
CLOTH_W, CLOTH_L = 4.4, 6.2


def linen_images():
    width, height = 512, 640
    rng = random.Random(21)
    slub = [0.92 + 0.16 * rng.random() for _ in range(width)]
    weft = [0.92 + 0.16 * rng.random() for _ in range(height)]
    natural, blue = (0.62, 0.57, 0.48), (0.06, 0.12, 0.3)
    color_rows = []
    for y in range(height):
        s = (y + 0.5) / height * CLOTH_L  # distance from the towel's far end, in units
        from_hem = CLOTH_L - s
        # Two woven blue stripes near the hanging hem, as on a kitchen towel.
        stripe = 0.0
        for start, width_units in ((0.45, 0.22), (0.8, 0.08)):
            stripe = max(stripe, smoothstep(start - 0.01, start + 0.01, from_hem)
                         * (1 - smoothstep(start + width_units - 0.01, start + width_units + 0.01, from_hem)))
        row = []
        for x in range(width):
            v = slub[x] * weft[y]
            # The stripes are woven, so the natural warp still shows through on alternate threads.
            t = stripe * (0.85 if (x + y) % 2 else 1.0)
            r, g, b = mix(natural, blue, t)
            row += (r * v, g * v, b * v)
        color_rows.append(row)

    width, height = 1024, 1280
    thread = 4
    rng = random.Random(22)
    warp_thick = [0.8 + 0.4 * rng.random() for _ in range(width // thread + 1)]
    weft_thick = [0.8 + 0.4 * rng.random() for _ in range(height // thread + 1)]
    profile = [math.sin(math.pi * (i + 0.5) / thread) for i in range(thread)]
    height_rows = []
    for y in range(height):
        iy, cy = divmod(y, thread)
        row = []
        for x in range(width):
            ix, cx = divmod(x, thread)
            if (ix + iy) % 2:
                h = 0.5 + 0.5 * profile[cx] * warp_thick[ix]
            else:
                h = 0.5 + 0.5 * profile[cy] * weft_thick[iy]
            row.append(min(1.0, h * 0.95))
        height_rows.append(row)
    return (write_png("linen.png", 512, 640, color_rows, 3),
            write_png("linen-weave.png", width, height, height_rows, 1))


def pear_image():
    width, height = 256, 128
    speckle = noise_field(width, height, 32, 16, 2, 31)
    blotch = noise_field(width, height, 4, 2, 3, 32)
    rows = []
    for y in range(height):
        v = (y + 0.5) / height  # 0 at the bottom, 1 at the stem
        row = []
        for x in range(width):
            u = (x + 0.5) / width
            base = mix((0.42, 0.42, 0.06), (0.36, 0.3, 0.03), blotch[y][x])
            # A red blush on one side, stronger toward the shoulder.
            blush = max(0.0, math.cos(2 * math.pi * (u - 0.1))) ** 2 * (0.4 + 0.5 * smoothstep(0.1, 0.6, v))
            color = mix(base, (0.38, 0.07, 0.03), 0.75 * blush * (0.6 + 0.4 * blotch[y][x]))
            # Russet near the stem and small lenticels.
            color = mix(color, (0.2, 0.11, 0.04), smoothstep(0.82, 0.95, v) * 0.7)
            if speckle[y][x] > 0.72:
                color = mix(color, (0.25, 0.16, 0.06), 0.6)
            row += color
        rows.append(row)
    return write_png("pear.png", width, height, rows, 3)


# ---------------------------------------------------------------- meshes

class Mesh:
    def __init__(self):
        self.points, self.normals, self.uvs, self.triangles = [], [], [], []

    def add_grid(self, points, normals, uvs, rows, columns, wrap=False):
        """Adds a grid of rows x columns vertices, with triangles wound to face the normals."""
        base = len(self.points)
        self.points += points
        self.normals += normals
        self.uvs += uvs
        for i in range(rows - 1):
            for j in range(columns - 1):
                a = base + i * columns + j
                b, c, d = a + columns, a + columns + 1, a + 1
                for tri in ((a, b, c), (a, c, d)):
                    self.add_triangle(tri)

    def add_triangle(self, tri):
        p = [self.points[i] for i in tri]
        e1 = [p[1][k] - p[0][k] for k in range(3)]
        e2 = [p[2][k] - p[0][k] for k in range(3)]
        n = (e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0])
        if n == (0, 0, 0) or max(abs(c) for c in n) < 1e-12:
            return
        shading = [sum(self.normals[i][k] for i in tri) for k in range(3)]
        if sum(n[k] * shading[k] for k in range(3)) < 0:
            tri = (tri[0], tri[2], tri[1])
        self.triangles.append(tri)

    def write_glb(self, name):
        def pack(fmt, values):
            return struct.pack("<" + fmt * len(values), *values)

        flat_points = [c for p in self.points for c in p]
        flat_normals = [c for n in self.normals for c in n]
        flat_uvs = [c for uv in self.uvs for c in uv]
        flat_indices = [i for t in self.triangles for i in t]
        blobs = [pack("f", flat_points), pack("f", flat_normals), pack("f", flat_uvs), pack("I", flat_indices)]
        binary, views = bytearray(), []
        for blob, target in zip(blobs, (34962, 34962, 34962, 34963)):
            views.append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(blob), "target": target})
            binary += blob
        count = len(self.points)
        gltf = {
            "asset": {"version": "2.0", "generator": "Far Ray Tracer tools/still_life.py"},
            "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0, "name": name}],
            "meshes": [{"name": name, "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
                                                      "indices": 3}]}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": count, "type": "VEC3",
                 "min": [min(p[k] for p in self.points) for k in range(3)],
                 "max": [max(p[k] for p in self.points) for k in range(3)]},
                {"bufferView": 1, "componentType": 5126, "count": count, "type": "VEC3"},
                {"bufferView": 2, "componentType": 5126, "count": count, "type": "VEC2"},
                {"bufferView": 3, "componentType": 5125, "count": len(flat_indices), "type": "SCALAR"},
            ],
            "bufferViews": views,
            "buffers": [{"byteLength": len(binary)}],
        }
        text = json.dumps(gltf, separators=(",", ":")).encode()
        text += b" " * (-len(text) % 4)
        binary += b"\0" * (-len(binary) % 4)
        data = (struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(text) + 8 + len(binary))
                + struct.pack("<II", len(text), 0x4E4F534A) + text
                + struct.pack("<II", len(binary), 0x004E4942) + bytes(binary))
        file = PREFIX + name + ".glb"
        (MESHES / file).write_bytes(data)
        print(f"{file}: {count} vertices, {len(self.triangles)} triangles, {len(data) // 1024} KB")
        return file


def catmull_rom(points, step=0.002):
    """A dense polyline through the points."""
    padded = [points[0]] + list(points) + [points[-1]]
    out = []
    for i in range(1, len(padded) - 2):
        p0, p1, p2, p3 = padded[i - 1:i + 3]
        n = max(2, int(math.dist(p1, p2) / step))
        for k in range(n):
            t = k / n
            out.append(tuple(0.5 * (2 * b + (-a + c) * t + (2 * a - 5 * b + 4 * c - d) * t * t
                                    + (-a + 3 * b - 3 * c + d) * t ** 3) for a, b, c, d in zip(p0, p1, p2, p3)))
    out.append(tuple(points[-1]))
    return out


def arc(center, radius, start, end, steps=12):
    """Points on a circle in the profile plane, angles in degrees."""
    return [(center[0] + radius * math.cos(math.radians(start + (end - start) * k / steps)),
             center[1] + radius * math.sin(math.radians(start + (end - start) * k / steps))) for k in range(steps + 1)]


def simplify(dense, step, angle=10.0):
    """Keeps a point every `step` units, or sooner where the line turns more than `angle` degrees."""
    out = [dense[0]]
    length, turn = 0.0, 0.0
    for i in range(1, len(dense) - 1):
        length += math.dist(dense[i - 1], dense[i])
        a = math.atan2(dense[i][1] - dense[i - 1][1], dense[i][0] - dense[i - 1][0])
        b = math.atan2(dense[i + 1][1] - dense[i][1], dense[i + 1][0] - dense[i][0])
        turn += abs((b - a + math.pi) % (2 * math.pi) - math.pi)
        if length >= step or math.degrees(turn) >= angle:
            out.append(dense[i])
            length, turn = 0.0, 0.0
    out.append(dense[-1])
    return out


def profile_normals(profile):
    """Normals in the (r, y) plane: to the right of the direction of travel."""
    normals = []
    for i in range(len(profile)):
        a, b = profile[max(i - 1, 0)], profile[min(i + 1, len(profile) - 1)]
        dr, dy = b[0] - a[0], b[1] - a[1]
        length = math.hypot(dr, dy) or 1.0
        normals.append((dy / length, -dr / length))
    return normals


def offset(dense, distance):
    """The polyline moved `distance` to the left of its travel, that is, inward."""
    return [(p[0] - distance * n[0], p[1] - distance * n[1]) for p, n in zip(dense, profile_normals(dense))]


def lathe(mesh, profile, segments=96, bend=None):
    """Turns a profile (r, y), travelling counterclockwise around the solid, about the Y axis."""
    normals = profile_normals(profile)
    lengths = [0.0]
    for a, b in zip(profile, profile[1:]):
        lengths.append(lengths[-1] + math.dist(a, b))
    points, vertex_normals, uvs = [], [], []
    for (r, y), (nr, ny), length in zip(profile, normals, lengths):
        r = max(r, 0.0)
        for j in range(segments + 1):
            angle = 2 * math.pi * j / segments
            c, s = math.cos(angle), math.sin(angle)
            x, z = r * c, r * s
            if bend:
                x += bend(y)
            points.append((x, y, z))
            vertex_normals.append((nr * c, ny, nr * s))
            uvs.append((j / segments, length / lengths[-1]))
    mesh.add_grid(points, vertex_normals, uvs, len(profile), segments + 1)


def shell(outer, thickness, inner_from=None):
    """A thin-walled vessel: the outer side, a rounded rim and the inner side back to the axis.

    `outer` runs from the axis at the bottom to the rim. The inner side is the outer one moved
    inward; `inner_from` cuts it below that height and closes it flat to the axis there, so a
    stem stays solid.
    """
    inner = offset(outer, thickness)
    if inner_from is not None:
        inner = [p for p in inner if p[1] >= inner_from and p[0] > 0.002]
        inner.insert(0, (0.0, inner[0][1]))
    else:
        inner = [p for p in inner if p[0] > 0.002]
        inner.insert(0, (0.0, inner[0][1]))
    rim_out, rim_in = outer[-1], inner[-1]
    center = ((rim_out[0] + rim_in[0]) / 2, (rim_out[1] + rim_in[1]) / 2)
    start = math.degrees(math.atan2(rim_out[1] - center[1], rim_out[0] - center[0]))
    rim = arc(center, math.dist(rim_out, rim_in) / 2, start, start + 180, 10)
    return outer + rim[1:-1] + list(reversed(inner)), inner


def split_at(profile, y):
    """The profile below height `y`, the point where it first rises through `y`, and the rest."""
    for i, (a, b) in enumerate(zip(profile, profile[1:])):
        if a[1] <= y < b[1]:
            t = (y - a[1]) / (b[1] - a[1])
            point = (a[0] + (b[0] - a[0]) * t, y)
            return profile[:i + 1], point, profile[i + 1:]
    raise ValueError("the profile never reaches that height")


def filled_vessel(outer, thickness, level, floor=None):
    """A glass vessel holding a liquid up to `level`, as two solids that each cost few crossings.

    Below the level the liquid takes the vessel's outer surface itself: the glass there is too
    thin to see, and a ray crossing the full vessel meets 2 surfaces instead of 6, so the glass
    renders within the app's default ray depth. Above the level the glass is a ring with both
    walls and a rounded rim. With `floor`, the glass below that height (a foot and stem) stays
    a separate solid, and the liquid starts there.
    Returns (glass meshes as profiles, liquid profiles), each a list of profiles to lathe.
    """
    gap = 0.003
    inner = [p for p in offset(outer, thickness) if p[0] > 0.002]
    _, out_low, out_up = split_at(outer, level + gap)
    _, in_low, in_up = split_at(inner, level + gap)
    rim_out, rim_in = outer[-1], inner[-1]
    center = ((rim_out[0] + rim_in[0]) / 2, (rim_out[1] + rim_in[1]) / 2)
    start = math.degrees(math.atan2(rim_out[1] - center[1], rim_out[0] - center[0]))
    rim = arc(center, math.dist(rim_out, rim_in) / 2, start, start + 180, 10)
    ring = [out_low] + out_up + rim[1:-1] + list(reversed(in_up)) + [in_low]
    glass = [ring, [in_low, out_low]]
    base = 0.0 if floor is None else floor + gap
    if floor is not None:
        below, top_edge, _ = split_at(outer, floor)
        glass += [below + [top_edge], [top_edge, (top_edge[0] * 0.5, floor), (0.0, floor)]]
    if base > 0.0:
        _, low, rest = split_at(outer, base)
        side = [(0.0, base), (low[0] * 0.5, base), low]
    else:
        rest = outer[1:]
        side = [outer[0]]
    _, edge, _ = split_at(outer, level)
    side += [p for p in rest if p[1] < level] + [edge]
    liquid = [side, [edge, (edge[0] * 0.5, level), (0.0, level)]]
    return glass, liquid


def lathe_all(name, profiles, seg):
    mesh = Mesh()
    for profile in profiles:
        lathe(mesh, profile, seg)
    return mesh.write_glb(name)


def wine_glass(seg):
    outer = catmull_rom([(0.0, 0.0), (0.2, 0.0), (0.355, 0.0)], 0.004)
    outer += arc((0.355, 0.009), 0.009, -90, 90, 8)[1:]
    outer += catmull_rom([(0.355, 0.018), (0.29, 0.03), (0.16, 0.06), (0.07, 0.11), (0.042, 0.2),
                          (0.038, 0.45), (0.042, 0.66), (0.075, 0.75), (0.2, 0.86), (0.33, 1.02), (0.4, 1.25),
                          (0.395, 1.5), (0.36, 1.75), (0.33, 1.98)])[1:]
    outer = simplify(outer, 0.06)
    glass, wine = filled_vessel(outer, 0.016, 1.22, floor=0.79)
    return lathe_all("wine-glass", glass, seg), lathe_all("wine-glass-wine", wine, seg)


def decanter(seg):
    outer = catmull_rom([(0.0, 0.0), (0.5, 0.0), (0.98, 0.0)], 0.004)
    outer += arc((0.98, 0.05), 0.05, -90, 0, 8)[1:]
    outer += catmull_rom([(1.03, 0.05), (1.1, 0.3), (1.12, 0.55), (1.02, 0.95), (0.72, 1.4), (0.4, 1.82),
                          (0.27, 2.2), (0.24, 2.75), (0.26, 3.1), (0.33, 3.32), (0.36, 3.4)])[1:]
    outer = simplify(outer, 0.07)
    glass, wine = filled_vessel(outer, 0.03, 0.82)
    return lathe_all("decanter", glass, seg), lathe_all("decanter-wine", wine, seg)


def bowl(seg):
    outer = catmull_rom([(0.0, 0.05), (0.4, 0.05), (0.58, 0.04)], 0.004)
    outer += catmull_rom([(0.58, 0.04), (0.62, 0.0), (0.74, 0.0), (0.79, 0.06), (0.9, 0.12), (1.25, 0.25),
                          (1.55, 0.44), (1.72, 0.62), (1.78, 0.7)])[1:]
    outer = simplify(outer, 0.06)
    profile, inner = shell(outer, 0.07, inner_from=0.16)
    mesh = Mesh()
    lathe(mesh, profile, seg)
    return mesh.write_glb("bowl"), inner


def copper_cup(seg):
    outer = catmull_rom([(0.0, 0.0), (0.3, 0.0), (0.5, 0.0)], 0.004)
    outer += arc((0.5, 0.04), 0.04, -90, 0, 8)[1:]
    outer += catmull_rom([(0.54, 0.04), (0.56, 0.4), (0.58, 0.9), (0.6, 1.2), (0.64, 1.32), (0.67, 1.36)])[1:]
    outer = simplify(outer, 0.07)
    profile, _ = shell(outer, 0.035)
    mesh = Mesh()
    lathe(mesh, profile, seg)
    # The handle: a tube swept along a curve in the XY plane, its ends hidden inside the wall.
    path = catmull_rom([(0.54, 1.12), (0.75, 1.13), (0.93, 1.02), (0.98, 0.78), (0.9, 0.52), (0.72, 0.38),
                        (0.54, 0.34)], 0.002)
    path = simplify(path, 0.03, 8)
    radius, around = 0.055, 24
    points, normals, uvs = [], [], []
    for i, (x, y) in enumerate(path):
        a, b = path[max(i - 1, 0)], path[min(i + 1, len(path) - 1)]
        tx, ty = b[0] - a[0], b[1] - a[1]
        length = math.hypot(tx, ty)
        tx, ty = tx / length, ty / length
        # Cross-section in the plane of (ty, -tx, 0) and (0, 0, 1).
        for j in range(around + 1):
            angle = 2 * math.pi * j / around
            n = (ty * math.cos(angle), -tx * math.cos(angle), math.sin(angle))
            points.append((x + radius * n[0], y + radius * n[1], radius * n[2]))
            normals.append(n)
            uvs.append((j / around, i / (len(path) - 1)))
    mesh.add_grid(points, normals, uvs, len(path), around + 1)
    return mesh.write_glb("copper-cup")


def candlestick(seg):
    outer = catmull_rom([(0.0, 0.0), (0.4, 0.0), (0.62, 0.0)], 0.004)
    outer += arc((0.62, 0.03), 0.03, -90, 90, 8)[1:]
    outer += catmull_rom([(0.62, 0.06), (0.5, 0.1), (0.3, 0.16), (0.16, 0.26), (0.11, 0.4), (0.1, 0.55),
                          (0.16, 0.64), (0.1, 0.72), (0.075, 0.9), (0.08, 1.2), (0.12, 1.3), (0.1, 1.36),
                          (0.16, 1.42), (0.3, 1.48), (0.33, 1.53), (0.3, 1.56), (0.22, 1.56), (0.2, 1.5)], 0.003)[1:]
    outer += [(0.1, 1.5), (0.0, 1.5)]
    outer = simplify(outer, 0.03)
    mesh = Mesh()
    lathe(mesh, outer, seg)
    return mesh.write_glb("candlestick")


def flame(seg):
    profile = catmull_rom([(0.0, 0.0), (0.035, 0.025), (0.058, 0.1), (0.052, 0.19), (0.03, 0.29), (0.0, 0.38)])
    mesh = Mesh()
    lathe(mesh, simplify(profile, 0.02), seg)
    return mesh.write_glb("flame")


def pear(seg):
    profile = catmull_rom([(0.0, 0.0), (0.2, 0.015), (0.42, 0.13), (0.53, 0.36), (0.5, 0.62), (0.38, 0.86),
                           (0.27, 1.05), (0.2, 1.25), (0.13, 1.4), (0.05, 1.47), (0.0, 1.48)])
    profile = simplify(profile, 0.04)
    mesh = Mesh()
    # The pear leans: its upper half bends sideways.
    lathe(mesh, profile, seg, bend=lambda y: 0.14 * max(0.0, y - 0.6) ** 2)
    # A short curved stem.
    stem = [(0.14 * 0.88 ** 2 + 0.02 * t * t, 1.46 + 0.22 * t) for t in [k / 8 for k in range(9)]]
    radius, around = 0.022, 10
    points, normals, uvs = [], [], []
    for i, (x, y) in enumerate(stem):
        for j in range(around + 1):
            angle = 2 * math.pi * j / around
            n = (math.cos(angle), 0.0, math.sin(angle))
            points.append((x + radius * n[0], y, radius * n[2]))
            normals.append(n)
            uvs.append((0.5, 0.97))
    mesh.add_grid(points, normals, uvs, len(stem), around + 1)
    return mesh.write_glb("pear")


# The table top: a board 5 cm thick with rounded edges, extruded along X.
TABLE_X, TABLE_FRONT, TABLE_BACK, TABLE_THICK = 8.95, -3.5, 6.0, 0.5


def table_top():
    r = 0.05
    f, b, t = TABLE_FRONT, TABLE_BACK, TABLE_THICK
    # Cross-section in (z, y), counterclockwise seen from +X: top from back to front, then down.
    section = arc((f + r, -r), r, 90, 180, 6) + arc((f + r, -t + r), r, 180, 270, 6)[1:] \
        + arc((b - r, -t + r), r, 270, 360, 6)[1:] + arc((b - r, -r), r, 0, 90, 6)[1:] + [(f + r, 0.0)]
    lengths = [0.0]
    for a, c in zip(section, section[1:]):
        lengths.append(lengths[-1] + math.dist(a, c))
    mesh = Mesh()
    xs = [-TABLE_X + 2 * TABLE_X * k / 24 for k in range(25)]
    points, normals, uvs = [], [], []
    for (z, y), length in zip(section, lengths):
        for x in xs:
            # Seen from +X, the section runs counterclockwise in (z, y); its outward normal is to
            # the right of travel: (dy, -dz) gives it in (z, y).
            points.append((x, y, z))
            uvs.append(((x + TABLE_X) / WOOD_U, (length + 0.37) / WOOD_V))
    sn = []
    for i in range(len(section)):
        a, c = section[max(i - 1, 0)], section[min(i + 1, len(section) - 1)]
        dz, dy = c[0] - a[0], c[1] - a[1]
        n = math.hypot(dz, dy) or 1
        sn.append((-dy / n, dz / n))
    # Check the direction on the top face, which must face up.
    top_index = len(section) - 1
    sign = 1 if sn[top_index][1] > 0 else -1
    for nz, ny in sn:
        for _ in xs:
            normals.append((0.0, sign * ny, sign * nz))
    mesh.add_grid(points, normals, uvs, len(section), len(xs))
    return mesh.write_glb("table")


# Where the towel lies: its far end on the table, at z = CLOTH_BACK, and its near end hanging.
CLOTH_X0, CLOTH_BACK, CLOTH_LIFT = -3.5, 0.8, 0.012
GLASS_AT = (-0.9, -1.0)


def towel():
    columns, rows = 97, 150
    on_table = CLOTH_BACK - TABLE_FRONT
    bend_r = 0.03
    rng = random.Random(41)
    phases = [rng.uniform(0, 2 * math.pi) for _ in range(4)]

    def wrinkle(x, z):
        """Soft folds on the part lying on the table, running toward the edge where they fall into
        the hanging folds, and flattened where the wine glass stands."""
        d = math.hypot(x - GLASS_AT[0], z - GLASS_AT[1])
        keep_flat = smoothstep(0.45, 1.2, d)
        toward_edge = smoothstep(CLOTH_BACK - 0.3, CLOTH_BACK - 3.0, z)
        u = x - CLOTH_X0
        ridge = (math.sin(2 * math.pi * u / 1.15 + phases[2] + 0.35 * z) * 0.5 + 0.5) ** 2
        ridge2 = (math.sin(1.7 * x - 1.1 * z + phases[1]) * 0.5 + 0.5) ** 4
        return keep_flat * (0.11 * ridge * toward_edge + 0.045 * ridge2)

    def hang(x, depth):
        """Folds of the hanging part, growing toward the hem."""
        grow = smoothstep(0.0, 1.6, depth)
        u = x - CLOTH_X0
        fold = (math.sin(2 * math.pi * u / 1.15 + phases[2]) * 0.6 + math.sin(2 * math.pi * u / 0.53 + phases[3]) * 0.4)
        return grow * (0.17 + 0.17 * fold) + 0.02 * depth

    bend_len = math.pi / 2 * bend_r
    points, uvs = [], []
    for i in range(rows):
        s = CLOTH_L * i / (rows - 1)
        for j in range(columns):
            u = j / (columns - 1)
            # The long sides are not quite straight.
            x = CLOTH_X0 + CLOTH_W * u + 0.05 * math.sin(1.3 * s + phases[0]) + 0.03 * math.sin(3.1 * s)
            if s <= on_table:
                z = CLOTH_BACK - s
                y = CLOTH_LIFT + wrinkle(x, z) * smoothstep(0.0, 0.6, z - TABLE_FRONT)
            elif s <= on_table + bend_len:
                a = (s - on_table) / bend_r
                z = TABLE_FRONT + bend_r - bend_r * math.sin(a) - CLOTH_LIFT * math.sin(a)
                y = -bend_r + (bend_r + CLOTH_LIFT) * math.cos(a)
            else:
                depth = s - on_table - bend_len
                z = TABLE_FRONT - CLOTH_LIFT - hang(x, depth)
                y = -bend_r - depth
            points.append((x, y, z))
            uvs.append((u, s / CLOTH_L))
    # Normals from the grid; the hanging part's length shrinks a little by its folds, which the
    # weave does not notice at this scale.
    normals = []
    for i in range(rows):
        for j in range(columns):
            p = lambda a, b: points[min(max(a, 0), rows - 1) * columns + min(max(b, 0), columns - 1)]
            du = [p(i, j + 1)[k] - p(i, j - 1)[k] for k in range(3)]
            ds = [p(i + 1, j)[k] - p(i - 1, j)[k] for k in range(3)]
            n = (du[1] * ds[2] - du[2] * ds[1], du[2] * ds[0] - du[0] * ds[2], du[0] * ds[1] - du[1] * ds[0])
            length = math.sqrt(sum(c * c for c in n)) or 1
            normals.append(tuple(c / length for c in n))
    mesh = Mesh()
    mesh.add_grid(points, normals, uvs, rows, columns)
    return mesh.write_glb("towel")


# ---------------------------------------------------------------- grapes

def heap_of_grapes(inner, center, count, seed):
    """Drops grapes into the bowl and lets them settle against each other and the bowl."""
    rng = random.Random(seed)
    floor = min(p[1] for p in inner)
    rim = max(p[1] for p in inner)
    # Inner radius at each height.
    curve = sorted((p[1], p[0]) for p in inner if p[0] > 0)

    def wall(y):
        if y <= curve[0][0]:
            return curve[0][1]
        for (y0, r0), (y1, r1) in zip(curve, curve[1:]):
            if y0 <= y <= y1:
                return r0 + (r1 - r0) * (y - y0) / ((y1 - y0) or 1)
        return curve[-1][1]

    grapes = []
    for _ in range(count):
        radius = rng.uniform(0.15, 0.19)
        a, d = rng.uniform(0, 2 * math.pi), math.sqrt(rng.random()) * 0.9
        grapes.append([d * math.cos(a), floor + 0.3 + rng.uniform(0, 3.0), d * math.sin(a), radius])
    for step in range(600):
        for g in grapes:
            g[1] -= 0.01
            # Pull toward the middle a little, so they mound rather than spread flat.
            g[0] *= 0.996
            g[2] *= 0.996
        for _ in range(2):
            for i, g in enumerate(grapes):
                for h in grapes[i + 1:]:
                    dx, dy, dz = h[0] - g[0], h[1] - g[1], h[2] - g[2]
                    d = math.sqrt(dx * dx + dy * dy + dz * dz)
                    overlap = g[3] + h[3] - d
                    if overlap > 0 and d > 1e-6:
                        push = overlap / d / 2
                        g[0] -= dx * push; g[1] -= dy * push; g[2] -= dz * push
                        h[0] += dx * push; h[1] += dy * push; h[2] += dz * push
                if g[1] - g[3] < floor:
                    g[1] = floor + g[3]
                y = min(g[1], rim)
                r = math.hypot(g[0], g[2])
                limit = wall(y) - g[3] * 1.05
                if r > limit and r > 0:
                    g[0] *= limit / r
                    g[2] *= limit / r
    return [(center[0] + g[0], center[1] + g[1], center[2] + g[2], g[3]) for g in grapes]


# ---------------------------------------------------------------- scene

def surface(color, preset, roughness, **extra):
    return {"color": color, "surface": {"preset": preset, "roughness": roughness, **extra}}


def main():
    MESHES.mkdir(parents=True, exist_ok=True)
    IMAGES.mkdir(parents=True, exist_ok=True)
    oak, oak_height = wood_images()
    linen, weave = linen_images()
    pear_skin = pear_image()

    glass_file, glass_wine_file = wine_glass(96)
    decanter_file, decanter_wine_file = decanter(96)
    bowl_file, bowl_inner = bowl(96)
    cup_file = copper_cup(96)
    candlestick_file = candlestick(64)
    pear_file = pear(48)
    flame_file = flame(24)
    table_file = table_top()
    towel_file = towel()

    clear_glass = surface([0.93, 0.97, 0.95], "glass", 0.0, ior=1.52, absorption_depth=2.5)
    # The wine glass is lead crystal: it splits light a little more.
    crystal = surface([0.95, 0.97, 0.97], "glass", 0.0, ior=1.56, abbe=48, absorption_depth=3.0)
    wine = surface([0.72, 0.06, 0.1], "glass", 0.0, ior=1.34, absorption_depth=0.55)
    oak_material = {"color": [1, 1, 1], "texture": oak, "surface": {"preset": "plastic", "roughness": 0.42},
                    "bump": {"map": oak_height, "strength": 0.006}}
    linen_material = {"color": [1, 1, 1], "texture": linen,
                      "surface": {"preset": "plastic", "roughness": 0.85, "sheen": 0.7, "sheen_color": [0.95, 0.92, 0.85]},
                      "bump": {"map": weave, "strength": 0.004}}
    glaze = {"color": [0.36, 0.45, 0.4], "surface": {"preset": "ceramic", "roughness": 0.18, "clearcoat": 0.5,
                                                     "clearcoat_roughness": 0.05, "subsurface": 0.3,
                                                     "subsurface_radius": 0.05}}
    copper = surface([0.96, 0.58, 0.42], "metal", 0.28, anisotropy=0.7, clearcoat=0.6, clearcoat_roughness=0.04)
    brass = surface([0.95, 0.74, 0.4], "metal", 0.2, anisotropy=0.5)
    purple = surface([0.16, 0.03, 0.1], "plastic", 0.3, subsurface=0.6, subsurface_radius=0.06,
                     sheen=0.5, sheen_color=[0.55, 0.6, 0.75])
    green = surface([0.42, 0.48, 0.12], "plastic", 0.25, subsurface=0.7, subsurface_radius=0.08,
                    sheen=0.3, sheen_color=[0.8, 0.85, 0.7])
    wax = surface([0.9, 0.85, 0.72], "plastic", 0.4, subsurface=0.9, subsurface_radius=0.18)
    plaster = surface([0.3, 0.27, 0.23], "plastic", 0.9)
    frame_wood = surface([0.1, 0.07, 0.05], "plastic", 0.6)
    pear_material = {"color": [1, 1, 1], "texture": pear_skin,
                     "surface": {"preset": "plastic", "roughness": 0.45, "subsurface": 0.35, "subsurface_radius": 0.05}}

    objects = []

    def model(file, position, material, yaw=0.0):
        objects.append({"type": "model", "file": file, "position": position, "rotation": [0, yaw, 0],
                        "scale": 1, "material": material})

    def box(center, size, material):
        objects.append({"type": "box", "center": center, "size": size, "material": material})

    model(table_file, [0, 0, 0], oak_material)
    model(towel_file, [0, 0, 0], linen_material)
    glass_y = CLOTH_LIFT + 0.002
    model(glass_file, [GLASS_AT[0], glass_y, GLASS_AT[1]], crystal)
    model(glass_wine_file, [GLASS_AT[0], glass_y, GLASS_AT[1]], wine)
    decanter_at = [-4.6, 0.0, 3.4]
    model(decanter_file, decanter_at, clear_glass)
    model(decanter_wine_file, decanter_at, surface([0.72, 0.07, 0.1], "glass", 0.0, ior=1.34, absorption_depth=1.3))
    bowl_at = [1.2, 0.0, 1.4]
    model(bowl_file, bowl_at, glaze)
    for x, y, z, r in heap_of_grapes(bowl_inner, bowl_at, 90, 51):
        objects.append({"type": "sphere", "center": [round(x, 4), round(y, 4), round(z, 4)], "radius": round(r, 4),
                        "material": purple})
    # A few grapes fallen on the table, and two green ones.
    for x, z, r, material in ((1.15, -2.2, 0.17, purple), (1.5, -2.55, 0.16, purple),
                              (3.7, -2.0, 0.17, green), (4.0, -1.75, 0.16, green)):
        objects.append({"type": "sphere", "center": [x, r, z], "radius": r, "material": material})
    model(pear_file, [2.5, 0.0, -1.5], pear_material, yaw=200)
    model(cup_file, [3.9, 0.0, 3.4], copper, yaw=-30)
    candle_at = [-0.2, 0.0, 5.0]
    model(candlestick_file, candle_at, brass)
    candle_top = 1.5 + 2.1
    objects.append({"type": "cylinder", "center": [candle_at[0], 1.5 + 2.1 / 2, candle_at[2]], "radius": 0.19,
                    "height": 2.1, "material": wax})
    objects.append({"type": "cylinder", "center": [candle_at[0], candle_top + 0.06, candle_at[2]], "radius": 0.012,
                    "height": 0.12, "material": surface([0.02, 0.02, 0.02], "plastic", 0.8)})
    model(flame_file, [candle_at[0], candle_top + 0.17, candle_at[2]],
          {"color": [0, 0, 0], "emission": {"color": [1.0, 0.6, 0.22], "strength": 12}})

    # The room: a dark plaster box around the table, open only through a window in the back
    # wall, behind the still life on the left, so the glass is seen against the light.
    back, left, right, floor, ceiling, front = 8.0, -12.5, 14.0, -7.5, 14.0, -18.0
    t = 0.3
    wx0, wx1, wy0, wy1 = -11.5, -2.5, 0.4, 8.5  # the window: x from, x to, y from, y to
    z = back + t / 2
    box([(left + wx0) / 2, (floor + ceiling) / 2, z], [wx0 - left, ceiling - floor, t], plaster)
    box([(wx1 + right) / 2, (floor + ceiling) / 2, z], [right - wx1, ceiling - floor, t], plaster)
    box([(wx0 + wx1) / 2, (floor + wy0) / 2, z], [wx1 - wx0, wy0 - floor, t], plaster)
    box([(wx0 + wx1) / 2, (wy1 + ceiling) / 2, z], [wx1 - wx0, ceiling - wy1, t], plaster)
    # Window frame: two mullions upright, one across, and a deep sill.
    for x in (wx0 + (wx1 - wx0) / 3, wx0 + 2 * (wx1 - wx0) / 3):
        box([x, (wy0 + wy1) / 2, z], [0.18, wy1 - wy0, t + 0.1], frame_wood)
    box([(wx0 + wx1) / 2, wy0 + (wy1 - wy0) * 0.6, z], [wx1 - wx0, 0.18, t + 0.1], frame_wood)
    box([(wx0 + wx1) / 2, wy0 - 0.08, z - 0.2], [wx1 - wx0 + 0.4, 0.16, t + 0.6], frame_wood)
    height, depth, width = ceiling - floor, back - front, right - left
    box([left - t / 2, (floor + ceiling) / 2, (front + back) / 2], [t, height, depth], plaster)
    box([right + t / 2, (floor + ceiling) / 2, (front + back) / 2], [t, height, depth], plaster)
    box([(left + right) / 2, (floor + ceiling) / 2, front - t / 2], [width + 2 * t, height, t], plaster)
    box([(left + right) / 2, ceiling + t / 2, (front + back) / 2], [width + 2 * t, t, depth], plaster)
    box([(left + right) / 2, floor - t / 2, (front + back) / 2], [width + 2 * t, t, depth], frame_wood)

    scene = {
        "format": 3,
        "camera": {"position": [3.0, 3.0, -11.0], "target": [-1.0, 1.3, 2.0], "zoom": 1.9,
                   "aperture": 0.14, "focus_distance": 11.0, "blades": 0},
        "environment": {"sky": {"elevation": 14, "azimuth": 312, "turbidity": 3, "sun_strength": 3},
                        "intensity": 1.3, "visible": True},
        "fog": {"density": 0.008, "color": [0.92, 0.9, 0.86], "anisotropy": 0.6,
                "bounds": [[left, floor, front], [right, ceiling, back]]},
        "lights": [
            # The candle flame.
            {"type": "sphere", "position": [candle_at[0], candle_top + 0.15, candle_at[2]], "radius": 0.035,
             "color": [1.0, 0.62, 0.28], "intensity": 3.0},
            # Soft light from the rest of the room, behind the camera on the right.
            {"type": "rectangle", "position": [12.0, 9.0, -14.0], "edge_u": [3.0, 0.0, 3.0],
             "edge_v": [-1.2, 2.5, 1.2], "color": [0.85, 0.9, 1.0], "intensity": 4.5},
        ],
        "objects": objects,
        # How the app first renders it: film tone mapping and the denoiser. Ray depth stays at the
        # app's 5, which the glassware is built for; each extra level costs about three times more.
        "render": {"app": 1, "quality": 256, "toneMapping": "agx_punchy", "denoise": True, "depth": 12,
                   "glossyDepth": 3, "transmissionDepth": 10},
    }
    SCENE.write_text(json.dumps(scene, indent=1) + "\n")
    print(SCENE)


if __name__ == "__main__":
    main()

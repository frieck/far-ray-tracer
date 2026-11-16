// Path tracing in a compute shader: one camera sample per pixel per dispatch, added to `accum`.
// The scene comes packed by lib.rs; the BVH is the one the CPU walks (far-core's bvh.rs).

struct Params {
    origin: vec3<f32>, width: u32,
    corner: vec3<f32>, height: u32,
    dx: vec3<f32>, pass_index: u32,
    // Diffuse rebounds a path may take.
    dy: vec3<f32>, max_bounces: u32,
    plane_count: u32, light_count: u32, has_env: u32, env_visible: u32,
    exposure: f32, cap: f32, env_intensity: f32, env_rotation: f32,
    // The lens: its opening (0 for a pinhole), the distance in focus along `forward`, and the
    // diaphragm's blades (0 for round) and their rotation in radians.
    right: vec3<f32>, aperture: f32,
    up: vec3<f32>, focus: f32,
    forward: vec3<f32>, blades: u32,
    blade_rotation: f32,
    // Whether any material lets light through, which shadow rays then follow.
    has_glass: u32,
    // Multiplier of each rebound, and the most light one may bring back: far-core's Bounces.
    strength: f32, clamp: f32,
    // Mirror-like and glass segments a path may take, and of each kind.
    max_depth: u32, glossy_max: u32, transmission_max: u32,
    // Where the environment's sampling tables start in `texels`: far-core's sampling_tables.
    env_tables: u32,
    // What `accum` holds per pixel, in vec4s: `stride` of them, the light first; the first
    // hits next when `features` is set, for the denoiser; the light for bloom at `glow_at`
    // when it is not 0.
    stride: u32, features: u32, glow_at: u32,
    // Fog and smoke: two entries each in `shapes` from `volume_first`, read by volume_shape and
    // volume_medium.
    volume_first: u32, volume_count: u32,
    // far-core's spectral::white.
    white_r: f32, white_g: f32, white_b: f32,
    // The caustic photon map in `texels`, as lib.rs packs it: its buckets (0 for none), where
    // their starts are and where the photons are, seven words each, and the lookup radius.
    photon_buckets: u32, bucket_first: u32, photon_first: u32, photon_radius: f32,
    // The multiplier of caustic light; whether only that light is shown; whether photons carry
    // the environment's light.
    caustic_strength: f32, caustics_only: u32, photon_env: u32, pad: u32,
}

struct Node { low: vec3<f32>, first: u32, high: vec3<f32>, count: u32 }
// With each corner's texture coordinates.
struct Tri {
    p0: vec3<f32>, material: u32,
    e1: vec3<f32>, u0: f32,
    e2: vec3<f32>, v0: f32,
    n0: vec3<f32>, u1: f32,
    n1: vec3<f32>, v1: f32,
    n2: vec3<f32>, u2: f32,
    // For a displaced triangle, one more than the index of its record in `tris`: the shape
    // under it, as lib.rs packs it. 0 for others.
    v2: f32, displaced: u32, pad2: u32, pad3: u32,
}
// A sphere (kind 0), or a solid with its own axes, its own Y the axis of revolution: a box (1,
// with `half`), a cylinder (2), a tube (3, inner radius in half.x), a cone (4, apex at +Y) or a
// torus (5, the tube's radius in half.x). `radius` is the outer one; half.y the half height.
// A ground (6) has its normal in ax and its texture axes in ay and az; grounds come first, and
// params.plane_count of them are tested apart from the BVH.
struct Shape {
    center: vec3<f32>, kind: u32,
    ax: vec3<f32>, material: u32,
    ay: vec3<f32>, radius: f32,
    az: vec3<f32>, pad1: u32,
    half: vec3<f32>, pad2: u32,
}
// kind 0: the 2008 material (diff, refl). 1: GGX (roughness, metallic). 2: glass.
struct Material {
    color: vec3<f32>, kind: u32,
    emission: vec3<f32>, roughness: f32,
    metallic: f32, ior: f32, diff: f32, refl: f32,
    // The 2008 highlight's strength.
    spec: f32,
    // The color texture, or NO_TEXTURE, and its repeats.
    texture: u32, u_scale: f32, v_scale: f32,
    // How much light goes through (2008 materials and glass), and the depth at which glass
    // keeps its color's share of light, or 0 for the CPU's fixed absorption.
    refr: f32, absorption_depth: f32,
    // The bump map's texture, NORMAL_MAP set for a normal map, or NO_TEXTURE; its strength.
    bump: u32, bump_strength: f32,
    // Maps that scale a GGX surface's roughness and metallic, tint the emission, and cut holes
    // where darker than half gray; NO_TEXTURE where absent.
    roughness_map: u32, metallic_map: u32, emission_map: u32, alpha_map: u32,
    // The GGX layers: sheen, clear coat, and anisotropy turned by `rotation` radians.
    sheen_color: vec3<f32>, sheen: f32,
    clearcoat: f32, clearcoat_roughness: f32, anisotropy: f32, rotation: f32,
    // The glass's Abbe number, 0 when it does not disperse.
    abbe: f32,
    // The displacement map and the height white reaches; NO_TEXTURE without one.
    displacement: u32, displacement_height: f32, pad: u32,
    // The share of diffuse light that walks under a GGX surface, and the mean step there.
    subsurface: f32, subsurface_radius: f32, pad2: u32, pad3: u32,
}
const NORMAL_MAP: u32 = 0x80000000u;
const NO_TEXTURE: u32 = 0xffffffffu;

// What the scene uses, set when the pipeline is made (lib.rs's Features): the compiler drops the
// code for what is off, which frees registers and lets more threads run at once, as Cycles'
// specialized kernels do. Each is also checked at run time, so `true` is always correct.
override HAS_VOLUMES: bool = true;
override HAS_GLASS: bool = true;
override HAS_ENV: bool = true;
override HAS_GGX: bool = true;
override HAS_TEXTURES: bool = true;
// Boxes, cylinders, tubes, cones and tori; spheres and grounds are always there.
override HAS_SOLIDS: bool = true;
override HAS_LENS: bool = true;
override HAS_BUMP: bool = true;
// Roughness, metallic and emission maps; alpha maps, which rays look through.
override HAS_MAPS: bool = true;
override HAS_ALPHA: bool = true;
// Clear coat, sheen and anisotropy on GGX surfaces.
override HAS_COATS: bool = true;
// Spotlight cones and IES profiles on bulbs.
override HAS_PROFILES: bool = true;
// Glass that splits light by wavelength.
override HAS_DISPERSION: bool = true;
// Displaced objects and grounds.
override HAS_DISPLACEMENT: bool = true;
// Light walking under GGX surfaces.
override HAS_SUBSURFACE: bool = true;
// The kind of a stand-in material for a fog particle, which `respond` answers with its phase.
const MEDIUM: u32 = 7u;
// kind 0: a 2008 light, with no falloff. 1: a bulb. 2: a rectangular panel.
struct Light {
    pos: vec3<f32>, kind: u32,
    color: vec3<f32>, radius: f32,
    u: vec3<f32>, area: f32,
    v: vec3<f32>, pad1: u32,
    normal: vec3<f32>, pad2: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> nodes: array<Node>;
@group(0) @binding(2) var<storage, read> refs: array<u32>;
@group(0) @binding(3) var<storage, read> tris: array<Tri>;
@group(0) @binding(4) var<storage, read> shapes: array<Shape>;
// For each texture its first texel, width and height; then texels, RGB bytes over 256; then the
// environment's sampling tables, as float bits.
@group(0) @binding(5) var<storage, read> texels: array<u32>;
@group(0) @binding(6) var<storage, read> materials: array<Material>;
@group(0) @binding(7) var<storage, read> lights: array<Light>;
@group(0) @binding(8) var<storage, read_write> accum: array<vec4<f32>>;
@group(0) @binding(9) var env: texture_2d<f32>;

const PI: f32 = 3.14159265;
const FAR: f32 = 1e30;
const OFFSET: f32 = 1e-3;

var<private> rng: u32;
// What the camera ray met first, which guides the denoiser; zero when it met nothing.
var<private> first_albedo: vec3<f32>;
var<private> first_normal: vec3<f32>;
var<private> first_depth: f32;

fn hash(x: u32) -> u32 {
    var v = x * 747796405u + 2891336453u;
    let w = ((v >> ((v >> 28u) + 4u)) ^ v) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rand() -> f32 {
    rng = hash(rng);
    return f32(rng >> 8u) / 16777216.0;
}

struct Hit {
    t: f32,
    // Geometric normal and shading normal, as stored: not yet turned toward the ray.
    ng: vec3<f32>,
    ns: vec3<f32>,
    material: u32,
    // A triangle, or a shape with the top bit set; and a triangle's barycentric coordinates.
    prim: u32,
    bary: vec2<f32>,
}

fn hit_tri(i: u32, o: vec3<f32>, d: vec3<f32>, best: ptr<function, Hit>) {
    let tri = tris[i];
    let p = cross(d, tri.e2);
    let det = dot(tri.e1, p);
    if (abs(det) < 1e-12) { return; }
    let inv = 1.0 / det;
    let s = o - tri.p0;
    let u = dot(s, p) * inv;
    if (u < 0.0 || u > 1.0) { return; }
    let q = cross(s, tri.e1);
    let v = dot(d, q) * inv;
    if (v < 0.0 || u + v > 1.0) { return; }
    let t = dot(tri.e2, q) * inv;
    if (t <= OFFSET * 0.1 || t >= (*best).t) { return; }
    // The CPU's Triangle::new takes (c - a) x (b - a): which side is the front decides when a ray
    // enters glass, and how much it absorbs.
    let ng = normalize(cross(tri.e2, tri.e1));
    var ns = tri.n0 * (1.0 - u - v) + tri.n1 * u + tri.n2 * v;
    if (dot(ns, ns) < 1e-12) { ns = ng; } else { ns = normalize(ns); }
    *best = Hit(t, ng, ns, tri.material, i, vec2<f32>(u, v));
}

fn hit_shape(i: u32, o: vec3<f32>, d: vec3<f32>, best: ptr<function, Hit>) {
    let s = shapes[i];
    let rel = o - s.center;
    if (s.kind == 0u) {
        let b = dot(rel, d);
        let c = dot(rel, rel) - s.radius * s.radius;
        let disc = b * b - c;
        if (disc <= 0.0) { return; }
        let root = sqrt(disc);
        var t = -b - root;
        if (t <= OFFSET * 0.1) { t = -b + root; }
        if (t <= OFFSET * 0.1 || t >= (*best).t) { return; }
        let n = normalize(rel + d * t);
        *best = Hit(t, n, n, s.material, i | 0x80000000u, vec2<f32>(0.0));
        return;
    }
    if (!HAS_SOLIDS) { return; }
    // A solid, in its own frame; turning keeps lengths, so t is the same in both.
    let lo = vec3<f32>(dot(rel, s.ax), dot(rel, s.ay), dot(rel, s.az));
    let ld = vec3<f32>(dot(d, s.ax), dot(d, s.ay), dot(d, s.az));
    var t = FAR;
    if (s.kind == 1u) {
        let inv = 1.0 / ld;
        let a = (-s.half - lo) * inv;
        let b = (s.half - lo) * inv;
        let near = max(max(min(a.x, b.x), min(a.y, b.y)), min(a.z, b.z));
        let far = min(min(max(a.x, b.x), max(a.y, b.y)), max(a.z, b.z));
        if (near <= far) { t = closer(near, closer(far, t)); }
    } else if (s.kind == 2u) {
        t = wall(lo, ld, s.radius, s.half.y, t);
        t = ring(lo, ld, s.half.y, 0.0, s.radius, t);
        t = ring(lo, ld, -s.half.y, 0.0, s.radius, t);
    } else if (s.kind == 3u) {
        t = wall(lo, ld, s.radius, s.half.y, t);
        t = wall(lo, ld, s.half.x, s.half.y, t);
        t = ring(lo, ld, s.half.y, s.half.x, s.radius, t);
        t = ring(lo, ld, -s.half.y, s.half.x, s.radius, t);
    } else if (s.kind == 4u) {
        t = cone(lo, ld, s.radius, s.half.y, t);
        t = ring(lo, ld, -s.half.y, 0.0, s.radius, t);
    } else {
        t = torus(lo, ld, s.radius, s.half.x, t);
    }
    if (t >= (*best).t) { return; }
    let p = lo + ld * t;
    let local = solid_normal(s, p);
    let n = normalize(s.ax * local.x + s.ay * local.y + s.az * local.z);
    *best = Hit(t, n, n, s.material, i | 0x80000000u, vec2<f32>(0.0));
}

// The nearer of `t` and `best`, when `t` is ahead of the ray.
fn closer(t: f32, best: f32) -> f32 {
    if (t > OFFSET * 0.1 && t < best) { return t; }
    return best;
}

// The roots of a t^2 + b t + c, in order, or two FARs.
fn quadratic(a: f32, b: f32, c: f32) -> vec2<f32> {
    if (abs(a) < 1e-9) {
        if (abs(b) > 1e-9) { return vec2<f32>(-c / b); }
        return vec2<f32>(FAR);
    }
    let disc = b * b - 4.0 * a * c;
    if (disc < 0.0) { return vec2<f32>(FAR); }
    let q = -0.5 * (b + sign_of(b) * sqrt(disc));
    var t1 = 0.0;
    if (q != 0.0) { t1 = c / q; }
    let t0 = q / a;
    return vec2<f32>(min(t0, t1), max(t0, t1));
}

// The side of a cylinder around Y, between -h and h.
fn wall(o: vec3<f32>, d: vec3<f32>, radius: f32, h: f32, best: f32) -> f32 {
    let roots = quadratic(d.x * d.x + d.z * d.z, 2.0 * (o.x * d.x + o.z * d.z), o.x * o.x + o.z * o.z - radius * radius);
    var t = best;
    if (roots.x < FAR && abs(o.y + roots.x * d.y) <= h) { t = closer(roots.x, t); }
    if (roots.y < FAR && abs(o.y + roots.y * d.y) <= h) { t = closer(roots.y, t); }
    return t;
}

// A flat ring at height y, between two radii.
fn ring(o: vec3<f32>, d: vec3<f32>, y: f32, inner: f32, outer: f32, best: f32) -> f32 {
    if (d.y == 0.0) { return best; }
    let t = (y - o.y) / d.y;
    let x = o.x + t * d.x;
    let z = o.z + t * d.z;
    let r2 = x * x + z * z;
    if (r2 <= outer * outer && r2 >= inner * inner) { return closer(t, best); }
    return best;
}

fn cone(o: vec3<f32>, d: vec3<f32>, radius: f32, h: f32, best: f32) -> f32 {
    let k = radius / (2.0 * h);
    let k2 = k * k;
    let oy = h - o.y;
    let dy = -d.y;
    let roots = quadratic(d.x * d.x + d.z * d.z - k2 * dy * dy, 2.0 * (o.x * d.x + o.z * d.z - k2 * oy * dy),
        o.x * o.x + o.z * o.z - k2 * oy * oy);
    var t = best;
    if (roots.x < FAR && abs(o.y + roots.x * d.y) <= h) { t = closer(roots.x, t); }
    if (roots.y < FAR && abs(o.y + roots.y * d.y) <= h) { t = closer(roots.y, t); }
    return t;
}

fn torus_distance(p: vec3<f32>, major: f32, minor: f32) -> f32 {
    return length(vec2<f32>(length(p.xz) - major, p.y)) - minor;
}

// A torus by sphere tracing its distance field: the CPU solves the quartic in double precision,
// which shaders do not have everywhere. The distance is exact, so steps never pass the surface;
// a ray that starts inside walks the negated distance to leave it.
fn torus(o: vec3<f32>, d: vec3<f32>, major: f32, minor: f32, best: f32) -> f32 {
    let bound = major + minor;
    // Where the ray meets the bounding sphere.
    let b = dot(o, d);
    let disc = b * b - (dot(o, o) - bound * bound);
    if (disc <= 0.0) { return best; }
    let root = sqrt(disc);
    let far = min(-b + root, best);
    var t = max(-b - root, 0.0);
    // Leave the surface the ray starts on before marching.
    t = t + OFFSET * 0.5;
    let side = sign_of(torus_distance(o + d * t, major, minor));
    for (var i = 0; i < 96; i++) {
        if (t >= far) { return best; }
        let distance = torus_distance(o + d * t, major, minor) * side;
        if (distance < 1e-5 * max(1.0, minor)) { return closer(t, best); }
        t += max(distance, 1e-5);
    }
    return best;
}

// The outward normal of a solid at a local point on it, as the CPU picks it.
fn solid_normal(s: Shape, p: vec3<f32>) -> vec3<f32> {
    let distance = length(p.xz);
    var radial = vec3<f32>(1.0, 0.0, 0.0);
    if (distance > 1e-9) { radial = vec3<f32>(p.x / distance, 0.0, p.z / distance); }
    let cap = vec3<f32>(0.0, sign_of(p.y), 0.0);
    if (s.kind == 1u) {
        let ratio = abs(p / s.half);
        if (ratio.x >= ratio.y && ratio.x >= ratio.z) { return vec3<f32>(sign_of(p.x), 0.0, 0.0); }
        if (ratio.y >= ratio.z) { return vec3<f32>(0.0, sign_of(p.y), 0.0); }
        return vec3<f32>(0.0, 0.0, sign_of(p.z));
    }
    if (s.kind == 2u) {
        if (abs(abs(p.y) - s.half.y) < abs(distance - s.radius)) { return cap; }
        return radial;
    }
    if (s.kind == 3u) {
        let to_cap = abs(abs(p.y) - s.half.y);
        let to_outer = abs(distance - s.radius);
        let to_inner = abs(distance - s.half.x);
        if (to_cap < to_outer && to_cap < to_inner) { return cap; }
        if (to_outer < to_inner) { return radial; }
        return -radial;
    }
    if (s.kind == 4u) {
        let height = 2.0 * s.half.y;
        let side_radius = s.radius * (s.half.y - p.y) / height;
        if (abs(p.y + s.half.y) < abs(distance - side_radius)) { return vec3<f32>(0.0, -1.0, 0.0); }
        return normalize(radial * height + vec3<f32>(0.0, s.radius, 0.0));
    }
    return normalize(p - radial * s.radius);
}

fn sign_of(x: f32) -> f32 {
    if (x < 0.0) { return -1.0; }
    return 1.0;
}

// Where the ray enters the node's box before `limit`, or a negative number.
fn enter(node: u32, o: vec3<f32>, inv: vec3<f32>, limit: f32) -> f32 {
    let a = (nodes[node].low - o) * inv;
    let b = (nodes[node].high - o) * inv;
    let lo = min(a, b);
    let hi = max(a, b);
    let near = max(max(max(lo.x, lo.y), lo.z), 0.0);
    let far = min(min(min(hi.x, hi.y), hi.z), limit);
    if (near <= far) { return near; }
    return -1.0;
}

// The nearest surface along the ray before `limit`, looking through the holes alpha maps cut
// as the CPU's nearest_through_cutouts does, up to its MAX_CUTOUT_CROSSINGS; `any` returns the
// first one found instead, for shadow rays.
fn intersect(o: vec3<f32>, d: vec3<f32>, limit: f32, any: bool) -> Hit {
    if (!HAS_ALPHA) { return intersect_surface(o, d, limit, any); }
    var start = o;
    var skipped = 0.0;
    for (var k = 0; k < 16; k++) {
        var hit = intersect_surface(start, d, limit - skipped, false);
        if (hit.t >= limit - skipped) { break; }
        let m = materials[hit.material];
        let pos = start + d * hit.t;
        if (m.alpha_map == NO_TEXTURE || map_height(m.alpha_map, texture_uv(hit, pos, m)) >= 0.5) {
            hit.t += skipped;
            return hit;
        }
        skipped += hit.t + OFFSET;
        start = pos + d * OFFSET;
    }
    return Hit(limit, vec3<f32>(0.0), vec3<f32>(0.0), 0u, 0u, vec2<f32>(0.0));
}

fn intersect_surface(o: vec3<f32>, d: vec3<f32>, limit: f32, any: bool) -> Hit {
    var best = Hit(limit, vec3<f32>(0.0), vec3<f32>(0.0), 0u, 0u, vec2<f32>(0.0));
    for (var i = 0u; i < params.plane_count; i++) {
        let plane = shapes[i];
        if (HAS_DISPLACEMENT && materials[plane.material].displacement != NO_TEXTURE) {
            let t = displaced_ground(plane, materials[plane.material], o, d, best.t);
            if (t < best.t) {
                best = Hit(t, plane.ax, plane.ax, plane.material, i | 0x80000000u, vec2<f32>(0.0));
                if (any) { return best; }
            }
            continue;
        }
        let denom = dot(d, plane.ax);
        if (abs(denom) > 1e-7) {
            let t = dot(plane.center - o, plane.ax) / denom;
            if (t > OFFSET * 0.1 && t < best.t) {
                best = Hit(t, plane.ax, plane.ax, plane.material, i | 0x80000000u, vec2<f32>(0.0));
                if (any) { return best; }
            }
        }
    }
    let inv = 1.0 / d;
    if (enter(0u, o, inv, best.t) < 0.0) { return best; }
    var stack: array<u32, 48>;
    var top = 0u;
    var node = 0u;
    loop {
        let n = nodes[node];
        if (n.count > 0u) {
            for (var k = n.first; k < n.first + n.count; k++) {
                let r = refs[k];
                if ((r & 0x80000000u) != 0u) { hit_shape(r & 0x7fffffffu, o, d, &best); }
                else { hit_tri(r, o, d, &best); }
            }
            if (any && best.t < limit) { return best; }
        } else {
            let l = n.first;
            let a = enter(l, o, inv, best.t);
            let b = enter(l + 1u, o, inv, best.t);
            if (a >= 0.0 && b >= 0.0) {
                if (a <= b) { stack[top] = l + 1u; node = l; } else { stack[top] = l; node = l + 1u; }
                if (top < 47u) { top++; }
                continue;
            } else if (a >= 0.0) { node = l; continue; }
            else if (b >= 0.0) { node = l + 1u; continue; }
        }
        if (top == 0u) { break; }
        top--;
        node = stack[top];
    }
    return best;
}

// Where the ray meets a displaced ground before `limit`, or FAR, as displace.rs's
// intersect_ground: it enters the slab the relief can fill, steps across it about half a texel
// at a time and refines the crossing. Rays from below the base plane see the plain plane.
fn displaced_ground(plane: Shape, m: Material, o: vec3<f32>, d: vec3<f32>, limit: f32) -> f32 {
    let altitude = dot(o - plane.center, plane.ax);
    let climb = dot(d, plane.ax);
    let height = m.displacement_height;
    if (altitude < 0.0 || height <= 0.0) {
        if (abs(climb) <= 1e-7) { return FAR; }
        let t = -altitude / climb;
        if (t > OFFSET * 0.1 && t < limit) { return t; }
        return FAR;
    }
    var enter = 0.0;
    if (altitude > height) {
        if (climb >= -1e-7) { return FAR; }
        enter = (height - altitude) / climb;
    }
    var leave = FAR;
    if (climb < -1e-7) { leave = -altitude / climb; } else if (climb > 1e-7) { leave = (height - altitude) / climb; }
    let start = max(enter, 1e-4);
    let end = min(leave, limit);
    if (start >= end) { return FAR; }
    let width = f32(texels[3u * m.displacement + 1u]);
    let texel = 18.0 / max(max(abs(m.u_scale), abs(m.v_scale)), 1e-6) / max(width, 1.0);
    let across = max(sqrt(max(1.0 - climb * climb, 0.0)), 1e-3);
    let step = max(0.5 * texel / across, (end - start) / 1500.0);
    let uv0 = vec2<f32>(dot(o - plane.center, plane.ay), dot(o - plane.center, plane.az)) / 18.0;
    let duv = vec2<f32>(dot(d, plane.ay), dot(d, plane.az)) / 18.0;
    let scale = vec2<f32>(m.u_scale, m.v_scale);
    var t = start;
    // A ray from above the slab enters at its top, above the relief even where the map is white
    // and rounding would put the point a hair under it; one starting inside must first get out.
    var was_above = altitude > height || ground_above(m, altitude, climb, uv0, duv, scale, t) > 0.0;
    for (var k = 0; k < 4096 && t < end; k++) {
        let next = min(t + step, end);
        let now_above = ground_above(m, altitude, climb, uv0, duv, scale, next) > 0.0;
        if (was_above && !now_above) {
            var low = t;
            var high = next;
            for (var b = 0; b < 12; b++) {
                let middle = 0.5 * (low + high);
                if (ground_above(m, altitude, climb, uv0, duv, scale, middle) > 0.0) { low = middle; } else { high = middle; }
            }
            if (high >= limit) { return FAR; }
            return high;
        }
        was_above = now_above;
        t = next;
    }
    // The relief never goes below the base plane, so a ray that reaches it meets the surface
    // there, as on the CPU; where the map is black, rounding could otherwise let it through.
    if (was_above && climb < -1e-7 && leave < limit) { return leave; }
    return FAR;
}

// How far above the relief the point at `t` along the ray is.
fn ground_above(m: Material, altitude: f32, climb: f32, uv0: vec2<f32>, duv: vec2<f32>, scale: vec2<f32>, t: f32) -> f32 {
    return altitude + climb * t - m.displacement_height * map_height(m.displacement, (uv0 + duv * t) * scale);
}

fn blocked(o: vec3<f32>, d: vec3<f32>, limit: f32) -> bool {
    return intersect(o, d, limit, true).t < limit;
}

// Turns a world direction into the map's own, as the CPU's rotate(d, -1), or back with sign 1.
fn env_rotate(d: vec3<f32>, sign: f32) -> vec3<f32> {
    let c = cos(params.env_rotation);
    let s = sign * sin(params.env_rotation);
    return vec3<f32>(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

fn env_texel(x: i32, y: i32) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(env));
    return textureLoad(env, vec2<i32>(((x % size.x) + size.x) % size.x, clamp(y, 0, size.y - 1)), 0).rgb;
}

// The environment's light from direction `d`, filtered bilinearly as on the CPU.
fn environment(d: vec3<f32>) -> vec3<f32> {
    if (!HAS_ENV || params.has_env == 0u) { return vec3<f32>(0.0); }
    let local = env_rotate(d, -1.0);
    let size = vec2<f32>(textureDimensions(env));
    let u = fract(atan2(local.x, local.z) / (2.0 * PI) + 0.5);
    let v = acos(clamp(local.y, -1.0, 1.0)) / PI;
    let x = u * size.x - 0.5;
    let y = v * size.y - 0.5;
    let ix = i32(floor(x));
    let iy = i32(floor(y));
    let t = vec2<f32>(x - floor(x), y - floor(y));
    let top = mix(env_texel(ix, iy), env_texel(ix + 1, iy), t.x);
    let bottom = mix(env_texel(ix, iy + 1), env_texel(ix + 1, iy + 1), t.x);
    return mix(top, bottom, t.y) * params.env_intensity;
}

fn table(i: u32) -> f32 {
    return bitcast<f32>(texels[params.env_tables + i]);
}

// The last index in [first, first + count) whose table value is at most `value`.
fn search(first: u32, count: u32, value: f32) -> u32 {
    var low = 0u;
    var high = count - 1u;
    while (low < high) {
        let middle = (low + high + 1u) / 2u;
        if (table(first + middle) <= value) { low = middle; } else { high = middle - 1u; }
    }
    return low;
}

// A direction drawn by the environment's light, as the CPU's EnvironmentMap::sample: the world
// direction, and its density over solid angle in w.
fn sample_environment(u: f32, v: f32) -> vec4<f32> {
    let size = textureDimensions(env);
    let w = size.x;
    let h = size.y;
    let y = min(search(0u, h + 1u, v), h - 1u);
    let row_start = table(y);
    let row_mass = table(y + 1u) - row_start;
    if (row_mass <= 0.0) { return vec4<f32>(0.0); }
    let latitude = clamp((v - row_start) / row_mass, 0.000001, 0.999999);
    let row = h + 1u + y * (w + 1u);
    let x = min(search(row, w + 1u, u), w - 1u);
    let mass = table(row + x + 1u) - table(row + x);
    if (mass <= 0.0) { return vec4<f32>(0.0); }
    let longitude = clamp((u - table(row + x)) / mass, 0.000001, 0.999999);
    let phi = 2.0 * PI * (f32(x) + longitude) / f32(w);
    let a = cos(PI * f32(y) / f32(h));
    let b = cos(PI * f32(y + 1u) / f32(h));
    let cosine = a + (b - a) * latitude;
    let sine = sqrt(max(0.0, 1.0 - cosine * cosine));
    let local = vec3<f32>(-sine * sin(phi), cosine, -sine * cos(phi));
    let solid_angle = 2.0 * PI / f32(w) * (a - b);
    return vec4<f32>(env_rotate(local, 1.0), row_mass * mass / solid_angle);
}

fn basis(n: vec3<f32>) -> mat3x3<f32> {
    var helper = vec3<f32>(1.0, 0.0, 0.0);
    if (abs(n.x) > 0.9) { helper = vec3<f32>(0.0, 1.0, 0.0); }
    let t = normalize(cross(helper, n));
    return mat3x3<f32>(t, cross(n, t), n);
}

fn cosine_direction(n: vec3<f32>) -> vec3<f32> {
    let u = rand();
    let angle = 2.0 * PI * rand();
    let r = sqrt(u);
    return basis(n) * vec3<f32>(r * cos(angle), r * sin(angle), sqrt(max(0.0, 1.0 - u)));
}

// What a surface sends back along `wo` of light arriving along `l`, per unit of the light's
// estimate (the CPU's radiance times PI): GGX by its BRDF; the 2008 material by its diffuse
// part `albedo` and highlight of strength `spec`. `light_pdf` > 0 is the light sample's density,
// for MIS with GGX reflections drawn with chance `reflect_chance`.
fn respond(m: Material, albedo: vec3<f32>, spec: f32, n: vec3<f32>, ng: vec3<f32>, wo: vec3<f32>, l: vec3<f32>,
    light_pdf: f32, reflect_chance: f32) -> vec3<f32> {
    // A fog particle (MEDIUM), whose anisotropy is in roughness: the phase function instead.
    if (m.kind == MEDIUM) { return vec3<f32>(phase(-dot(l, wo), m.roughness)); }
    let cosine = dot(l, n);
    if (cosine <= 0.0 || dot(l, ng) <= 0.0) { return vec3<f32>(0.0); }
    if (HAS_GGX && m.kind == 1u) {
        // What the specular lobes would bring, the photons already carry.
        if (diffuse_only) {
            let sum = wo + l;
            if (dot(n, wo) <= 0.0 || dot(sum, sum) < 1e-12) { return vec3<f32>(0.0); }
            return ggx_diffuse(m, n, wo, l, normalize(sum), cosine);
        }
        var pdf = -1.0;
        if (light_pdf > 0.0 && reflect_chance > 0.0) { pdf = light_pdf / reflect_chance; }
        return ggx_evaluate(m, n, wo, l, pdf);
    }
    var r = albedo * (cosine / PI);
    if (spec > 0.0) {
        let h = dot(-wo, reflect(l, n));
        if (h > 0.0) { r += vec3<f32>(h * spec / ((50.0 - 49.0 * h) * PI)); }
    }
    return r;
}

// The light a point at `pos` sends back along `wo` from every light, as the CPU's direct_light.
// 2008 lights are all sampled; with more than four panels and bulbs, one of those is picked by
// the light it likely brings and divided by its chance. The environment is drawn by its light.
fn direct(pos: vec3<f32>, m: Material, albedo: vec3<f32>, spec: f32, n: vec3<f32>, ng: vec3<f32>, wo: vec3<f32>,
    reflect_chance: f32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    diffuse_only = photons_carry;
    var emitters = 0u;
    for (var i = 0u; i < params.light_count; i++) {
        if (lights[i].kind == 0u) { sum += from_light(i, 1.0, pos, m, albedo, spec, n, ng, wo, reflect_chance); }
        else { emitters++; }
    }
    if (HAS_ENV && params.has_env != 0u && (m.kind == 1u || any(albedo > vec3<f32>(0.0)))) {
        let drawn = sample_environment(rand(), rand());
        diffuse_only = photons_carry && params.photon_env != 0u;
        if (drawn.w > 0.0) {
            // No 2008 highlight from the sky, as on the CPU.
            let f = respond(m, albedo, 0.0, n, ng, wo, drawn.xyz, drawn.w, reflect_chance);
            if (any(f > vec3<f32>(0.0))) {
                sum += f * environment(drawn.xyz) * visibility(pos + ng * OFFSET, drawn.xyz, FAR) / drawn.w;
            }
        }
        diffuse_only = photons_carry;
    }
    if (emitters <= 4u) {
        for (var i = 0u; i < params.light_count; i++) {
            if (lights[i].kind != 0u) { sum += from_light(i, 1.0, pos, m, albedo, spec, n, ng, wo, reflect_chance); }
        }
        return sum;
    }
    var total = 0.0;
    for (var i = 0u; i < params.light_count; i++) { total += emitter_weight(i, pos); }
    if (!(total > 0.0)) { return sum; }
    var left = rand() * total;
    var picked = params.light_count;
    var chance = 0.0;
    for (var i = 0u; i < params.light_count; i++) {
        let weight = emitter_weight(i, pos);
        if (weight <= 0.0) { continue; }
        picked = i;
        chance = weight / total;
        left -= weight;
        if (left < 0.0) { break; }
    }
    if (picked < params.light_count) { sum += from_light(picked, chance, pos, m, albedo, spec, n, ng, wo, reflect_chance); }
    return sum;
}

// Roughly how much light panel or bulb `i` sends to `pos`, as the CPU's emitter_weight; 0 for a
// 2008 light, which is never picked.
fn emitter_weight(i: u32, pos: vec3<f32>) -> f32 {
    let light = lights[i];
    let luminance = max(dot(light.color, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0);
    let to_point = pos - light.pos;
    if (light.kind == 2u) {
        let distance2 = max(max(dot(to_point, to_point), 0.25 * light.area), 1e-6);
        let facing = clamp(dot(light.normal, to_point) / sqrt(distance2) + 0.1, 0.0, 1.1);
        return luminance * light.area * facing / distance2;
    }
    if (light.kind == 1u) {
        let r2 = light.radius * light.radius;
        let distance2 = max(max(dot(to_point, to_point), r2), 1e-6);
        return luminance * PI * r2 * bulb_factor(light, to_point / sqrt(distance2)) / distance2;
    }
    return 0.0;
}

// The chance `direct` samples panel or bulb `i` from `pos`, as the CPU's emitter_probability.
fn emitter_chance(i: u32, pos: vec3<f32>) -> f32 {
    var emitters = 0u;
    var total = 0.0;
    for (var k = 0u; k < params.light_count; k++) {
        if (lights[k].kind != 0u) {
            emitters++;
            total += emitter_weight(k, pos);
        }
    }
    if (emitters <= 4u) { return 1.0; }
    if (!(total > 0.0)) { return 0.0; }
    return emitter_weight(i, pos) / total;
}

// Light `i`, sampled at one point and picked with `chance`, as the surface sends it along `wo`.
fn from_light(i: u32, chance: f32, pos: vec3<f32>, m: Material, albedo: vec3<f32>, spec: f32, n: vec3<f32>,
    ng: vec3<f32>, wo: vec3<f32>, reflect_chance: f32) -> vec3<f32> {
    let start = pos + ng * OFFSET;
    let light = lights[i];
    var target_point = light.pos;
    var w = vec3<f32>(0.0);
    if (light.kind == 2u) {
        target_point = light.pos + light.u * (rand() - 0.5) + light.v * (rand() - 0.5);
    } else {
        let z = 1.0 - 2.0 * rand();
        let r = sqrt(max(0.0, 1.0 - z * z));
        let angle = 2.0 * PI * rand();
        w = vec3<f32>(r * cos(angle), r * sin(angle), z);
        if (dot(w, pos - light.pos) < 0.0) { w = -w; }
        target_point = light.pos + w * light.radius;
    }
    let delta = target_point - start;
    let distance = length(delta);
    let l = delta / distance;
    // The light's estimate per unit of its color: what it brings over the density of the point.
    var estimate = PI;
    if (light.kind == 1u) {
        estimate = 2.0 * PI * light.radius * light.radius * max(dot(w, -l), 0.0) * bulb_factor(light, -l)
            / (distance * distance);
    } else if (light.kind == 2u) {
        estimate = light.area * max(-dot(light.normal, l), 0.0) / (distance * distance);
    }
    if (estimate <= 0.0) { return vec3<f32>(0.0); }
    var light_pdf = -1.0;
    if (light.kind != 0u) { light_pdf = chance / estimate; }
    let f = respond(m, albedo, spec, n, ng, wo, l, light_pdf, reflect_chance);
    if (all(f <= vec3<f32>(0.0))) { return vec3<f32>(0.0); }
    let passed = visibility(start, l, distance * 0.999);
    return f * light.color * passed * (estimate / chance);
}

// The light that gets from `start` along `l` to `limit`: blocked by surfaces, tinted by glass and
// dimmed by fog, as the CPU's visibility.
fn visibility(start: vec3<f32>, l: vec3<f32>, limit: f32) -> vec3<f32> {
    let passed = surface_visibility(start, l, limit);
    if (!HAS_VOLUMES || params.volume_count == 0u || all(passed <= vec3<f32>(0.0))) { return passed; }
    return passed * exp(-optical_depth(start, l, limit));
}

// The light that gets from `start` along `l` to `limit`, as the CPU's surface_visibility:
// lamps and opaque surfaces block it; glass lets its share through, absorbing along the way.
fn surface_visibility(start: vec3<f32>, l: vec3<f32>, limit: f32) -> vec3<f32> {
    if (emitter(start, l, limit).t < limit) { return vec3<f32>(0.0); }
    if (!HAS_GLASS || params.has_glass == 0u) {
        if (blocked(start, l, limit)) { return vec3<f32>(0.0); }
        return vec3<f32>(1.0);
    }
    var o = start;
    var remaining = limit;
    var passed = vec3<f32>(1.0);
    var traveled = 0.0;
    var entered = -1.0;
    // The CPU's MAX_GLASS_CROSSINGS.
    for (var i = 0; i < 6; i++) {
        let hit = intersect(o, l, remaining, false);
        if (hit.t >= remaining) { return passed; }
        let m = materials[hit.material];
        if (m.refr <= 0.0) { return vec3<f32>(0.0); }
        passed *= m.refr / (m.refr + m.diff);
        let here = traveled + hit.t;
        if (dot(hit.ng, l) < 0.0) {
            entered = here;
        } else if (m.absorption_depth > 0.0) {
            passed *= absorption(m, here - max(entered, 0.0));
            entered = -1.0;
        }
        traveled = here + OFFSET;
        o = o + l * (hit.t + OFFSET);
        remaining -= hit.t + OFFSET;
        if (remaining <= 0.0) { return passed; }
    }
    return vec3<f32>(0.0);
}

// Spotlights and IES profiles, as far-core's light_profile.rs: the share of a bulb's light that
// leaves along unit `direction`. lib.rs packs the profile into the bulb's spare fields.
fn bulb_factor(light: Light, direction: vec3<f32>) -> f32 {
    if (!HAS_PROFILES || (light.pad2 & 2u) == 0u) { return 1.0; }
    let cosine = clamp(dot(direction, light.normal), -1.0, 1.0);
    var factor = 1.0;
    if ((light.pad2 & 1u) != 0u) {
        let outer = light.v.x;
        let inner = light.v.y;
        if (cosine <= outer) { return 0.0; }
        if (cosine < inner) { factor = smooth_step((cosine - outer) / (inner - outer)); }
    }
    if (light.pad1 != NO_TEXTURE) {
        let side = cross(light.normal, light.u);
        let flat = direction - light.normal * cosine;
        let horizontal = degrees(atan2(dot(flat, side), dot(flat, light.u)));
        factor *= ies_value(light.pad1, degrees(acos(cosine)), horizontal);
    }
    return factor;
}

fn ies_number(at: u32) -> f32 { return bitcast<f32>(texels[at]); }

// Between the angles at `angles` (count of them), the values at `values`; 0 outside them.
fn ies_interpolate(angles: u32, count: u32, values: u32, at: f32) -> f32 {
    if (at < ies_number(angles) || at > ies_number(angles + count - 1u)) { return 0.0; }
    var after = 1u;
    while (after < count - 1u && ies_number(angles + after) <= at) { after++; }
    let a = ies_number(angles + after - 1u);
    let b = ies_number(angles + after);
    let t = clamp((at - a) / (b - a), 0.0, 1.0);
    return ies_number(values + after - 1u) * (1.0 - t) + ies_number(values + after) * t;
}

// An IES table's scaled intensity at a vertical and a horizontal angle, in degrees, as Ies::value:
// the vertical and horizontal counts, the angles, then a row of vertical values per horizontal.
fn ies_value(table: u32, vertical: f32, horizontal: f32) -> f32 {
    let vertical_count = u32(ies_number(table));
    let horizontal_count = u32(ies_number(table + 1u));
    let verticals = table + 2u;
    let horizontals = verticals + vertical_count;
    let rows = horizontals + horizontal_count;
    let last = ies_number(horizontals + horizontal_count - 1u);
    // The file lists only part of the circle when the fixture is symmetric.
    var h = horizontal - 360.0 * floor(horizontal / 360.0);
    if (last <= 0.0) {
        h = 0.0;
    } else if (last <= 90.0) {
        h = h - 180.0 * floor(h / 180.0);
        if (h > 90.0) { h = 180.0 - h; }
    } else if (last <= 180.0 && h > 180.0) {
        h = 360.0 - h;
    }
    if (horizontal_count == 1u) { return ies_interpolate(verticals, vertical_count, rows, vertical); }
    var after = 0u;
    while (after < horizontal_count && ies_number(horizontals + after) <= h) { after++; }
    if (after == 0u) { return ies_interpolate(verticals, vertical_count, rows, vertical); }
    if (after >= horizontal_count) {
        var row = horizontal_count - 1u;
        if (last >= 360.0 - 1e-3) { row = 0u; }
        return ies_interpolate(verticals, vertical_count, rows + row * vertical_count, vertical);
    }
    let a = ies_number(horizontals + after - 1u);
    let b = ies_number(horizontals + after);
    let t = (h - a) / (b - a);
    return ies_interpolate(verticals, vertical_count, rows + (after - 1u) * vertical_count, vertical) * (1.0 - t)
        + ies_interpolate(verticals, vertical_count, rows + after * vertical_count, vertical) * t;
}

// Dispersion, as far-core's spectral.rs: one lobe of the color matching fit.
fn spectral_lobe(x: f32, center: f32, below: f32, above: f32) -> f32 {
    var width = above;
    if (x < center) { width = below; }
    let t = (x - center) / width;
    return exp(-0.5 * t * t);
}

// The color a path of this wavelength carries, linear sRGB over its average: white on average.
fn spectral_weight(l: f32) -> vec3<f32> {
    let x = 1.056 * spectral_lobe(l, 599.8, 37.9, 31.0) + 0.362 * spectral_lobe(l, 442.0, 16.0, 26.7)
        - 0.065 * spectral_lobe(l, 501.1, 20.4, 26.2);
    let y = 0.821 * spectral_lobe(l, 568.8, 46.9, 40.5) + 0.286 * spectral_lobe(l, 530.9, 16.3, 31.1);
    let z = 1.217 * spectral_lobe(l, 437.0, 11.8, 36.0) + 0.681 * spectral_lobe(l, 459.0, 26.0, 13.8);
    let rgb = max(vec3<f32>(3.2406 * x - 1.5372 * y - 0.4986 * z, -0.9689 * x + 1.8758 * y + 0.0415 * z,
        0.0557 * x - 0.2040 * y + 1.0570 * z), vec3<f32>(0.0));
    return rgb / vec3<f32>(params.white_r, params.white_g, params.white_b);
}

// The refractive index at `wavelength` of glass with index `ior` at the d line and Abbe number
// `abbe`, by Cauchy's equation, as spectral::ior.
fn cauchy_ior(ior: f32, abbe: f32, wavelength: f32) -> f32 {
    let f = 1.0 / (486.1 * 486.1 * 1e-6);
    let c = 1.0 / (656.3 * 656.3 * 1e-6);
    let b = (ior - 1.0) / abbe / (f - c);
    let a = ior - b / (587.6 * 587.6 * 1e-6);
    return a + b / (wavelength * wavelength * 1e-6);
}

// The chance of each lobe that can be traced, by the light it carries, as the CPU's
// lobe_chances: every lobe keeps at least 0.1. A negative weight is a lobe that cannot be
// traced. Diffuse, subsurface and reflection.
fn lobe_chances(diffuse: f32, subsurface: f32, reflection: f32) -> vec3<f32> {
    let weights = vec3<f32>(diffuse, subsurface, reflection);
    let present = weights >= vec3<f32>(0.0);
    let count = dot(select(vec3<f32>(0.0), vec3<f32>(1.0), present), vec3<f32>(1.0));
    if (count == 0.0) { return vec3<f32>(0.0); }
    if (count == 1.0) { return select(vec3<f32>(0.0), vec3<f32>(1.0), present); }
    let clamped = max(weights, vec3<f32>(0.0));
    let total = dot(select(vec3<f32>(0.0), clamped, present), vec3<f32>(1.0));
    var shares = vec3<f32>(1.0 / count);
    if (total > 0.0) { shares = clamped / total; }
    let chances = select(vec3<f32>(0.0), max(shares, vec3<f32>(0.1)), present);
    return chances / dot(chances, vec3<f32>(1.0));
}

// The share of light a scattering inside keeps, from the surface color, as the CPU's
// scattering_albedo.
fn scattering_albedo(color: vec3<f32>) -> vec3<f32> {
    let a = clamp(color, vec3<f32>(0.0), vec3<f32>(0.999));
    let x = vec3<f32>(4.09712) + 4.20863 * a - sqrt(vec3<f32>(9.59217) + 41.6808 * a + 17.7126 * a * a);
    return vec3<f32>(1.0) - x * x;
}

// Where subsurface_walk left the material, facing out, and the light it carried there.
var<private> subsurface_out: vec3<f32>;
var<private> subsurface_carried: vec3<f32>;

// Light entering material `m` at `pos` walks inside it, as the CPU's subsurface: steps of a
// random length set by the radius, each scattering any way and keeping the color's share,
// with Russian roulette after eight. Gives where it leaves, and in w 0 when it never does.
fn subsurface_walk(m: Material, pos: vec3<f32>, ns: vec3<f32>, ng: vec3<f32>, wo: vec3<f32>) -> vec4<f32> {
    let f = ggx_fresnel(m, dot(ns, wo));
    let enters = (1.0 - m.metallic) * (1.0 - max(max(f.x, f.y), f.z)) * m.subsurface;
    if (enters <= 0.0) { return vec4<f32>(0.0); }
    let albedo = scattering_albedo(m.color);
    var dir = cosine_direction(-ns);
    if (dot(dir, ng) >= 0.0) { return vec4<f32>(0.0); }
    var p = pos - ng * OFFSET;
    var carried = vec3<f32>(enters);
    for (var step = 0; step < 128; step++) {
        let length = -log(max(1.0 - rand(), 1e-12)) * m.subsurface_radius;
        let met = intersect(p, dir, length, false);
        if (met.t < length) {
            var out = met.ng;
            if (dot(out, dir) < 0.0) { out = -out; }
            subsurface_out = out;
            subsurface_carried = carried;
            return vec4<f32>(p + dir * met.t, 1.0);
        }
        p += dir * length;
        carried *= albedo;
        let z = 1.0 - 2.0 * rand();
        let ring = sqrt(max(0.0, 1.0 - z * z));
        let angle = 2.0 * PI * rand();
        dir = vec3<f32>(ring * cos(angle), ring * sin(angle), z);
        if (step >= 8) {
            let survival = clamp(max(max(carried.x, carried.y), carried.z), 0.05, 0.95);
            if (rand() >= survival) { return vec4<f32>(0.0); }
            carried /= survival;
        }
    }
    return vec4<f32>(0.0);
}

struct Lamp {
    color: vec3<f32>,
    t: f32,
    // The light's index, or params.light_count for none.
    index: u32,
}

// The nearest panel or bulb the ray reaches before `limit`.
fn emitter(o: vec3<f32>, d: vec3<f32>, limit: f32) -> Lamp {
    var found = Lamp(vec3<f32>(0.0), limit, params.light_count);
    for (var i = 0u; i < params.light_count; i++) {
        let light = lights[i];
        if (light.kind == 1u) {
            let rel = o - light.pos;
            let b = dot(rel, d);
            let disc = b * b - (dot(rel, rel) - light.radius * light.radius);
            if (disc > 0.0) {
                let t = -b - sqrt(disc);
                if (t > OFFSET && t < found.t) { found = Lamp(light.color * bulb_factor(light, -d), t, i); }
            }
        } else if (light.kind == 2u) {
            let denom = dot(d, light.normal);
            if (abs(denom) > 1e-7) {
                let t = dot(light.pos - o, light.normal) / denom;
                if (t > OFFSET && t < found.t) {
                    let p = o + d * t - light.pos;
                    let u = dot(p, light.u) / dot(light.u, light.u);
                    let v = dot(p, light.v) / dot(light.v, light.v);
                    if (abs(u) <= 0.5 && abs(v) <= 0.5) {
                        // Only the front shines; the back still blocks.
                        var color = vec3<f32>(0.0);
                        if (denom < 0.0) { color = light.color; }
                        found = Lamp(color, t, i);
                    }
                }
            }
        }
    }
    return found;
}

// The density light sampling from `origin` gives direction `d` toward lamp `lamp`, met at
// lamp.t from `o`: as the CPU weighs a GGX reflection that meets a lamp.
fn lamp_pdf(lamp: Lamp, origin: vec3<f32>, o: vec3<f32>, d: vec3<f32>) -> f32 {
    let light = lights[lamp.index];
    let chance = emitter_chance(lamp.index, origin);
    if (light.kind == 2u) {
        let cosine = -dot(d, light.normal);
        if (cosine <= 0.0) { return 0.0; }
        return chance * lamp.t * lamp.t / (light.area * cosine);
    }
    let cosine = -dot(d, (o + d * lamp.t - light.pos) / light.radius);
    if (cosine <= 0.0) { return 0.0; }
    return chance * lamp.t * lamp.t / (2.0 * PI * light.radius * light.radius * cosine);
}

// Fog and smoke, as far-core's volume.rs: a medium in a box or a sphere, even, thinning with
// height (`falloff`) or shaped by noise. Where volumes overlap their densities add.

fn volume_shape(i: u32) -> Shape { return shapes[params.volume_first + 2u * i]; }

// The medium: albedo in center; in kind, bit 0 for a falloff and the noise's octaves from bit 8
// (0 for none); ax holds density, anisotropy and the falloff's base height; ay the falloff's
// scale, the noise's scale and coverage; material the noise's seed.
fn volume_medium(i: u32) -> Shape { return shapes[params.volume_first + 2u * i + 1u]; }

fn has_noise(m: Shape) -> bool { return (m.kind >> 8u) != 0u; }

// The part of the ray from `o` along `d`, between 0 and `limit`, inside volume `i`: (near, far),
// empty when near >= far.
fn volume_interval(i: u32, o: vec3<f32>, d: vec3<f32>, limit: f32) -> vec2<f32> {
    let s = volume_shape(i);
    var near = 0.0;
    var far = limit;
    let rel = o - s.center;
    if (s.kind == 1u) {
        let b = dot(rel, d);
        let disc = b * b - (dot(rel, rel) - s.radius * s.radius);
        if (disc <= 0.0) { return vec2<f32>(1.0, 0.0); }
        let root = sqrt(disc);
        near = max(near, -b - root);
        far = min(far, -b + root);
    } else {
        let axes = array<vec3<f32>, 3>(s.ax, s.ay, s.az);
        for (var k = 0; k < 3; k++) {
            let start = dot(rel, axes[k]);
            let speed = dot(d, axes[k]);
            let size = s.half[k];
            if (abs(speed) < 1e-12) {
                if (abs(start) > size) { return vec2<f32>(1.0, 0.0); }
                continue;
            }
            let a = (-size - start) / speed;
            let b = (size - start) / speed;
            near = max(near, min(a, b));
            far = min(far, max(a, b));
        }
    }
    return vec2<f32>(near, far);
}

fn volume_hash(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    var h = (u32(x) * 0x8da6b343u) ^ (u32(y) * 0xd8163841u) ^ (u32(z) * 0xcb1ab31fu) ^ (seed * 0x9e3779b9u);
    h ^= h >> 15u;
    h *= 0x2c1b3c6du;
    h ^= h >> 12u;
    h *= 0x297a2d39u;
    return h ^ (h >> 15u);
}

// Perlin's gradient noise, as volume.rs's gradient_noise.
fn gradient_noise(p: vec3<f32>, seed: u32) -> f32 {
    let cell = floor(p);
    let c = vec3<i32>(cell);
    let t = p - cell;
    let fade = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    var corners: array<f32, 8>;
    for (var k = 0; k < 8; k++) {
        let o = vec3<i32>(k & 1, (k >> 1) & 1, (k >> 2) & 1);
        let h = volume_hash(c.x + o.x, c.y + o.y, c.z + o.z, seed) % 12u;
        // The twelve gradients of volume.rs, in its order.
        var g = vec3<f32>(0.0);
        if (h < 4u) { g = vec3<f32>(select(1.0, -1.0, (h & 1u) == 1u), select(1.0, -1.0, h >= 2u), 0.0); }
        else if (h < 8u) { g = vec3<f32>(select(1.0, -1.0, (h & 1u) == 1u), 0.0, select(1.0, -1.0, h >= 6u)); }
        else { g = vec3<f32>(0.0, select(1.0, -1.0, (h & 1u) == 1u), select(1.0, -1.0, h >= 10u)); }
        corners[k] = dot(g, t - vec3<f32>(o));
    }
    let x00 = mix(corners[0], corners[1], fade.x);
    let x10 = mix(corners[2], corners[3], fade.x);
    let x01 = mix(corners[4], corners[5], fade.x);
    let x11 = mix(corners[6], corners[7], fade.x);
    return mix(mix(x00, x10, fade.y), mix(x01, x11, fade.y), fade.z);
}

fn smooth_step(t: f32) -> f32 { return t * t * (3.0 - 2.0 * t); }

// The density of volume `i` at a point inside it, as Volume::density_at.
fn volume_density(i: u32, p: vec3<f32>) -> f32 {
    let m = volume_medium(i);
    var density = m.ax.x;
    if ((m.kind & 1u) != 0u) { density *= exp(-(p.y - m.ax.z) / m.ay.x); }
    let octaves = m.kind >> 8u;
    if (octaves != 0u) {
        let q = p / m.ay.y;
        var sum = 0.0;
        var amplitude = 1.0;
        var frequency = 1.0;
        var norm = 0.0;
        for (var k = 0u; k < octaves; k++) {
            sum += amplitude * gradient_noise(q * frequency, m.material + k * 7919u);
            norm += amplitude;
            amplitude *= 0.5;
            frequency *= 2.03;
        }
        let n = clamp(0.5 + 1.6 * sum / norm, 0.0, 1.0);
        density *= smooth_step(clamp((n - (1.0 - m.ay.z)) / 0.2 + 0.5, 0.0, 1.0));
        // Thinning out toward the edges, so the box or sphere never shows.
        let s = volume_shape(i);
        let rel = p - s.center;
        if (s.kind == 1u) {
            density *= smooth_step(clamp((1.0 - length(rel) / s.radius) / 0.35, 0.0, 1.0));
        } else {
            density *= smooth_step(clamp((1.0 - abs(dot(rel, s.ax)) / s.half.x) / 0.35, 0.0, 1.0))
                * smooth_step(clamp((1.0 - abs(dot(rel, s.ay)) / s.half.y) / 0.35, 0.0, 1.0))
                * smooth_step(clamp((1.0 - abs(dot(rel, s.az)) / s.half.z) / 0.35, 0.0, 1.0));
        }
    }
    return density;
}

// The optical depth of volume `i`, which has no noise, between distances a and b inside it.
fn exact_depth(i: u32, o: vec3<f32>, d: vec3<f32>, a: f32, b: f32) -> f32 {
    let m = volume_medium(i);
    if ((m.kind & 1u) == 0u) { return m.ax.x * (b - a); }
    let base = m.ax.z;
    let scale = m.ay.x;
    let at_a = exp(-(o.y + d.y * a - base) / scale);
    if (abs(d.y) < 1e-6) { return m.ax.x * at_a * (b - a); }
    return m.ax.x * scale / d.y * (at_a - exp(-(o.y + d.y * b - base) / scale));
}

// Steps through a crossing of smoke, as volume.rs's MARCH_STEPS and the puff size.
fn march_step(i: u32, crossing: f32) -> f32 {
    return min(crossing / 48.0, volume_medium(i).ay.y * 0.35);
}

// Optical depth of every volume along the ray up to `limit`: light keeps exp(-depth).
fn optical_depth(o: vec3<f32>, d: vec3<f32>, limit: f32) -> f32 {
    var depth = 0.0;
    for (var i = 0u; i < params.volume_count; i++) {
        let span = volume_interval(i, o, d, limit);
        if (span.x >= span.y) { continue; }
        if (!has_noise(volume_medium(i))) {
            depth += exact_depth(i, o, d, span.x, span.y);
            continue;
        }
        let crossing = span.y - span.x;
        let steps = u32(clamp(ceil(crossing / march_step(i, crossing)), 1.0, 1024.0));
        let dt = crossing / f32(steps);
        var sum = 0.0;
        for (var k = 0u; k < steps; k++) {
            sum += volume_density(i, o + d * (span.x + (f32(k) + 0.5) * dt));
        }
        depth += sum * dt;
    }
    return depth;
}

// The distance after `a` where volume `i`, with no noise, adds optical depth `depth`.
fn invert_exact(i: u32, o: vec3<f32>, d: vec3<f32>, a: f32, b: f32, depth: f32) -> f32 {
    let m = volume_medium(i);
    let share = a + depth / max(exact_depth(i, o, d, a, b), 1e-30) * (b - a);
    if ((m.kind & 1u) == 0u) { return a + depth / m.ax.x; }
    let base = m.ax.z;
    let scale = m.ay.x;
    let at_a = exp(-(o.y + d.y * a - base) / scale);
    if (abs(d.y) < 1e-6) { return a + depth / (m.ax.x * at_a); }
    let inner = at_a - depth * d.y / (m.ax.x * scale);
    if (inner <= 0.0) { return share; }
    return clamp((base - scale * log(inner) - o.y) / d.y, a, b);
}

// Where along the ray the optical depth reaches `wanted`, and in which volume the light
// scatters there, picked by its share of the density with `pick`: (distance, volume), the
// distance negative when the ray leaves every volume before `limit` with less depth. As
// volume.rs's march: crossings are cut where volumes begin and end; parts without noise are
// integrated exactly, noise at the middle of short steps.
fn distance_at_depth(o: vec3<f32>, d: vec3<f32>, limit: f32, wanted: f32, pick: f32) -> vec2<f32> {
    var near: array<f32, 8>;
    var far: array<f32, 8>;
    var stops: array<f32, 16>;
    var count = 0u;
    for (var i = 0u; i < params.volume_count; i++) {
        let span = volume_interval(i, o, d, limit);
        near[i] = span.x;
        far[i] = span.y;
        if (span.x < span.y && volume_medium(i).ax.x > 0.0) {
            stops[count] = span.x;
            stops[count + 1u] = span.y;
            count += 2u;
        } else {
            far[i] = span.x - 1.0;
        }
    }
    for (var k = 1u; k < count; k++) {
        let value = stops[k];
        var j = k;
        while (j > 0u && stops[j - 1u] > value) {
            stops[j] = stops[j - 1u];
            j--;
        }
        stops[j] = value;
    }
    var depth = 0.0;
    for (var k = 0u; k + 1u < count; k++) {
        let start = stops[k];
        let end = stops[k + 1u];
        if (end <= start) { continue; }
        let middle = 0.5 * (start + end);
        var inside = 0u;
        var exact = true;
        var step = FAR;
        var only = 0u;
        for (var i = 0u; i < params.volume_count; i++) {
            if (near[i] <= middle && middle < far[i]) {
                inside++;
                only = i;
                if (has_noise(volume_medium(i))) {
                    exact = false;
                    step = min(step, march_step(i, far[i] - near[i]));
                }
            }
        }
        if (inside == 0u) { continue; }
        var steps = 1u;
        if (!exact) { steps = u32(clamp(ceil((end - start) / step), 1.0, 1024.0)); }
        let dt = (end - start) / f32(steps);
        for (var s = 0u; s < steps; s++) {
            let a = start + f32(s) * dt;
            let b = a + dt;
            let p = o + d * (0.5 * (a + b));
            var added = 0.0;
            var total = 0.0;
            for (var i = 0u; i < params.volume_count; i++) {
                if (!(near[i] <= middle && middle < far[i])) { continue; }
                let density = volume_density(i, p);
                total += density;
                if (exact) { added += exact_depth(i, o, d, a, b); } else { added += density * (b - a); }
            }
            if (depth + added >= wanted && added > 0.0) {
                var t = a + (wanted - depth) / added * (b - a);
                if (exact && inside == 1u) { t = invert_exact(only, o, d, a, b, wanted - depth); }
                var left = pick * total;
                var chosen = only;
                for (var i = 0u; i < params.volume_count; i++) {
                    if (!(near[i] <= middle && middle < far[i])) { continue; }
                    chosen = i;
                    left -= volume_density(i, p);
                    if (left < 0.0) { break; }
                }
                return vec2<f32>(t, f32(chosen));
            }
            depth += added;
        }
    }
    return vec2<f32>(-1.0, 0.0);
}

// Henyey-Greenstein: the density of scattering by an angle of cosine `cosine`, as volume.rs's phase.
fn phase(cosine: f32, g: f32) -> f32 {
    let denom = max(1.0 + g * g - 2.0 * g * cosine, 1e-6);
    return (1.0 - g * g) / (4.0 * PI * denom * sqrt(denom));
}

// A direction scattered from travel direction `d`, distributed by the phase function.
fn sample_phase(d: vec3<f32>, g: f32, u: f32, v: f32) -> vec3<f32> {
    var cosine = 1.0 - 2.0 * u;
    if (abs(g) >= 1e-3) {
        let s = (1.0 - g * g) / (1.0 - g + 2.0 * g * u);
        cosine = clamp((1.0 + g * g - s * s) / (2.0 * g), -1.0, 1.0);
    }
    let sine = sqrt(max(0.0, 1.0 - cosine * cosine));
    let angle = 2.0 * PI * v;
    let frame = basis(d);
    return frame[0] * (sine * cos(angle)) + frame[1] * (sine * sin(angle)) + d * cosine;
}

// The light a particle of volume `i` at `pos`, met by a ray going along `d`, scatters back along
// the ray from every light, before its albedo, as the CPU's in_scattered without the rebound.
fn in_scattered(i: u32, pos: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    var particle: Material;
    particle.kind = MEDIUM;
    particle.roughness = volume_medium(i).ax.y;
    return direct(pos, particle, vec3<f32>(1.0), 0.0, vec3<f32>(0.0), vec3<f32>(0.0), -d, 0.0);
}

// The share of light glass of material `m` keeps over `distance`, as the CPU's absorption.
fn absorption(m: Material, distance: f32) -> vec3<f32> {
    let along = min(distance, 100.0);
    if (m.absorption_depth > 0.0) {
        return exp(log(max(m.color, vec3<f32>(1e-4))) * along / m.absorption_depth);
    }
    // ABSORPTION in far-core.
    return exp((vec3<f32>(1.0) - m.color) * (-0.15 * along));
}

fn schlick(cosine: f32, f0: f32) -> f32 {
    return f0 + (1.0 - f0) * pow(1.0 - clamp(cosine, 0.0, 1.0), 5.0);
}

// far-core's ggx.rs, isotropic, without the clear coat, sheen and subsurface.
fn ggx_alpha(roughness: f32) -> f32 {
    return max(roughness * roughness, 0.001);
}

fn ggx_fresnel(m: Material, cosine: f32) -> vec3<f32> {
    let dielectric = pow((m.ior - 1.0) / (m.ior + 1.0), 2.0);
    let f0 = vec3<f32>(dielectric * (1.0 - m.metallic)) + m.color * m.metallic;
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - clamp(cosine, 0.0, 1.0), 5.0);
}

// Smith masking of direction `v`, for an isotropic surface (rough glass).
fn ggx_g1(a: f32, n: vec3<f32>, v: vec3<f32>) -> f32 {
    let z = dot(n, v);
    if (z <= 0.0) { return 0.0; }
    let slope = a * a * max(1.0 - z * z, 0.0) / (z * z);
    return 2.0 / (1.0 + sqrt(1.0 + slope));
}

// The surface direction at the hit being shaded, which anisotropic highlights follow: where the
// texture's u grows, as the CPU's texture_tangent; zero when unknown.
var<private> surface_tangent: vec3<f32>;

// A GGX lobe in its own frame, as far-core's ggx.rs: the tangent and bitangent around the
// normal, and the highlight's widths along them.
struct Lobe { t: vec3<f32>, b: vec3<f32>, ax: f32, ay: f32 }

// The base lobe of material `m` at normal `n`, as Ggx::frame and Ggx::alphas.
fn base_lobe(m: Material, n: vec3<f32>) -> Lobe {
    let a = ggx_alpha(m.roughness);
    if (!HAS_COATS || m.anisotropy == 0.0) {
        let frame = basis(n);
        return Lobe(frame[0], frame[1], a, a);
    }
    var t = surface_tangent - n * dot(n, surface_tangent);
    if (dot(t, t) < 1e-8) {
        var helper = vec3<f32>(1.0, 0.0, 0.0);
        if (abs(n.z) < 0.999) { helper = vec3<f32>(0.0, 0.0, 1.0); }
        t = cross(helper, n);
    }
    t = normalize(t);
    var b = cross(n, t);
    if (m.rotation != 0.0) {
        t = t * cos(m.rotation) + b * sin(m.rotation);
        b = cross(n, t);
    }
    let aspect = sqrt(1.0 - 0.9 * abs(m.anisotropy));
    let along = max(a / aspect, 0.001);
    let across = max(a * aspect, 0.001);
    if (m.anisotropy > 0.0) { return Lobe(t, b, along, across); }
    return Lobe(t, b, across, along);
}

// The clear coat: a smooth isotropic lobe of its own.
fn coat_lobe(m: Material, n: vec3<f32>) -> Lobe {
    let frame = basis(n);
    let a = ggx_alpha(m.clearcoat_roughness);
    return Lobe(frame[0], frame[1], a, a);
}

fn to_lobe(l: Lobe, n: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(v, l.t), dot(v, l.b), dot(v, n));
}

fn lobe_g1(l: Lobe, v: vec3<f32>) -> f32 {
    if (v.z <= 0.0) { return 0.0; }
    let slope = (v.x * v.x * l.ax * l.ax + v.y * v.y * l.ay * l.ay) / (v.z * v.z);
    return 2.0 / (1.0 + sqrt(1.0 + slope));
}

fn lobe_distribution(l: Lobe, h: vec3<f32>) -> f32 {
    let z = min(h.z, 1.0);
    let d = h.x * h.x / (l.ax * l.ax) + h.y * h.y / (l.ay * l.ay) + z * z;
    return 1.0 / (PI * l.ax * l.ay * d * d);
}

// ggx.rs's COAT_F0: varnish of IOR 1.5.
fn coat_fresnel(cosine: f32) -> f32 {
    return 0.04 + 0.96 * pow(1.0 - clamp(cosine, 0.0, 1.0), 5.0);
}

// The share of light the clear coat leaves for the base, seen from `wo`.
fn under_coat(m: Material, n: vec3<f32>, wo: vec3<f32>) -> f32 {
    if (!HAS_COATS) { return 1.0; }
    return 1.0 - m.clearcoat * coat_fresnel(dot(n, wo));
}

// The diffuse lobes, sheen included, times the incoming cosine `ni`, as Ggx::evaluate_diffuse.
fn ggx_diffuse(m: Material, n: vec3<f32>, wo: vec3<f32>, wi: vec3<f32>, h: vec3<f32>, ni: f32) -> vec3<f32> {
    let f = ggx_fresnel(m, dot(wo, h));
    // The share that walks under the surface instead is handled apart.
    var diffuse = (vec3<f32>(1.0) - f) * m.color * ((1.0 - m.metallic) * (1.0 - m.subsurface) * ni / PI);
    if (HAS_COATS && m.sheen > 0.0) {
        diffuse += m.sheen_color * (m.sheen * (1.0 - m.metallic) * pow(1.0 - clamp(dot(wi, h), 0.0, 1.0), 5.0) * ni);
    }
    return diffuse * under_coat(m, n, wo);
}

// The chance ggx_sample picks the clear coat rather than the base, as Ggx::coat_chance.
fn coat_chance(m: Material, n: vec3<f32>, wo: vec3<f32>) -> f32 {
    if (!HAS_COATS || m.clearcoat <= 0.0) { return 0.0; }
    let cosine = dot(n, wo);
    let coat = m.clearcoat * coat_fresnel(cosine);
    let f = ggx_fresnel(m, cosine);
    let base = max(max(f.x, f.y), f.z) * (1.0 - coat);
    if (coat + base <= 0.0) { return 0.0; }
    return clamp(coat / (coat + base), 0.05, 0.95);
}

// Density of visible-normal sampling of one lobe toward `wi`.
fn lobe_pdf(l: Lobe, n: vec3<f32>, wo: vec3<f32>, wi: vec3<f32>) -> f32 {
    let no = dot(n, wo);
    let sum = wo + wi;
    if (no <= 0.0 || dot(n, wi) <= 0.0 || dot(sum, sum) < 1e-12) { return 0.0; }
    let h = normalize(sum);
    return lobe_distribution(l, to_lobe(l, n, h)) * lobe_g1(l, to_lobe(l, n, wo)) / (4.0 * no);
}

// The density of ggx_sample toward `wi`, coat and base together, in solid angle.
fn ggx_pdf(m: Material, n: vec3<f32>, wo: vec3<f32>, wi: vec3<f32>) -> f32 {
    let base = lobe_pdf(base_lobe(m, n), n, wo, wi);
    let chance = coat_chance(m, n, wo);
    if (chance <= 0.0) { return base; }
    return chance * lobe_pdf(coat_lobe(m, n), n, wo, wi) + (1.0 - chance) * base;
}

// BRDF times the incoming cosine, as Ggx::evaluate. With `light_pdf` > 0, the specular part is
// weighed against the reflection ray by MIS.
fn ggx_evaluate(m: Material, n: vec3<f32>, wo: vec3<f32>, wi: vec3<f32>, light_pdf: f32) -> vec3<f32> {
    let no = dot(n, wo);
    let ni = dot(n, wi);
    let sum = wo + wi;
    if (no <= 0.0 || ni <= 0.0 || dot(sum, sum) < 1e-12) { return vec3<f32>(0.0); }
    let h = normalize(sum);
    let l = base_lobe(m, n);
    let lo = to_lobe(l, n, wo);
    let li = to_lobe(l, n, wi);
    var specular = ggx_fresnel(m, dot(wo, h)) * (lobe_distribution(l, to_lobe(l, n, h)) * lobe_g1(l, lo) * lobe_g1(l, li)
        / (4.0 * lo.z)) * under_coat(m, n, wo);
    if (HAS_COATS && m.clearcoat > 0.0) {
        let c = coat_lobe(m, n);
        let co = to_lobe(c, n, wo);
        specular += vec3<f32>(m.clearcoat * coat_fresnel(dot(wo, h)) * lobe_distribution(c, to_lobe(c, n, h))
            * lobe_g1(c, co) * lobe_g1(c, to_lobe(c, n, wi)) / (4.0 * co.z));
    }
    var mis = 1.0;
    if (light_pdf > 0.0) { mis = power_weight(light_pdf, ggx_pdf(m, n, wo, wi)); }
    return ggx_diffuse(m, n, wo, wi, h, ni) + specular * mis;
}

fn power_weight(a: f32, b: f32) -> f32 {
    if (a <= 0.0) { return 0.0; }
    let ratio = b / a;
    return 1.0 / (1.0 + ratio * ratio);
}

// A microfacet normal of lobe `l` visible from `wo` (Heitz 2018), as the CPU's sample_normal.
fn lobe_normal(l: Lobe, n: vec3<f32>, wo: vec3<f32>, u: f32, v: f32) -> vec3<f32> {
    let view = normalize(vec3<f32>(l.ax * dot(wo, l.t), l.ay * dot(wo, l.b), dot(n, wo)));
    let side = view.x * view.x + view.y * view.y;
    var t1 = vec3<f32>(1.0, 0.0, 0.0);
    if (side > 1e-12) { t1 = vec3<f32>(-view.y, view.x, 0.0) / sqrt(side); }
    let t2 = cross(view, t1);
    let radius = sqrt(clamp(u, 0.0, 1.0));
    let angle = 2.0 * PI * v;
    let p1 = radius * cos(angle);
    let visible = 0.5 * (1.0 + view.z);
    let p2 = (1.0 - visible) * sqrt(max(0.0, 1.0 - p1 * p1)) + visible * radius * sin(angle);
    let normal = t1 * p1 + t2 * p2 + view * sqrt(max(0.0, 1.0 - p1 * p1 - p2 * p2));
    let h = l.t * (l.ax * normal.x) + l.b * (l.ay * normal.y) + n * max(normal.z, 0.0);
    if (dot(h, h) < 1e-12) { return n; }
    return normalize(h);
}

// An isotropic visible normal, for rough glass.
fn ggx_normal(a: f32, n: vec3<f32>, wo: vec3<f32>, u: f32, v: f32) -> vec3<f32> {
    let frame = basis(n);
    return lobe_normal(Lobe(frame[0], frame[1], a, a), n, wo, u, v);
}

// A specular bounce, as Ggx::sample: the clear coat or the base, each by its visible
// microfacets. Gives the direction and, in w, 0 when there is none; the weight in `weight`.
fn ggx_sample(m: Material, n: vec3<f32>, wo: vec3<f32>, weight: ptr<function, vec3<f32>>) -> vec4<f32> {
    let chance = coat_chance(m, n, wo);
    let on_coat = rand() < chance;
    var l = base_lobe(m, n);
    if (on_coat) { l = coat_lobe(m, n); }
    let h = lobe_normal(l, n, wo, rand(), rand());
    let oh = dot(wo, h);
    let wi = h * (2.0 * oh) - wo;
    if (oh <= 0.0 || dot(n, wi) <= 0.0) { return vec4<f32>(0.0); }
    let g1 = lobe_g1(l, to_lobe(l, n, wi));
    if (on_coat) {
        *weight = vec3<f32>(m.clearcoat * coat_fresnel(oh) * g1 / chance);
    } else {
        *weight = ggx_fresnel(m, oh) * (g1 * under_coat(m, n, wo) / (1.0 - chance));
    }
    return vec4<f32>(wi, 1.0);
}

// The surface direction at `pos`: where the texture's u grows, as bump.rs's texture_tangent.
fn texture_tangent(prim: u32, m: Material, pos: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let at = uv_on(prim, pos, m);
    let frame = basis(normal);
    let r1 = uv_rate(prim, m, pos, frame[0], at);
    let r2 = uv_rate(prim, m, pos, frame[1], at);
    if (r1.z == 0.0 || r2.z == 0.0) { return vec3<f32>(0.0); }
    let det = r1.x * r2.y - r2.x * r1.y;
    if (abs(det) < 1e-12) { return vec3<f32>(0.0); }
    let along_u = (frame[0] * r2.y - frame[1] * r1.y) / det;
    if (length(along_u) < 1e-12) { return vec3<f32>(0.0); }
    return normalize(along_u);
}

// The environment's density for direction `d`, as the CPU's EnvironmentMap::pdf.
fn environment_pdf(d: vec3<f32>) -> f32 {
    let local = env_rotate(d, -1.0);
    let size = textureDimensions(env);
    let u = fract(atan2(local.x, local.z) / (2.0 * PI) + 0.5);
    let v = acos(clamp(local.y, -1.0, 1.0)) / PI;
    let x = min(u32(u * f32(size.x)), size.x - 1u);
    let y = min(u32(v * f32(size.y)), size.y - 1u);
    let row = size.y + 1u + y * (size.x + 1u);
    let mass = (table(y + 1u) - table(y)) * (table(row + x + 1u) - table(row + x));
    let solid_angle = 2.0 * PI / f32(size.x) * (cos(PI * f32(y) / f32(size.y)) - cos(PI * f32(y + 1u) / f32(size.y)));
    return mass / solid_angle;
}

// Texture coordinates at `pos` on what `hit` met, as the CPU's texture_uv_on reads them, with
// the material's repeats (which triangles do not use).
fn texture_uv(hit: Hit, pos: vec3<f32>, m: Material) -> vec2<f32> {
    if ((hit.prim & 0x80000000u) == 0u) {
        let tri = tris[hit.prim];
        let a = vec2<f32>(tri.u0, tri.v0);
        return a + hit.bary.x * (vec2<f32>(tri.u1, tri.v1) - a) + hit.bary.y * (vec2<f32>(tri.u2, tri.v2) - a);
    }
    let s = shapes[hit.prim & 0x7fffffffu];
    let scale = vec2<f32>(m.u_scale, m.v_scale);
    let rel = pos - s.center;
    if (s.kind == 6u) {
        // GROUND_TILE in far-core: one texture repeat every 18 units.
        return vec2<f32>(dot(rel, s.ay), dot(rel, s.az)) / 18.0 * scale;
    }
    if (s.kind == 0u) {
        // The sphere's poles on Y, its seam toward X, as the CPU's Sphere.
        let vp = normalize(rel);
        let phi = acos(clamp(-vp.y, -1.0, 1.0));
        let v = phi * m.v_scale / PI;
        let theta = acos(clamp(vp.x / max(sin(phi), 1e-6), -1.0, 1.0)) * (2.0 / PI);
        var u = theta * m.u_scale;
        if (-vp.z >= 0.0) { u = (1.0 - theta) * m.u_scale; }
        return vec2<f32>(u, v);
    }
    let p = vec3<f32>(dot(rel, s.ax), dot(rel, s.ay), dot(rel, s.az));
    let around = atan2(p.z, p.x) / (2.0 * PI) + 0.5;
    if (s.kind == 1u) {
        let r = abs(p / s.half);
        var uv = vec2<f32>(p.x / s.half.x, p.y / s.half.y);
        if (r.x >= r.y && r.x >= r.z) { uv = vec2<f32>(p.z / s.half.z, p.y / s.half.y); }
        else if (r.y >= r.z) { uv = vec2<f32>(p.x / s.half.x, p.z / s.half.z); }
        return (0.5 * uv + 0.5) * scale;
    }
    if (s.kind == 5u) {
        let ring_distance = length(p.xz) - s.radius;
        return vec2<f32>(around, atan2(p.y, ring_distance) / (2.0 * PI) + 0.5) * scale;
    }
    return vec2<f32>(around, 0.5 * p.y / s.half.y + 0.5) * scale;
}

fn texel(texture: u32, x: i32, y: i32) -> vec3<f32> {
    let first = texels[3u * texture];
    let w = i32(texels[3u * texture + 1u]);
    let h = i32(texels[3u * texture + 2u]);
    let at = first + u32(((x % w) + w) % w + (((y % h) + h) % h) * w);
    let c = texels[at];
    return vec3<f32>(f32(c & 255u), f32((c >> 8u) & 255u), f32((c >> 16u) & 255u)) / 256.0;
}

// A bilinear lookup, repeating, as the CPU's texel_fixed.
fn texture_color(texture: u32, uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(f32(texels[3u * texture + 1u]), f32(texels[3u * texture + 2u]));
    let f = (uv + vec2<f32>(1000.5, 1000.0)) * size;
    let x = i32(f.x);
    let y = i32(f.y);
    let frac = f - floor(f);
    return mix(mix(texel(texture, x, y), texel(texture, x + 1, y), frac.x),
        mix(texel(texture, x, y + 1), texel(texture, x + 1, y + 1), frac.x), frac.y);
}

// Bump and normal maps, as far-core's bump.rs: the shading normal tilted by the map, read through
// the primitive's texture coordinates. How those coordinates run over the surface is measured
// with two small steps in the tangent plane, which gives the slope and the tangent axes on every
// kind of primitive.

// The texture coordinates of any point of primitive `prim`, as the CPU's texture_uv_on: on a
// triangle, of the point projected onto its plane.
fn uv_on(prim: u32, pos: vec3<f32>, m: Material) -> vec2<f32> {
    if ((prim & 0x80000000u) == 0u) {
        let tri = tris[prim];
        let rel = pos - tri.p0;
        let d11 = dot(tri.e1, tri.e1);
        let d12 = dot(tri.e1, tri.e2);
        let d22 = dot(tri.e2, tri.e2);
        let det = d11 * d22 - d12 * d12;
        if (abs(det) < 1e-20) { return vec2<f32>(0.0); }
        let s1 = (d22 * dot(rel, tri.e1) - d12 * dot(rel, tri.e2)) / det;
        let s2 = (d11 * dot(rel, tri.e2) - d12 * dot(rel, tri.e1)) / det;
        let a = vec2<f32>(tri.u0, tri.v0);
        return a + s1 * (vec2<f32>(tri.u1, tri.v1) - a) + s2 * (vec2<f32>(tri.u2, tri.v2) - a);
    }
    var on: Hit;
    on.prim = prim;
    return texture_uv(on, pos, m);
}

// bump.rs's STEP.
const BUMP_STEP: f32 = 1e-3;

// Change of texture coordinates per scene unit along `dir`, and 1 in z when it could be measured:
// a step across a seam, where the coordinates jump, is measured backward instead.
fn uv_rate(prim: u32, m: Material, pos: vec3<f32>, dir: vec3<f32>, at: vec2<f32>) -> vec3<f32> {
    let forward = uv_on(prim, pos + dir * BUMP_STEP, m) - at;
    if (all(abs(forward) < vec2<f32>(0.25))) { return vec3<f32>(forward / BUMP_STEP, 1.0); }
    let backward = at - uv_on(prim, pos - dir * BUMP_STEP, m);
    if (all(abs(backward) < vec2<f32>(0.25))) { return vec3<f32>(backward / BUMP_STEP, 1.0); }
    return vec3<f32>(0.0);
}

fn map_height(texture: u32, uv: vec2<f32>) -> f32 {
    return dot(texture_color(texture, uv), vec3<f32>(1.0 / 3.0));
}

// `normal`, the stored shading normal of `prim` at `pos`, tilted by material `m`'s bump map.
fn bumped(prim: u32, m: Material, pos: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    return tilt(prim, m, pos, normal, m.bump, m.bump_strength);
}

// `normal` tilted by the height or normal map `map` (NORMAL_MAP set for a normal map) of
// strength `strength`, read through the texture coordinates of `prim` at `pos`, as bump.rs's
// tilt. A height map of a displacement's height gives the normal of the displaced surface.
fn tilt(prim: u32, m: Material, pos: vec3<f32>, normal: vec3<f32>, map: u32, strength: f32) -> vec3<f32> {
    let at = uv_on(prim, pos, m);
    let frame = basis(normal);
    let t1 = frame[0];
    let t2 = frame[1];
    let r1 = uv_rate(prim, m, pos, t1, at);
    let r2 = uv_rate(prim, m, pos, t2, at);
    if (r1.z == 0.0 || r2.z == 0.0) { return normal; }
    let texture = map & ~NORMAL_MAP;
    var tilted = normal;
    if ((map & NORMAL_MAP) == 0u) {
        // The height's slope in texture space, one texel on each side, then along the tangents.
        let su = 1.0 / f32(texels[3u * texture + 1u]);
        let sv = 1.0 / f32(texels[3u * texture + 2u]);
        let hu = (map_height(texture, at + vec2<f32>(su, 0.0)) - map_height(texture, at - vec2<f32>(su, 0.0))) / (2.0 * su);
        let hv = (map_height(texture, at + vec2<f32>(0.0, sv)) - map_height(texture, at - vec2<f32>(0.0, sv))) / (2.0 * sv);
        let g1 = strength * (hu * r1.x + hv * r1.y);
        let g2 = strength * (hu * r2.x + hv * r2.y);
        tilted = normal - (t1 * g1 + t2 * g2);
    } else {
        // The tangent directions in which u and v grow; up in the image, where green points,
        // is against v.
        let det = r1.x * r2.y - r2.x * r1.y;
        if (abs(det) < 1e-12) { return normal; }
        let along_u = (t1 * r2.y - t2 * r1.y) / det;
        let along_v = (t2 * r1.x - t1 * r2.x) / det;
        if (length(along_u) < 1e-12 || length(along_v) < 1e-12) { return normal; }
        let c = texture_color(texture, at);
        let x = (2.0 * c.x - 1.0) * strength;
        let y = (2.0 * c.y - 1.0) * strength;
        tilted = normalize(along_u) * x - normalize(along_v) * y + normal * max(2.0 * c.z - 1.0, 0.0);
    }
    let size = length(tilted);
    if (!(size > 1e-12) || size > 1e30) { return normal; }
    tilted /= size;
    // A strong map must not turn the surface away from where it faces.
    if (dot(tilted, normal) > 0.05) { return tilted; }
    return normal;
}

// Whether the caustic photons carry the light that reaches the surface being lit through
// mirrors, glass and GGX lobes: after a diffuse rebound in a render with caustics, as the
// CPU's photons_carry. Light sampling then leaves out what GGX reflects, `diffuse_only`.
var<private> photons_carry: bool;
var<private> diffuse_only: bool;

fn photon_cell(v: vec3<f32>) -> vec3<i32> {
    return vec3<i32>(floor(v / (2.0 * params.photon_radius)));
}

// The light the photon map holds at `pos` on a surface facing `n`, as far-core's
// PhotonMap::irradiance: the photons within the radius on that surface, with a cone filter.
fn caustic_light(pos: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let r = params.photon_radius;
    let r2 = r * r;
    let low = photon_cell(pos - vec3<f32>(r));
    let high = photon_cell(pos + vec3<f32>(r));
    var sum = vec3<f32>(0.0);
    for (var x = low.x; x <= high.x; x++) {
        for (var y = low.y; y <= high.y; y++) {
            for (var z = low.z; z <= high.z; z++) {
                let hash = (u32(x) * 73856093u) ^ (u32(y) * 19349663u) ^ (u32(z) * 83492791u);
                let b = params.bucket_first + (hash & (params.photon_buckets - 1u));
                for (var i = texels[b]; i < texels[b + 1u]; i++) {
                    let at = params.photon_first + 7u * i;
                    let p = vec3<f32>(bitcast<f32>(texels[at]), bitcast<f32>(texels[at + 1u]), bitcast<f32>(texels[at + 2u]));
                    let offset = p - pos;
                    let d2 = dot(offset, offset);
                    if (d2 >= r2 || abs(dot(offset, n)) >= 0.5 * r) { continue; }
                    if (dot(octahedral(texels[at + 3u]), n) <= 0.7) { continue; }
                    // Different cells can share a bucket; each photon counts once.
                    if (any(photon_cell(p) != vec3<i32>(x, y, z))) { continue; }
                    let power = vec3<f32>(bitcast<f32>(texels[at + 4u]), bitcast<f32>(texels[at + 5u]), bitcast<f32>(texels[at + 6u]));
                    sum += power * (1.0 - d2 / r2);
                }
            }
        }
    }
    // The cone integrates to half a disk, hence the factor of 2.
    return sum * (2.0 / (PI * r2));
}

// A unit vector from 32 bits, as lib.rs's octahedral packs it.
fn octahedral(e: u32) -> vec3<f32> {
    let f = unpack2x16snorm(e);
    var n = vec3<f32>(f.x, f.y, 1.0 - abs(f.x) - abs(f.y));
    if (n.z < 0.0) {
        let sx = select(-1.0, 1.0, n.x >= 0.0);
        let sy = select(-1.0, 1.0, n.y >= 0.0);
        n = vec3<f32>((1.0 - abs(f.y)) * sx, (1.0 - abs(f.x)) * sy, n.z);
    }
    return normalize(n);
}

fn trace(origin: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    var o = origin;
    var d = direction;
    var through = vec3<f32>(1.0);
    var light = vec3<f32>(0.0);
    // Whether light met head-on counts: on camera rays and after mirrors and glass. After a
    // diffuse bounce the lights were already sampled.
    var sees_lights = true;
    var diffuse_bounces = 0u;
    // The glass the ray is inside of, which absorbs light along the way to its next hit.
    var absorbing = NO_TEXTURE;
    var glossy_count = 0u;
    var transmission_count = 0u;
    // After a diffuse rebound, what the light found from there is weighed by: the CPU caps
    // that light at params.clamp, which here caps each part of it.
    var rebound = vec3<f32>(-1.0);
    // After a GGX reflection, the density it was drawn with and where, for MIS with the light
    // sampling there; negative otherwise.
    var mis_pdf = -1.0;
    var mis_from = vec3<f32>(0.0);
    // The product of the chances Russian roulette let the path go on with.
    var luck = 1.0;
    // The wavelength the path follows once it crosses glass that disperses, 0 before.
    var wavelength = 0.0;
    // A path ends at its depth limits, by Russian roulette or at this guard.
    for (var segment = 0u; segment < 160u; segment++) {
        var hit = intersect(o, d, FAR, false);
        if (HAS_GLASS && absorbing != NO_TEXTURE) {
            through *= absorption(materials[absorbing], hit.t);
            absorbing = NO_TEXTURE;
        }
        let lamp = emitter(o, d, hit.t);
        if (HAS_VOLUMES && params.volume_count > 0u && params.caustics_only == 0u) {
            // What the camera ray meets first guides the denoiser even when it scatters in fog.
            if (segment == 0u && hit.t < FAR) {
                first_albedo = materials[hit.material].color;
                first_normal = hit.ns;
                first_depth = hit.t;
            }
            let closest = min(hit.t, lamp.t);
            let fog = optical_depth(o, d, closest);
            if (fog > 0.0) {
                // As the CPU's trace: camera rays, and mirror and glass rays from them, gather the
                // light the fog scatters toward them every time, from a particle drawn among
                // those the ray would first meet, so shafts come out smooth; then the path goes
                // on into the fog or to the surface by the chance light gets through. After a
                // diffuse rebound, the distance drawn decides: a particle, or the surface.
                let kept = exp(-fog);
                let camera = diffuse_bounces == 0u;
                var reach = 1.0;
                if (camera) { reach = 1.0 - kept; }
                let found = distance_at_depth(o, d, closest, -log(max(1.0 - rand() * reach, 1e-30)), rand());
                let may_scatter = diffuse_bounces < params.max_bounces;
                if (found.x >= 0.0) {
                    let v = u32(found.y);
                    let medium = volume_medium(v);
                    let pos = o + d * found.x;
                    var share = 1.0;
                    if (camera) { share = reach; }
                    light += capped(through * medium.center * (share * in_scattered(v, pos, d)), rebound);
                    if (!camera || (may_scatter && rand() >= kept)) {
                        if (!may_scatter) { break; }
                        // A rebound in the fog, by the phase function, weighed as the CPU's.
                        d = sample_phase(d, medium.ax.y, rand(), rand());
                        o = pos;
                        through = survived(through * medium.center * params.strength, &luck, diffuse_bounces);
                        if (all(through <= vec3<f32>(0.0))) { break; }
                        rebound = through;
                        sees_lights = false;
                        mis_pdf = -1.0;
                        diffuse_bounces++;
                        continue;
                    }
                    // On to the surface; drawn with chance `kept` when the fog could have
                    // been followed instead, which makes up for the dimming.
                    if (!may_scatter) { through *= kept; }
                } else if (camera) {
                    through *= kept;
                }
            }
        }
        // With caustics, the light that gets to a diffuse rebound through mirrors and glass
        // comes from the photons instead.
        let caustics = params.photon_buckets != 0u;
        photons_carry = caustics && diffuse_bounces > 0u;
        if (lamp.t < hit.t) {
            if (sees_lights && !photons_carry && params.caustics_only == 0u) {
                var weight = 1.0;
                if (HAS_GGX && mis_pdf > 0.0) { weight = power_weight(mis_pdf, lamp_pdf(lamp, mis_from, o, d)); }
                light += capped(through * lamp.color * weight, rebound);
            }
            break;
        }
        if (hit.t >= FAR) {
            // After a diffuse rebound the environment was drawn already, by its light.
            let carried = photons_carry && params.photon_env != 0u;
            if (sees_lights && !carried && params.caustics_only == 0u && (segment > 0u || params.env_visible != 0u)) {
                var weight = 1.0;
                if (HAS_GGX && mis_pdf > 0.0 && params.has_env != 0u) { weight = power_weight(mis_pdf, environment_pdf(d)); }
                light += capped(through * environment(d) * weight, rebound);
            }
            break;
        }
        var m = materials[hit.material];
        let pos = o + d * hit.t;
        // Where maps and textures are read: the hit, or for a displaced triangle the shape under
        // it, whose normal the displacement map tilts, as the CPU's hit_at.
        var shade_prim = hit.prim;
        var shade_pos = pos;
        var stored = hit.ns;
        var uv = vec2<f32>(0.0);
        var displaced = false;
        if (HAS_DISPLACEMENT && m.displacement != NO_TEXTURE) {
            if ((hit.prim & 0x80000000u) == 0u && tris[hit.prim].displaced != 0u) {
                let face = tris[tris[hit.prim].displaced - 1u];
                let b = hit.bary;
                shade_pos = face.p0 + b.x * (face.e1 - face.p0) + b.y * (face.e2 - face.p0);
                shade_prim = face.material;
                displaced = true;
                stored = normalize(face.n0 + b.x * (face.n1 - face.n0) + b.y * (face.n2 - face.n0));
            }
            stored = tilt(shade_prim, m, shade_pos, stored, m.displacement, m.displacement_height);
        }
        if (displaced) { uv = uv_on(shade_prim, shade_pos, m); } else { uv = texture_uv(hit, pos, m); }
        if (HAS_TEXTURES && m.texture != NO_TEXTURE) { m.color *= texture_color(m.texture, uv); }
        if (HAS_MAPS) {
            // As the CPU's hit_at: the maps scale GGX roughness and metallic, and tint emission.
            if (m.kind == 1u && m.roughness_map != NO_TEXTURE) { m.roughness *= map_height(m.roughness_map, uv); }
            if (m.kind == 1u && m.metallic_map != NO_TEXTURE) { m.metallic *= map_height(m.metallic_map, uv); }
            if (m.emission_map != NO_TEXTURE) { m.emission *= texture_color(m.emission_map, uv); }
        }
        var ng = hit.ng;
        let inside = dot(ng, d) > 0.0;
        if (inside) { ng = -ng; }
        // Whether the ray meets the side the stored normal points to.
        // The shading normal as stored, tilted by a bump or normal map.
        if (HAS_BUMP && m.bump != NO_TEXTURE) { stored = bumped(shade_prim, m, shade_pos, stored); }
        let front = dot(d, stored) < 0.0;
        var ns = stored;
        if (dot(ns, ng) < 0.0) { ns = -ns; }
        if (segment == 0u) {
            first_albedo = m.color;
            first_normal = ns;
            first_depth = hit.t;
        }
        let wo = -d;
        if (params.caustics_only == 0u) { light += capped(through * m.emission, rebound); }
        // The caustic light where the camera sees a diffuse surface, directly or through mirrors
        // and glass, as the CPU gathers it.
        if (caustics && diffuse_bounces == 0u && front) {
            var diff = m.diff;
            if (HAS_GGX && m.kind == 1u) {
                let f = ggx_fresnel(m, dot(ns, wo));
                diff = (1.0 - m.metallic) * (1.0 - max(max(f.x, f.y), f.z));
            }
            if (diff > 0.0) { light += through * m.color * (params.caustic_strength * diff * caustic_light(pos, stored)); }
        }
        mis_pdf = -1.0;
        let specular = glossy_count + transmission_count;
        let may_reflect = specular < params.max_depth && glossy_count < params.glossy_max;
        let may_bounce = diffuse_bounces < params.max_bounces && front;

        if (HAS_GGX && m.kind == 1u) {
            // GGX: a reflection drawn by its visible microfacets, a diffuse rebound, or a walk
            // under the surface, each by its chance as the CPU's lobe_chances gives it.
            if (HAS_COATS && m.anisotropy != 0.0) { surface_tangent = texture_tangent(shade_prim, m, shade_pos, ns); }
            let seen = ggx_fresnel(m, dot(ns, wo));
            let diff = (1.0 - m.metallic) * (1.0 - max(max(seen.x, seen.y), seen.z));
            let reflects = may_reflect && m.refl > 0.0;
            let bounces = may_bounce && diff > 0.0;
            let scatters = HAS_SUBSURFACE && may_bounce && m.subsurface > 0.0;
            let brightest = max(max(m.color.x, m.color.y), m.color.z);
            let chances = lobe_chances(select(-1.0, diff * brightest, bounces),
                select(-1.0, m.subsurface * (1.0 - m.metallic) * brightest, scatters), select(-1.0, m.refl, reflects));
            let reflect_chance = chances.z;
            if (params.caustics_only == 0u) {
                light += capped(through * direct(pos, m, vec3<f32>(0.0), 0.0, ns, ng, wo, reflect_chance), rebound);
            }
            if (!reflects && !bounces && !scatters) { break; }
            let pick = rand();
            if (pick >= chances.x + reflect_chance) {
                // Into the material, as the CPU's subsurface: a random walk that leaves where it
                // meets the surface again, gathering the light there as a white matte face.
                let walked = subsurface_walk(m, pos, ns, ng, wo);
                if (walked.w == 0.0) { break; }
                let exit = walked.xyz;
                let out = subsurface_out;
                through *= subsurface_carried / chances.y;
                diffuse_bounces++;
                var white: Material;
                white.color = vec3<f32>(1.0);
                white.diff = 1.0;
                light += capped(through * direct(exit, white, vec3<f32>(1.0), 0.0, out, out, out, 0.0), rebound);
                if (diffuse_bounces >= params.max_bounces) { break; }
                d = cosine_direction(out);
                o = exit + out * OFFSET;
                through = survived(through * params.strength, &luck, diffuse_bounces);
                if (all(through <= vec3<f32>(0.0))) { break; }
                rebound = through;
                sees_lights = false;
                mis_pdf = -1.0;
                diffuse_bounces++;
                continue;
            }
            if (pick < reflect_chance) {
                var weight = vec3<f32>(0.0);
                let drawn = ggx_sample(m, ns, wo, &weight);
                let wi = drawn.xyz;
                if (drawn.w == 0.0 || dot(wi, ng) <= 0.0) { break; }
                through *= weight / reflect_chance;
                mis_pdf = reflect_chance * ggx_pdf(m, ns, wo, wi);
                mis_from = pos;
                d = wi;
                o = pos + ng * OFFSET;
                glossy_count++;
                sees_lights = true;
                continue;
            }
            d = cosine_direction(ns);
            if (dot(d, ng) <= 0.0) { break; }
            o = pos + ng * OFFSET;
            // The diffuse lobes over the cosine density, sheen and coat included.
            let ni = dot(ns, d);
            if (ni <= 0.0) { break; }
            let weight = ggx_diffuse(m, ns, wo, d, normalize(wo + d), ni) * (PI / ni);
            through = survived(through * weight * (params.strength / chances.x), &luck, diffuse_bounces);
            if (all(through <= vec3<f32>(0.0))) { break; }
            rebound = through;
            sees_lights = false;
            diffuse_bounces++;
            continue;
        }

        // The 2008 material and glass: the light the diffuse part scatters is gathered at every
        // hit; then one way on is drawn, through, mirror-like or diffuse, by the strength of
        // each, with a weight that makes up for the others not being followed.
        var refr = m.refr;
        var refl = m.refl;
        var diff = m.diff;
        var scattered = m.color * diff;
        // The back of a 2008 surface shows only the highlight, as on the CPU.
        if (!front) { scattered = vec3<f32>(0.0); }
        if (params.caustics_only == 0u && (any(scattered > vec3<f32>(0.0)) || m.spec > 0.0)) {
            light += capped(through * direct(pos, m, scattered, m.spec, ns, ng, wo, 0.0), rebound);
        }
        // Each way on that the CPU would not follow here is left out: past the depth limits,
        // past the diffuse rebounds, and the rebound from the back of a surface. The one drawn
        // is divided by its chance, its strength over the sum of them.
        if (specular >= params.max_depth || transmission_count >= params.transmission_max) { refr = 0.0; }
        if (!may_reflect) { refl = 0.0; }
        if (!may_bounce) { diff = 0.0; }
        let strengths = refr + refl + diff;
        if (strengths <= 0.0) { break; }
        let pick = rand() * strengths;
        if (HAS_GLASS && pick < refr) {
            // Through: reflect or refract by Fresnel, as the CPU's refract_at, rough glass at a
            // microfacet seen from the ray. Glass takes no color at its surface; it absorbs
            // along the way inside.
            through *= strengths;
            transmission_count++;
            let a = ggx_alpha(m.roughness);
            var facet = ns;
            if (m.roughness > 0.0) { facet = ggx_normal(a, ns, wo, rand(), rand()); }
            var ior = m.ior;
            if (HAS_DISPERSION && m.abbe > 0.0) {
                // As the CPU's refraction: the path follows one wavelength from the first glass
                // that disperses on, and carries that wavelength's color.
                if (wavelength == 0.0) {
                    wavelength = 380.0 + rand() * 340.0;
                    through *= spectral_weight(wavelength);
                }
                ior = cauchy_ior(m.ior, m.abbe, wavelength);
            }
            var eta = 1.0 / ior;
            if (inside) { eta = ior; }
            let cos_i = -dot(d, facet);
            let k = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
            let f0 = pow((ior - 1.0) / (ior + 1.0), 2.0);
            var cos_f = cos_i;
            if (inside && k > 0.0) { cos_f = sqrt(k); }
            if (k <= 0.0 || rand() < schlick(cos_f, f0)) {
                d = reflect(d, facet);
                if (dot(d, ng) <= 0.0) { break; }
                if (m.roughness > 0.0) { through *= ggx_g1(a, ns, d); }
                o = pos + ng * OFFSET;
                sees_lights = true;
            } else {
                d = normalize(d * eta + facet * (eta * cos_i - sqrt(k)));
                if (dot(d, ng) >= 0.0) { break; }
                if (m.roughness > 0.0) { through *= ggx_g1(a, -ns, d); }
                o = pos - ng * OFFSET;
                if (!inside) { absorbing = hit.material; }
                // After a diffuse bounce, shadow rays already brought light through glass.
                sees_lights = diffuse_bounces == 0u;
            }
            continue;
        }
        if (pick < refr + refl) {
            // Mirror-like, spread over a disk of radius drefl / 2 when glossy, as the CPU's
            // reflected_ray.
            d = reflect(d, ns);
            if (m.roughness > 0.0) {
                let frame = basis(d);
                let radius = 0.5 * m.roughness * sqrt(rand());
                let angle = 2.0 * PI * rand();
                d = normalize(d + frame[0] * (radius * cos(angle)) + frame[1] * (radius * sin(angle)));
            }
            if (dot(d, ng) <= 0.0) { d = reflect(d, ng); }
            o = pos + ng * OFFSET;
            through *= m.color * strengths;
            glossy_count++;
            sees_lights = true;
            continue;
        }
        // A diffuse rebound, scaled by the bounce strength.
        d = cosine_direction(ns);
        if (dot(d, ng) <= 0.0) { break; }
        o = pos + ng * OFFSET;
        through = survived(through * m.color * (strengths * params.strength), &luck, diffuse_bounces);
        if (all(through <= vec3<f32>(0.0))) { break; }
        rebound = through;
        sees_lights = false;
        diffuse_bounces++;
    }
    return light;
}

// Russian roulette from the third diffuse rebound on, as the CPU's ROULETTE_AFTER: a path
// survives by what it still carries, and one that survives is divided by its chance, so the
// average stays exact. The chance comes from what the path carries before those divisions,
// `luck` being their product: the CPU weighs by the last surface alone, which in a white room
// keeps paths going for dozens of rebounds, and on the GPU the longest path holds up its whole
// group. A path that ends comes back black.
fn survived(through: vec3<f32>, luck: ptr<function, f32>, diffuse_bounces: u32) -> vec3<f32> {
    if (diffuse_bounces < 3u) { return through; }
    let carried = through * *luck;
    let survive = clamp(max(max(carried.x, carried.y), carried.z), 0.05, 0.95);
    if (rand() >= survive) { return vec3<f32>(0.0); }
    *luck *= survive;
    return through / survive;
}

// `light` capped as the CPU caps what a diffuse rebound brings back, where `rebound` is what
// that light is weighed by; no cap before the first rebound.
fn capped(light: vec3<f32>, rebound: vec3<f32>) -> vec3<f32> {
    if (rebound.x < 0.0) { return light; }
    return min(light, rebound * params.clamp);
}

// A point of the lens opening, as the CPU's Camera::lens_point: a disk, or a polygon of
// `blades` sides with a flat edge at the bottom at rotation 0.
fn lens_point(u: f32, v: f32) -> vec2<f32> {
    let radius = 0.5 * params.aperture;
    if (params.blades < 3u) {
        let r = radius * sqrt(u);
        let angle = 2.0 * PI * v;
        return vec2<f32>(r * cos(angle), r * sin(angle));
    }
    let n = f32(params.blades);
    let side = min(u32(u * n), params.blades - 1u);
    let t = u * n - f32(side);
    let step = 2.0 * PI / n;
    let start = params.blade_rotation - 0.5 * PI - 0.5 * step + f32(side) * step;
    let a = radius * vec2<f32>(cos(start), sin(start));
    let b = radius * vec2<f32>(cos(start + step), sin(start + step));
    return sqrt(t) * mix(a, b, v);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.width || id.y >= params.height) { return; }
    let pixel = id.y * params.width + id.x;
    rng = hash(pixel ^ hash(params.pass_index + 1u));
    // A tent filter over the pixel, as the CPU uses.
    let a = rand();
    let b = rand();
    let jx = select(1.0 - sqrt(2.0 - 2.0 * a), sqrt(2.0 * a) - 1.0, a < 0.5);
    let jy = select(1.0 - sqrt(2.0 - 2.0 * b), sqrt(2.0 * b) - 1.0, b < 0.5);
    let target_point = params.corner + params.dx * (f32(id.x) + 0.5 + jx) + params.dy * (f32(id.y) + 0.5 + jy);
    var origin = params.origin;
    var d = normalize(target_point - params.origin);
    if (HAS_LENS && params.aperture > 0.0) {
        let focus = params.origin + d * (params.focus / dot(d, params.forward));
        let lens = lens_point(rand(), rand());
        origin = params.origin + params.right * lens.x + params.up * lens.y;
        d = normalize(focus - origin);
    }
    first_albedo = vec3<f32>(0.0);
    first_normal = vec3<f32>(0.0);
    first_depth = 0.0;
    var raw = trace(origin, d);
    // Drops a NaN or infinite sample.
    if (any(raw != raw) || any(abs(raw) > vec3<f32>(3e38))) { raw = vec3<f32>(0.0); }
    // Caps the brightest samples in scenes without an HDR sky, as the CPU does.
    let sample = clamp(raw, vec3<f32>(0.0), vec3<f32>(params.cap)) * params.exposure;
    let luma = dot(sample, vec3<f32>(0.2126, 0.7152, 0.0722));
    let base = pixel * params.stride;
    accum[base] += vec4<f32>(sample, luma * luma);
    if (params.features != 0u) {
        accum[base + 1u] += vec4<f32>(first_albedo, first_depth);
        accum[base + 2u] += vec4<f32>(first_normal, 0.0);
    }
    if (params.glow_at != 0u) {
        // far-core's BLOOM_THRESHOLD and BLOOM_MAX, on the light before the cap.
        accum[base + params.glow_at] += vec4<f32>(clamp(raw * params.exposure - vec3<f32>(1.0), vec3<f32>(0.0), vec3<f32>(64.0)), 0.0);
    }
}

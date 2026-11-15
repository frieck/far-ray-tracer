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
    stride: u32, features: u32, glow_at: u32, pad: u32,
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
    v2: f32, pad1: u32, pad2: u32, pad3: u32,
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
    refr: f32, absorption_depth: f32, pad1: u32, pad2: u32,
}
const NO_TEXTURE: u32 = 0xffffffffu;
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
    let ng = normalize(cross(tri.e1, tri.e2));
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

fn intersect(o: vec3<f32>, d: vec3<f32>, limit: f32, any: bool) -> Hit {
    var best = Hit(limit, vec3<f32>(0.0), vec3<f32>(0.0), 0u, 0u, vec2<f32>(0.0));
    for (var i = 0u; i < params.plane_count; i++) {
        let plane = shapes[i];
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
    if (params.has_env == 0u) { return vec3<f32>(0.0); }
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
    let cosine = dot(l, n);
    if (cosine <= 0.0 || dot(l, ng) <= 0.0) { return vec3<f32>(0.0); }
    if (m.kind == 1u) {
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
    var emitters = 0u;
    for (var i = 0u; i < params.light_count; i++) {
        if (lights[i].kind == 0u) { sum += from_light(i, 1.0, pos, m, albedo, spec, n, ng, wo, reflect_chance); }
        else { emitters++; }
    }
    if (params.has_env != 0u && (m.kind == 1u || any(albedo > vec3<f32>(0.0)))) {
        let drawn = sample_environment(rand(), rand());
        if (drawn.w > 0.0) {
            // No 2008 highlight from the sky, as on the CPU.
            let f = respond(m, albedo, 0.0, n, ng, wo, drawn.xyz, drawn.w, reflect_chance);
            if (any(f > vec3<f32>(0.0))) {
                sum += f * environment(drawn.xyz) * visibility(pos + ng * OFFSET, drawn.xyz, FAR) / drawn.w;
            }
        }
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
        return luminance * PI * r2 / distance2;
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
        estimate = 2.0 * PI * light.radius * light.radius * max(dot(w, -l), 0.0) / (distance * distance);
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

// The light that gets from `start` along `l` to `limit`, as the CPU's surface_visibility:
// lamps and opaque surfaces block it; glass lets its share through, absorbing along the way.
fn visibility(start: vec3<f32>, l: vec3<f32>, limit: f32) -> vec3<f32> {
    if (emitter(start, l, limit).t < limit) { return vec3<f32>(0.0); }
    if (params.has_glass == 0u) {
        if (blocked(start, l, limit)) { return vec3<f32>(0.0); }
        return vec3<f32>(1.0);
    }
    var o = start;
    var remaining = limit;
    var passed = vec3<f32>(1.0);
    var traveled = 0.0;
    var entered = -1.0;
    for (var i = 0; i < 8; i++) {
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
                if (t > OFFSET && t < found.t) { found = Lamp(light.color, t, i); }
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

// Smith masking of direction `v`.
fn ggx_g1(a: f32, n: vec3<f32>, v: vec3<f32>) -> f32 {
    let z = dot(n, v);
    if (z <= 0.0) { return 0.0; }
    let slope = a * a * max(1.0 - z * z, 0.0) / (z * z);
    return 2.0 / (1.0 + sqrt(1.0 + slope));
}

fn ggx_distribution(a: f32, cosine: f32) -> f32 {
    let z = min(cosine, 1.0);
    let d = (1.0 - z * z) / (a * a) + z * z;
    return 1.0 / (PI * a * a * d * d);
}

// The density of ggx_normal's reflection toward `wi`, in solid angle.
fn ggx_pdf(a: f32, n: vec3<f32>, wo: vec3<f32>, wi: vec3<f32>) -> f32 {
    let no = dot(n, wo);
    let sum = wo + wi;
    if (no <= 0.0 || dot(n, wi) <= 0.0 || dot(sum, sum) < 1e-12) { return 0.0; }
    return ggx_distribution(a, dot(n, normalize(sum))) * ggx_g1(a, n, wo) / (4.0 * no);
}

// BRDF times the incoming cosine. With `light_pdf` > 0, the specular part is weighed against
// the reflection ray by MIS.
fn ggx_evaluate(m: Material, n: vec3<f32>, wo: vec3<f32>, wi: vec3<f32>, light_pdf: f32) -> vec3<f32> {
    let no = dot(n, wo);
    let ni = dot(n, wi);
    let sum = wo + wi;
    if (no <= 0.0 || ni <= 0.0 || dot(sum, sum) < 1e-12) { return vec3<f32>(0.0); }
    let h = normalize(sum);
    let a = ggx_alpha(m.roughness);
    let f = ggx_fresnel(m, dot(wo, h));
    let diffuse = (vec3<f32>(1.0) - f) * m.color * ((1.0 - m.metallic) * ni / PI);
    var mis = 1.0;
    if (light_pdf > 0.0) { mis = power_weight(light_pdf, ggx_pdf(a, n, wo, wi)); }
    return diffuse + f * (ggx_distribution(a, dot(n, h)) * ggx_g1(a, n, wo) * ggx_g1(a, n, wi) / (4.0 * no)) * mis;
}

fn power_weight(a: f32, b: f32) -> f32 {
    if (a <= 0.0) { return 0.0; }
    let ratio = b / a;
    return 1.0 / (1.0 + ratio * ratio);
}

// A microfacet normal visible from `wo` (Heitz 2018), as the CPU's sample_normal.
fn ggx_normal(a: f32, n: vec3<f32>, wo: vec3<f32>, u: f32, v: f32) -> vec3<f32> {
    let frame = basis(n);
    let t = frame[0];
    let b = frame[1];
    let view = normalize(vec3<f32>(a * dot(wo, t), a * dot(wo, b), dot(n, wo)));
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
    let h = t * (a * normal.x) + b * (a * normal.y) + n * max(normal.z, 0.0);
    if (dot(h, h) < 1e-12) { return n; }
    return normalize(h);
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
    // A path ends at its depth limits, by Russian roulette or at this guard.
    for (var segment = 0u; segment < 160u; segment++) {
        var hit = intersect(o, d, FAR, false);
        if (absorbing != NO_TEXTURE) {
            through *= absorption(materials[absorbing], hit.t);
            absorbing = NO_TEXTURE;
        }
        let lamp = emitter(o, d, hit.t);
        if (lamp.t < hit.t) {
            if (sees_lights) {
                var weight = 1.0;
                if (mis_pdf > 0.0) { weight = power_weight(mis_pdf, lamp_pdf(lamp, mis_from, o, d)); }
                light += capped(through * lamp.color * weight, rebound);
            }
            break;
        }
        if (hit.t >= FAR) {
            // After a diffuse rebound the environment was drawn already, by its light.
            if (sees_lights && (segment > 0u || params.env_visible != 0u)) {
                var weight = 1.0;
                if (mis_pdf > 0.0 && params.has_env != 0u) { weight = power_weight(mis_pdf, environment_pdf(d)); }
                light += capped(through * environment(d) * weight, rebound);
            }
            break;
        }
        var m = materials[hit.material];
        let pos = o + d * hit.t;
        if (m.texture != NO_TEXTURE) { m.color *= texture_color(m.texture, texture_uv(hit, pos, m)); }
        var ng = hit.ng;
        let inside = dot(ng, d) > 0.0;
        if (inside) { ng = -ng; }
        // Whether the ray meets the side the stored normal points to.
        let front = dot(d, hit.ns) < 0.0;
        var ns = hit.ns;
        if (dot(ns, ng) < 0.0) { ns = -ns; }
        if (segment == 0u) {
            first_albedo = m.color;
            first_normal = ns;
            first_depth = hit.t;
        }
        let wo = -d;
        light += capped(through * m.emission, rebound);
        mis_pdf = -1.0;
        let specular = glossy_count + transmission_count;
        let may_reflect = specular < params.max_depth && glossy_count < params.glossy_max;
        let may_bounce = diffuse_bounces < params.max_bounces && front;

        if (m.kind == 1u) {
            // GGX: a reflection drawn by its visible microfacets, or a diffuse rebound, each by
            // its chance as the CPU's lobe_chances gives it.
            let a = ggx_alpha(m.roughness);
            let seen = ggx_fresnel(m, dot(ns, wo));
            let diff = (1.0 - m.metallic) * (1.0 - max(max(seen.x, seen.y), seen.z));
            let reflects = may_reflect && m.refl > 0.0;
            let bounces = may_bounce && diff > 0.0;
            var reflect_chance = 0.0;
            if (reflects && bounces) {
                let wd = diff * max(max(m.color.x, m.color.y), m.color.z);
                let total = wd + m.refl;
                var cr = 0.5;
                var cd = 0.5;
                if (total > 0.0) {
                    cr = max(m.refl / total, 0.1);
                    cd = max(wd / total, 0.1);
                }
                reflect_chance = cr / (cr + cd);
            } else if (reflects) {
                reflect_chance = 1.0;
            }
            light += capped(through * direct(pos, m, vec3<f32>(0.0), 0.0, ns, ng, wo, reflect_chance), rebound);
            if (!reflects && !bounces) { break; }
            if (rand() < reflect_chance) {
                let h = ggx_normal(a, ns, wo, rand(), rand());
                let oh = dot(wo, h);
                let wi = h * (2.0 * oh) - wo;
                if (oh <= 0.0 || dot(ns, wi) <= 0.0 || dot(wi, ng) <= 0.0) { break; }
                through *= ggx_fresnel(m, oh) * (ggx_g1(a, ns, wi) / reflect_chance);
                mis_pdf = reflect_chance * ggx_pdf(a, ns, wo, wi);
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
            let weight = (vec3<f32>(1.0) - ggx_fresnel(m, dot(wo, normalize(wo + d)))) * m.color * (1.0 - m.metallic);
            through = survived(through * weight * (params.strength / (1.0 - reflect_chance)), &luck, diffuse_bounces);
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
        if (any(scattered > vec3<f32>(0.0)) || m.spec > 0.0) {
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
        if (pick < refr) {
            // Through: reflect or refract by Fresnel, as the CPU's refract_at, rough glass at a
            // microfacet seen from the ray. Glass takes no color at its surface; it absorbs
            // along the way inside.
            through *= strengths;
            transmission_count++;
            let a = ggx_alpha(m.roughness);
            var facet = ns;
            if (m.roughness > 0.0) { facet = ggx_normal(a, ns, wo, rand(), rand()); }
            var eta = 1.0 / m.ior;
            if (inside) { eta = m.ior; }
            let cos_i = -dot(d, facet);
            let k = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
            let f0 = pow((m.ior - 1.0) / (m.ior + 1.0), 2.0);
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
    if (params.aperture > 0.0) {
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

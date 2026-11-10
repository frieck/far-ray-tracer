// Path tracing in a compute shader: one camera sample per pixel per dispatch, added to `accum`.
// The scene comes packed by lib.rs; the BVH is the one the CPU walks (far-core's bvh.rs).

struct Params {
    origin: vec3<f32>, width: u32,
    corner: vec3<f32>, height: u32,
    dx: vec3<f32>, pass_index: u32,
    dy: vec3<f32>, max_bounces: u32,
    plane_count: u32, light_count: u32, has_env: u32, env_visible: u32,
    exposure: f32, cap: f32, env_intensity: f32, env_rotation: f32,
}

struct Node { low: vec3<f32>, first: u32, high: vec3<f32>, count: u32 }
struct Tri {
    p0: vec3<f32>, material: u32,
    e1: vec3<f32>, pad1: u32,
    e2: vec3<f32>, pad2: u32,
    n0: vec3<f32>, pad3: u32,
    n1: vec3<f32>, pad4: u32,
    n2: vec3<f32>, pad5: u32,
}
// A sphere (kind 0) or a box (kind 1) with its own axes.
struct Shape {
    center: vec3<f32>, kind: u32,
    ax: vec3<f32>, material: u32,
    ay: vec3<f32>, radius: f32,
    az: vec3<f32>, pad1: u32,
    half: vec3<f32>, pad2: u32,
}
struct Plane { point: vec3<f32>, material: u32, normal: vec3<f32>, pad: u32 }
// kind 0: the 2008 material (diff, refl). 1: GGX (roughness, metallic). 2: glass.
struct Material {
    color: vec3<f32>, kind: u32,
    emission: vec3<f32>, roughness: f32,
    metallic: f32, ior: f32, diff: f32, refl: f32,
    // The 2008 highlight's strength.
    spec: f32, pad1: u32, pad2: u32, pad3: u32,
}
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
@group(0) @binding(5) var<storage, read> planes: array<Plane>;
@group(0) @binding(6) var<storage, read> materials: array<Material>;
@group(0) @binding(7) var<storage, read> lights: array<Light>;
@group(0) @binding(8) var<storage, read_write> accum: array<vec4<f32>>;
@group(0) @binding(9) var env: texture_2d<f32>;

const PI: f32 = 3.14159265;
const FAR: f32 = 1e30;
const OFFSET: f32 = 1e-3;

var<private> rng: u32;

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
    *best = Hit(t, ng, ns, tri.material);
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
        *best = Hit(t, n, n, s.material);
        return;
    }
    // A box, in its own frame.
    let lo = vec3<f32>(dot(rel, s.ax), dot(rel, s.ay), dot(rel, s.az));
    let ld = vec3<f32>(dot(d, s.ax), dot(d, s.ay), dot(d, s.az));
    let inv = 1.0 / ld;
    let a = (-s.half - lo) * inv;
    let b = (s.half - lo) * inv;
    let near3 = min(a, b);
    let far3 = max(a, b);
    let near = max(max(near3.x, near3.y), near3.z);
    let far = min(min(far3.x, far3.y), far3.z);
    if (near > far || far <= OFFSET * 0.1) { return; }
    var t = near;
    var side = near3;
    var sign = -1.0;
    if (t <= OFFSET * 0.1) { t = far; side = far3; sign = 1.0; }
    if (t >= (*best).t) { return; }
    var n = vec3<f32>(0.0);
    if (side.x == t) { n = s.ax * sign * sign_of(ld.x); }
    else if (side.y == t) { n = s.ay * sign * sign_of(ld.y); }
    else { n = s.az * sign * sign_of(ld.z); }
    *best = Hit(t, n, n, s.material);
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
    var best = Hit(limit, vec3<f32>(0.0), vec3<f32>(0.0), 0u);
    for (var i = 0u; i < params.plane_count; i++) {
        let plane = planes[i];
        let denom = dot(d, plane.normal);
        if (abs(denom) > 1e-7) {
            let t = dot(plane.point - o, plane.normal) / denom;
            if (t > OFFSET * 0.1 && t < best.t) {
                best = Hit(t, plane.normal, plane.normal, plane.material);
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

fn environment(d: vec3<f32>) -> vec3<f32> {
    if (params.has_env == 0u) { return vec3<f32>(0.0); }
    let c = cos(params.env_rotation);
    let s = sin(params.env_rotation);
    // The inverse of the rotation the CPU applies around the vertical axis.
    let local = vec3<f32>(c * d.x - s * d.z, d.y, s * d.x + c * d.z);
    let size = textureDimensions(env);
    let u = fract(atan2(local.x, local.z) / (2.0 * PI) + 0.5);
    let v = acos(clamp(local.y, -1.0, 1.0)) / PI;
    let x = min(u32(u * f32(size.x)), size.x - 1u);
    let y = min(u32(v * f32(size.y)), size.y - 1u);
    return textureLoad(env, vec2<u32>(x, y), 0).rgb * params.env_intensity;
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

// Light a diffuse point at `pos` with normal `n` sends back along `-d` from every light, as the
// CPU's direct_light computes it: `albedo` times the irradiance over PI, plus the 2008
// highlight of strength `spec`.
fn direct(pos: vec3<f32>, n: vec3<f32>, ng: vec3<f32>, d: vec3<f32>, albedo: vec3<f32>, spec: f32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let start = pos + ng * OFFSET;
    for (var i = 0u; i < params.light_count; i++) {
        let light = lights[i];
        var target_point = light.pos;
        var weight = 1.0;
        if (light.kind == 2u) {
            target_point = light.pos + light.u * (rand() - 0.5) + light.v * (rand() - 0.5);
        } else {
            let z = 1.0 - 2.0 * rand();
            let r = sqrt(max(0.0, 1.0 - z * z));
            let angle = 2.0 * PI * rand();
            var w = vec3<f32>(r * cos(angle), r * sin(angle), z);
            if (dot(w, pos - light.pos) < 0.0) { w = -w; }
            target_point = light.pos + w * light.radius;
            if (light.kind == 1u) {
                let to_point = normalize(pos - target_point);
                weight = 2.0 * PI * light.radius * light.radius * max(dot(w, to_point), 0.0);
            }
        }
        let delta = target_point - start;
        let distance = length(delta);
        let l = delta / distance;
        let cosine = dot(l, n);
        if (cosine <= 0.0 || dot(l, ng) <= 0.0) { continue; }
        if (light.kind == 2u) {
            weight = light.area * max(-dot(light.normal, l), 0.0);
        }
        if (light.kind != 0u) { weight = weight / (PI * distance * distance); }
        if (weight <= 0.0) { continue; }
        if (blocked(start, l, distance * 0.999)) { continue; }
        sum += albedo * light.color * (weight * cosine);
        if (spec > 0.0) {
            let h = dot(d, reflect(l, n));
            if (h > 0.0) { sum += light.color * (weight * h * spec / (50.0 - 49.0 * h)); }
        }
    }
    return sum;
}

// The nearest panel or bulb the ray reaches before `limit`: its light, and the distance in w.
fn emitter(o: vec3<f32>, d: vec3<f32>, limit: f32) -> vec4<f32> {
    var found = vec4<f32>(0.0, 0.0, 0.0, limit);
    for (var i = 0u; i < params.light_count; i++) {
        let light = lights[i];
        if (light.kind == 1u) {
            let rel = o - light.pos;
            let b = dot(rel, d);
            let disc = b * b - (dot(rel, rel) - light.radius * light.radius);
            if (disc > 0.0) {
                let t = -b - sqrt(disc);
                if (t > OFFSET && t < found.w) { found = vec4<f32>(light.color, t); }
            }
        } else if (light.kind == 2u) {
            let denom = dot(d, light.normal);
            if (abs(denom) > 1e-7) {
                let t = dot(light.pos - o, light.normal) / denom;
                if (t > OFFSET && t < found.w) {
                    let p = o + d * t - light.pos;
                    let u = dot(p, light.u) / dot(light.u, light.u);
                    let v = dot(p, light.v) / dot(light.v, light.v);
                    if (abs(u) <= 0.5 && abs(v) <= 0.5) {
                        // Only the front shines; the back still blocks.
                        var color = vec3<f32>(0.0);
                        if (denom < 0.0) { color = light.color; }
                        found = vec4<f32>(color, t);
                    }
                }
            }
        }
    }
    return found;
}

fn schlick(cosine: f32, f0: f32) -> f32 {
    return f0 + (1.0 - f0) * pow(1.0 - clamp(cosine, 0.0, 1.0), 5.0);
}

// A direction around `mirror` for a surface of this roughness.
fn glossy(mirror: vec3<f32>, roughness: f32) -> vec3<f32> {
    if (roughness < 0.01) { return mirror; }
    let a = roughness * roughness;
    let u = rand();
    let angle = 2.0 * PI * rand();
    let cos_t = sqrt((1.0 - u) / (1.0 + (a * a - 1.0) * u));
    let sin_t = sqrt(max(0.0, 1.0 - cos_t * cos_t));
    // The half vector's spread turns the reflection by twice its angle; this keeps the lobe's
    // width without the full microfacet weights.
    let tilt = basis(mirror) * vec3<f32>(sin_t * cos(angle), sin_t * sin(angle), cos_t);
    return normalize(mix(mirror, tilt, 0.999));
}

fn trace(origin: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    var o = origin;
    var d = direction;
    var through = vec3<f32>(1.0);
    var light = vec3<f32>(0.0);
    // Whether light met head-on counts: on camera rays and after mirrors and glass. After a
    // diffuse bounce the lights were already sampled.
    var sees_lights = true;
    for (var bounce = 0u; bounce <= params.max_bounces; bounce++) {
        var hit = intersect(o, d, FAR, false);
        let lamp = emitter(o, d, hit.t);
        if (lamp.w < hit.t) {
            if (sees_lights) { light += through * lamp.rgb; }
            break;
        }
        if (hit.t >= FAR) {
            if (bounce > 0u || params.env_visible != 0u) { light += through * environment(d); }
            break;
        }
        let m = materials[hit.material];
        let pos = o + d * hit.t;
        var ng = hit.ng;
        let inside = dot(ng, d) > 0.0;
        if (inside) { ng = -ng; }
        // A surface scatters light on the side its normal points to, as on the CPU: the back
        // of a wall stays dark.
        let front = dot(d, hit.ns) < 0.0;
        var ns = hit.ns;
        if (dot(ns, ng) < 0.0) { ns = -ns; }
        light += through * m.emission;

        if (m.kind == 2u) {
            // Glass: reflect or refract by Fresnel.
            var eta = 1.0 / m.ior;
            if (inside) { eta = m.ior; }
            let cos_i = -dot(d, ns);
            let k = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
            let f0 = pow((m.ior - 1.0) / (m.ior + 1.0), 2.0);
            if (k < 0.0 || rand() < schlick(cos_i, f0)) {
                d = glossy(reflect(d, ns), m.roughness);
                o = pos + ng * OFFSET;
            } else {
                d = glossy(normalize(d * eta + ns * (eta * cos_i - sqrt(k))), m.roughness);
                o = pos - ng * OFFSET;
                through *= m.color;
            }
            sees_lights = true;
            continue;
        }

        // A mirror-like bounce or a diffuse one, each with a chance and a weight that make up
        // for the other not being traced. A 2008 material may reflect and scatter all the light
        // at once, so its two parts are weighed by their own strengths.
        var mirror_chance = 0.0;
        var mirror_weight = vec3<f32>(0.0);
        var albedo = vec3<f32>(0.0);
        var roughness = 0.0;
        if (m.kind == 1u) {
            let f0 = pow((m.ior - 1.0) / (m.ior + 1.0), 2.0);
            let fresnel = schlick(-dot(d, ns), f0);
            mirror_chance = clamp(m.metallic + (1.0 - m.metallic) * fresnel, 0.0, 1.0);
            mirror_weight = mix(vec3<f32>(1.0), m.color, m.metallic);
            albedo = m.color;
            roughness = m.roughness;
        } else {
            let total = m.refl + m.diff;
            if (total <= 0.0) { break; }
            mirror_chance = m.refl / total;
            mirror_weight = m.color * total;
            albedo = m.color * total;
            roughness = m.roughness;
        }
        if (rand() < mirror_chance) {
            d = glossy(reflect(d, ns), roughness);
            if (dot(d, ng) <= 0.0) { d = reflect(d, ng); }
            o = pos + ng * OFFSET;
            through *= mirror_weight;
            sees_lights = true;
            continue;
        }
        if (!front) { break; }
        // Diffuse: the lights now, and one more bounce for the light that comes off other
        // surfaces. The chance not taken above is already this branch's share.
        // This branch stands for the diffuse part alone, so the highlight, which belongs to it,
        // is divided by the branch's chance, as the albedo already is.
        var spec = 0.0;
        if (m.kind == 0u) { spec = m.spec / max(1.0 - mirror_chance, 1e-3); }
        light += through * direct(pos, ns, ng, d, albedo, spec);
        d = cosine_direction(ns);
        if (dot(d, ng) <= 0.0) { break; }
        o = pos + ng * OFFSET;
        through *= albedo;
        sees_lights = false;
        if (bounce >= 2u) {
            let survive = clamp(max(max(through.x, through.y), through.z), 0.05, 0.95);
            if (rand() >= survive) { break; }
            through /= survive;
        }
    }
    return light;
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
    var sample = trace(params.origin, normalize(target_point - params.origin));
    // Drops a NaN or infinite sample, and caps the brightest ones in scenes without an HDR sky.
    sample = clamp(sample, vec3<f32>(0.0), vec3<f32>(params.cap));
    accum[pixel] += vec4<f32>(sample * params.exposure, 1.0);
}

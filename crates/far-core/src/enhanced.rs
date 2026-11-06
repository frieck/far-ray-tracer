//! Enhanced mode: distributed ray tracing with the defects of the original fixed.
//!
//! The image is built in passes. Each pass traces one sample per pixel and adds it
//! to what is already there, so the image shows up quickly and keeps refining.
//! Varying the position in the pixel, the point on the light and the reflection
//! direction at every sample gives, through one mechanism, antialiasing, soft
//! shadows and glossy reflection (Cook, 1984).
//!
//! Fixed mode also runs here, as the same tracer with the new techniques turned off:
//! one ray per pixel, point lights, opaque shadows and a hard cut at 1.0, as in 2008.
//! The difference between Classic and Fixed is therefore only the bugs.
//!
//! Classic mode lives in `engine.rs` and uses nothing from this file.

use crate::caustics::{Photon, PhotonMap};
use crate::scene::{Scene, Sphere, Material, Primitive};
use crate::kdtree::KdTree;
use crate::engine::{self, Context, Image, Mode, Options, PhotonStats, Progress, Ray};
use crate::twister::{Twister, DEFAULT_SEED};
use crate::vector::{Color, Vec3, EPSILON, PI};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Instant;

const TILE: u32 = 32;
const MAX_DISTANCE: f32 = 10000.0;
/// Distance from the surface, so a ray does not hit its own starting point.
const SURFACE_OFFSET: f32 = 1e-3;
const ABSORPTION: f32 = 0.15;
/// Limits very bright samples, which would become isolated dots in the image.
const MAX_SAMPLE_RADIANCE: f32 = 6.0;
/// How many diffuse surfaces indirect light goes through.
const MAX_INDIRECT_BOUNCES: u32 = 2;
/// Scene lights do not fall off with distance; without this scale bounced light blows out.
const INDIRECT_STRENGTH: f32 = 0.45;
const MAX_GLASS_CROSSINGS: u32 = 6;
/// Limits what one bounce brings back: strong reflections turn into grain without this.
const MAX_INDIRECT_RADIANCE: f32 = 1.5;
/// Photons per unit of work. Each batch has its own seed, so the result does not
/// depend on how batches are spread over threads.
const PHOTON_BATCH: u32 = 8192;
const MAX_PHOTON_BOUNCES: u32 = 10;
/// Rays per glossy reflection in Fixed mode, which has no passes to average them.
const FIXED_GLOSSY_SAMPLES: u32 = 128;
/// Glossy reflection stops at this depth in Fixed mode, as in the original.
const FIXED_GLOSSY_DEPTH: i32 = 3;

struct Hit<'a> {
    pos: Vec3,
    dist: f32,
    color: Color,
    /// Geometric normal, turned to the side the ray came from.
    geo: Vec3,
    /// Smooth normal, turned to the same side.
    smooth: Vec3,
    /// Smooth normal as defined by the vertex order. Scenes rely on this: one side
    /// of a plane receives light and the other stays black, like the mirror floor of
    /// scene B.
    original: Vec3,
    entering: bool,
    /// Material weights. The original adds diffuse, reflection and refraction, so
    /// `diff=1, refl=1` returns twice the light. Dividing by the square root of the
    /// sum fixes that; dividing by the sum itself leaves the scenes too dark.
    diff: f32,
    refl: f32,
    refr: f32,
    m: &'a Material,
}

/// Orthonormal basis around `n` (Duff et al., 2017).
fn base(n: Vec3) -> (Vec3, Vec3) {
    let sign = 1.0f32.copysign(n.z);
    let a = -1.0 / (sign + n.z);
    let b = n.x * n.y * a;
    (
        Vec3::new(1.0 + sign * n.x * n.x * a, sign * b, -sign * n.x),
        Vec3::new(b, sign + n.y * n.y * a, -n.y),
    )
}

fn reflect(d: Vec3, n: Vec3) -> Vec3 {
    d - 2.0 * d.dot(n) * n
}

/// Compresses only the highest tones: up to 0.8 the image is unchanged, and above
/// that brightness approaches 1 smoothly instead of becoming a flat white area.
fn shoulder(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    if x.is_nan() || x <= 0.0 {
        0.0
    } else if x < KNEE {
        x
    } else {
        let rest = 1.0 - KNEE;
        KNEE + rest * (1.0 - (-(x - KNEE) / rest).exp())
    }
}

/// Turns a uniform number in [0, 1) into an offset in (-1, 1) with a triangular
/// distribution. Sampling this way equals a tent reconstruction filter, which
/// smooths edges better than a plain pixel average.
fn tent(u: f32) -> f32 {
    let u = 2.0 * u;
    if u < 1.0 { u.sqrt() - 1.0 } else { 1.0 - (2.0 - u).sqrt() }
}

struct Tracer<'a> {
    ctx: Context<'a>,
    /// Fixed mode: keeps the 2008 algorithm and only removes its bugs.
    fixed: bool,
    has_glass: bool,
    indirect: bool,
    caustics: Option<&'a PhotonMap>,
    caustic_strength: f32,
    caustics_only: bool,
    photon_lookups: u64,
    photons_gathered: u64,
    /// Stratum of this pass for first-hit decisions: across passes, light and bounce
    /// samples cover their domain evenly, with far less noise than independent draws
    /// at the same cost.
    stratum: (f32, f32),
    grid: f32,
    first_hit: bool,
}

impl<'a> Tracer<'a> {
    fn random(&mut self) -> f32 {
        self.ctx.rng.rand()
    }

    /// Pair of numbers in [0, 1), stratified at the first hit.
    fn pair(&mut self) -> (f32, f32) {
        let (a, b) = (self.random(), self.random());
        if self.first_hit {
            ((self.stratum.0 + a) / self.grid, (self.stratum.1 + b) / self.grid)
        } else {
            (a, b)
        }
    }

    fn nearest(&mut self, r: &Ray, limit: f32) -> Option<(usize, f32, i32)> {
        self.ctx.rays += 1;
        let mut dist = limit;
        if let Some(kd) = self.ctx.kd {
            let (res, prim) = self.ctx.closest(kd, r, &mut dist);
            return (res != 0).then_some((prim, dist, res));
        }
        let mut found = None;
        for i in 0..self.ctx.scene.prims.len() {
            let res = self.ctx.intersect(i, r, &mut dist);
            if res != 0 {
                found = Some((i, res));
            }
        }
        found.map(|(i, res)| (i, dist, res))
    }

    fn blocked(&mut self, r: &Ray, limit: f32) -> bool {
        self.ctx.rays += 1;
        if let Some(kd) = self.ctx.kd {
            return self.ctx.occluded(kd, r, limit);
        }
        let mut dist = limit;
        (0..self.ctx.scene.prims.len()).any(|i| self.ctx.intersect(i, r, &mut dist) != 0)
    }

    /// Reads the hit point. `uv` is copied here, right after the intersection: the
    /// original read it again after the shadow rays, when it already held another value.
    fn hit_at(&self, r: &Ray, prim: usize, dist: f32, result: i32) -> Hit<'a> {
        let scene: &'a Scene = self.ctx.scene;
        let pos = r.o + r.d * dist;
        let p = &scene.prims[prim];
        let m = &scene.materials[p.material()];
        let (color, geo, smooth) = match p {
            Primitive::Sphere(e) => {
                let n = (pos - e.center) * e.inv_radius;
                (scene.sphere_color_fixed(e, pos), n, n)
            }
            Primitive::Solid(s) => {
                let n = s.normal(pos);
                (scene.solid_color(s, pos), n, n)
            }
            Primitive::Triangle(t) => {
                let uv = self.ctx.uv[prim];
                let [a, b, c] = scene.corner_normals[prim];
                let mut n = a + uv[0] * (b - a) + uv[1] * (c - a);
                n.normalize();
                let n = if n.x.is_finite() { n } else { t.n };
                (scene.triangle_color_fixed(t, uv), t.n, n)
            }
        };
        let original = smooth;
        let geo = if r.d.dot(geo) > 0.0 { -geo } else { geo };
        let smooth = if r.d.dot(smooth) > 0.0 { -smooth } else { smooth };
        // The smooth normal can point backward at grazing angles; the geometric one cannot.
        let smooth = if smooth.dot(geo) <= 0.0 { geo } else { smooth };
        let k = if self.fixed { 1.0 } else { 1.0 / (m.diff + m.refl + m.refr).max(1.0).sqrt() };
        Hit {
            pos,
            dist,
            color,
            geo,
            smooth,
            original,
            entering: result > 0,
            diff: m.diff * k,
            refl: m.refl * k,
            refr: m.refr * k,
            m,
        }
    }

    /// How much light reaches the point. Glass lets part of the light through instead
    /// of casting a black shadow as in the original.
    fn visibility(&mut self, start: Vec3, dir: Vec3, distance: f32) -> Color {
        let white = Color::new(1.0, 1.0, 1.0);
        let limit = distance * (1.0 - 4.0 * EPSILON);
        if !self.has_glass {
            let r = Ray { o: start, d: dir };
            return if self.blocked(&r, limit) { Color::default() } else { white };
        }
        let scene = self.ctx.scene;
        let (mut o, mut remaining, mut transmitted) = (start, limit, white);
        for _ in 0..MAX_GLASS_CROSSINGS {
            let r = Ray { o, d: dir };
            let Some((prim, dist, _)) = self.nearest(&r, remaining) else {
                return transmitted;
            };
            let m = &scene.materials[scene.prims[prim].material()];
            if m.refr <= 0.0 {
                return Color::default();
            }
            let transmittance = m.refr / (m.refr + m.diff);
            transmitted = transmitted * (Color::new(1.0, 1.0, 1.0) * transmittance);
            o = o + dir * (dist + SURFACE_OFFSET);
            remaining -= dist + SURFACE_OFFSET;
            if remaining <= 0.0 {
                return transmitted;
            }
        }
        Color::default()
    }

    /// Random point on the half of the light sphere that faces `pos`.
    fn point_on_light(&mut self, light: &Sphere, pos: Vec3) -> Vec3 {
        if self.fixed {
            return light.center;
        }
        let (u, v) = self.pair();
        let z = 1.0 - 2.0 * u;
        let radius = (1.0 - z * z).max(0.0).sqrt();
        let angle = 2.0 * PI * v;
        let w = Vec3::new(radius * angle.cos(), radius * angle.sin(), z);
        let w = if w.dot(pos - light.center) < 0.0 { -w } else { w };
        light.center + w * light.radius
    }

    fn direct_light(&mut self, a: &Hit, d: Vec3) -> Color {
        let scene = self.ctx.scene;
        let mut acc = Color::default();
        for light in &scene.lights {
            let target = self.point_on_light(light, a.pos);
            let mut l = target - a.pos;
            let distance = l.length();
            l *= 1.0 / distance;
            let dot = l.dot(a.original);
            let highlight = if a.m.spec > 0.0 { d.dot(reflect(l, a.original)) } else { 0.0 };
            // Skips the shadow ray when this light could add nothing anyway.
            if dot <= 0.0 && highlight <= 0.0 {
                continue;
            }
            let facing = if l.dot(a.geo) >= 0.0 { a.geo } else { -a.geo };
            let transmitted = self.visibility(a.pos + facing * SURFACE_OFFSET, l, distance);
            if transmitted.x <= 0.0 && transmitted.y <= 0.0 && transmitted.z <= 0.0 {
                continue;
            }
            let light_color = scene.materials[light.material].color * transmitted;
            if a.diff > 0.0 && dot > 0.0 {
                acc += (dot * a.diff) * a.color * light_color;
            }
            // Not tied to `dot`: as in the original, the unlit side of a plane has
            // no diffuse light but still shows a highlight.
            if highlight > 0.0 {
                acc += (highlight * a.m.spec / (50.0 - 50.0 * highlight + highlight)) * light_color;
            }
        }
        acc
    }

    fn indirect_light(&mut self, a: &Hit, depth: i32, bounces: u32) -> Color {
        let (u, v) = self.pair();
        let angle = 2.0 * PI * v;
        self.first_hit = false;
        let (t, b) = base(a.smooth);
        let (radius, z) = (u.sqrt(), (1.0 - u).max(0.0).sqrt());
        let dir = t * (radius * angle.cos()) + b * (radius * angle.sin()) + a.smooth * z;
        if dir.dot(a.geo) <= 0.0 {
            return Color::default();
        }
        let r = Ray { o: a.pos + a.geo * SURFACE_OFFSET, d: dir };
        let (c, _) = self.trace(&r, depth, bounces + 1);
        let c = Color::new(c.x.min(MAX_INDIRECT_RADIANCE), c.y.min(MAX_INDIRECT_RADIANCE), c.z.min(MAX_INDIRECT_RADIANCE));
        (INDIRECT_STRENGTH * a.diff) * a.color * c
    }

    fn reflection(&mut self, a: &Hit, d: Vec3, depth: i32, bounces: u32) -> Color {
        if !self.fixed {
            return self.reflected_ray(a, d, depth, bounces, a.m.drefl > 0.0);
        }
        if a.m.drefl <= 0.0 || depth >= FIXED_GLOSSY_DEPTH {
            return self.reflected_ray(a, d, depth, bounces, false);
        }
        let mut sum = Color::default();
        for _ in 0..FIXED_GLOSSY_SAMPLES {
            sum += self.reflected_ray(a, d, depth, bounces, true);
        }
        sum * (1.0 / FIXED_GLOSSY_SAMPLES as f32)
    }

    fn reflected_ray(&mut self, a: &Hit, d: Vec3, depth: i32, bounces: u32, glossy: bool) -> Color {
        let mut dir = reflect(d, a.smooth);
        if glossy {
            // Disk of radius drefl/2 around the reflection, like the square of the original.
            let (t, b) = base(dir);
            let (u, v) = self.pair();
            let radius = 0.5 * a.m.drefl * u.sqrt();
            let angle = 2.0 * PI * v;
            dir = dir + t * (radius * angle.cos()) + b * (radius * angle.sin());
            dir.normalize();
        }
        if dir.dot(a.geo) <= 0.0 {
            dir = reflect(dir, a.geo);
        }
        let r = Ray { o: a.pos + a.geo * SURFACE_OFFSET, d: dir };
        let (c, _) = self.trace(&r, depth + 1, bounces);
        a.refl * c * a.color
    }

    /// Glass. The original uses the same index entering and leaving, so the ray does
    /// not bend on the way out, and it absorbs more the lighter the color is. Here the
    /// bend changes direction, absorption applies only inside the material, and the
    /// reflected share follows Schlick's Fresnel approximation.
    fn refraction(&mut self, a: &Hit, d: Vec3, depth: i32, bounces: u32) -> Color {
        let eta = if a.entering { 1.0 / a.m.ior } else { a.m.ior };
        let cos_i = -a.smooth.dot(d);
        let cos_t2 = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
        let reflected = |s: &mut Self| {
            let r = Ray { o: a.pos + a.geo * SURFACE_OFFSET, d: reflect(d, a.smooth) };
            s.trace(&r, depth + 1, bounces).0
        };
        if cos_t2 <= 0.0 {
            return a.refr * reflected(self);
        }
        let cos_t = cos_t2.sqrt();
        let f0 = ((a.m.ior - 1.0) / (a.m.ior + 1.0)).powi(2);
        let cos_f = if a.entering { cos_i } else { cos_t };
        let fresnel = if self.fixed { 0.0 } else { f0 + (1.0 - f0) * (1.0 - cos_f).powi(5) };

        let transmit = |s: &mut Self| {
            let mut dir = eta * d + (eta * cos_i - cos_t) * a.smooth;
            dir.normalize();
            let r = Ray { o: a.pos - a.geo * SURFACE_OFFSET, d: dir };
            let (c, dist) = s.trace(&r, depth + 1, bounces);
            if !a.entering {
                return c;
            }
            let absorbed = (Color::new(1.0, 1.0, 1.0) - a.m.color) * (ABSORPTION * -dist.min(100.0));
            c * Color::new(absorbed.x.exp(), absorbed.y.exp(), absorbed.z.exp())
        };

        // At the first surface both sides are traced, since that is what shows the most.
        // After that one side is drawn at random, so the cost does not double per crossing.
        if self.fixed {
            a.refr * transmit(self)
        } else if depth == 1 {
            a.refr * (fresnel * reflected(self) + (1.0 - fresnel) * transmit(self))
        } else if self.random() < fresnel {
            a.refr * reflected(self)
        } else {
            a.refr * transmit(self)
        }
    }

    /// Follows one photon through glass and mirrors. It is stored on each diffuse
    /// surface it reaches after at least one such bounce; light that arrives straight
    /// from the lamp is direct light and is already handled by `direct_light`.
    fn trace_photon(&mut self, mut ray: Ray, mut power: Color, photons: &mut Vec<Photon>) {
        let mut inside: Option<Color> = None;
        for bounce in 0..MAX_PHOTON_BOUNCES {
            let Some((prim, dist, result)) = self.nearest(&ray, MAX_DISTANCE) else {
                return;
            };
            if let Some(color) = inside {
                let absorbed = (Color::new(1.0, 1.0, 1.0) - color) * (ABSORPTION * -dist);
                power = power * Color::new(absorbed.x.exp(), absorbed.y.exp(), absorbed.z.exp());
            }
            let a = self.hit_at(&ray, prim, dist, result);
            if bounce > 0 && a.diff > 0.0 && ray.d.dot(a.original) < 0.0 {
                photons.push(Photon { pos: a.pos, normal: a.original, power });
            }
            let specular = a.refl + a.refr;
            if specular <= 0.0 {
                return;
            }
            power = power * specular.min(1.0);
            let bounce_off = Ray { o: a.pos + a.geo * SURFACE_OFFSET, d: reflect(ray.d, a.smooth) };
            if self.random() * specular >= a.refr {
                power = power * a.color;
                ray = bounce_off;
                continue;
            }
            let eta = if a.entering { 1.0 / a.m.ior } else { a.m.ior };
            let cos_i = -a.smooth.dot(ray.d);
            let cos_t2 = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
            if cos_t2 <= 0.0 {
                ray = bounce_off;
                continue;
            }
            let cos_t = cos_t2.sqrt();
            let f0 = ((a.m.ior - 1.0) / (a.m.ior + 1.0)).powi(2);
            let cos_f = if a.entering { cos_i } else { cos_t };
            if self.random() < f0 + (1.0 - f0) * (1.0 - cos_f).powi(5) {
                ray = bounce_off;
                continue;
            }
            let mut dir = eta * ray.d + (eta * cos_i - cos_t) * a.smooth;
            dir.normalize();
            ray = Ray { o: a.pos - a.geo * SURFACE_OFFSET, d: dir };
            inside = a.entering.then_some(a.m.color);
        }
    }

    /// Returns the color seen by the ray and the distance to the hit point.
    fn trace(&mut self, r: &Ray, depth: i32, bounces: u32) -> (Color, f32) {
        let Some((prim, dist, result)) = self.nearest(r, MAX_DISTANCE) else {
            return (Color::default(), MAX_DISTANCE);
        };
        let a = self.hit_at(r, prim, dist, result);
        self.first_hit = depth == 1 && bounces == 0;
        let mut acc = if self.caustics_only { Color::default() } else { self.direct_light(&a, r.d) };
        if let Some(map) = self.caustics {
            if bounces == 0 && a.diff > 0.0 && r.d.dot(a.original) < 0.0 {
                let (light, found) = map.irradiance(a.pos, a.original);
                self.photon_lookups += 1;
                self.photons_gathered += found as u64;
                acc += (self.caustic_strength * a.diff) * a.color * light;
            }
        }
        // The side that receives no direct light does not bounce light either.
        if self.indirect && bounces < MAX_INDIRECT_BOUNCES && a.diff > 0.0 && r.d.dot(a.original) < 0.0 {
            acc += self.indirect_light(&a, depth, bounces);
        }
        if depth < self.ctx.max_depth {
            self.first_hit = depth == 1 && bounces == 0;
            if a.refl > 0.0 {
                acc += self.reflection(&a, r.d, depth, bounces);
            }
            if a.refr > 0.0 {
                acc += self.refraction(&a, r.d, depth, bounces);
            }
        }
        (acc, a.dist)
    }
}

/// A light and the sphere around a glass or mirror object that photons aim at.
struct PhotonTarget<'a> {
    light: &'a Sphere,
    center: Vec3,
    radius: f32,
}

/// One target per light and per glass or mirror material. A bounding sphere fits both
/// spheres and meshes. Photons that miss the object are simply lost, which keeps the
/// light per area correct. A light inside the sphere, such as one above a large mirror
/// floor, has no direction to aim at, so that pair is left out.
fn photon_targets(scene: &Scene) -> Vec<PhotonTarget<'_>> {
    let mut bounds: HashMap<usize, (Vec3, Vec3)> = HashMap::new();
    let mut grow = |material: usize, low: Vec3, high: Vec3| {
        let m = &scene.materials[material];
        if m.refl <= 0.0 && m.refr <= 0.0 {
            return;
        }
        let entry = bounds.entry(material).or_insert((low, high));
        entry.0 = Vec3::new(entry.0.x.min(low.x), entry.0.y.min(low.y), entry.0.z.min(low.z));
        entry.1 = Vec3::new(entry.1.x.max(high.x), entry.1.y.max(high.y), entry.1.z.max(high.z));
    };
    for prim in &scene.prims {
        match prim {
            Primitive::Sphere(s) => {
                let r = Vec3::new(s.radius, s.radius, s.radius);
                grow(s.material, s.center - r, s.center + r);
            }
            Primitive::Solid(s) => grow(s.material, s.low, s.high),
            Primitive::Triangle(t) => {
                for v in t.vertices {
                    let p = scene.vertices[v as usize].pos;
                    grow(t.material, p, p);
                }
            }
        }
    }
    let mut materials: Vec<_> = bounds.into_iter().collect();
    materials.sort_by_key(|(material, _)| *material);
    let mut targets = Vec::new();
    for light in &scene.lights {
        for (_, (low, high)) in &materials {
            let center = (*low + *high) * 0.5;
            let radius = (*high - center).length();
            if (center - light.center).length() > radius + light.radius {
                targets.push(PhotonTarget { light, center, radius });
            }
        }
    }
    targets
}

/// Traces the caustic photons. Lights here have no falloff: they deliver the same light
/// per area at any distance. So a disk facing the light receives its own area in power,
/// and that is shared among the photons aimed at it.
fn build_photon_map(scene: &Scene, kd: Option<&KdTree>, op: &Options, progress: &Progress) -> PhotonMap {
    let targets = photon_targets(scene);
    let total_area: f32 = targets.iter().map(|t| PI * t.radius * t.radius).sum();
    let radius = op.caustic_radius.max(1e-3);
    if targets.is_empty() || op.photons == 0 {
        return PhotonMap::new(Vec::new(), radius);
    }
    let power_per_photon = total_area / op.photons as f32;
    let batches = op.photons.div_ceil(PHOTON_BATCH);
    let next_batch = AtomicUsize::new(0);
    let finished = Mutex::new(Vec::new());

    let worker = || {
        let mut t = Tracer {
            ctx: Context::with_fixes(scene, kd, op.max_depth),
            fixed: false,
            has_glass: false,
            indirect: false,
            caustics: None,
            caustic_strength: 0.0,
            caustics_only: false,
            photon_lookups: 0,
            photons_gathered: 0,
            stratum: (0.0, 0.0),
            grid: 1.0,
            first_hit: false,
        };
        loop {
            let batch = next_batch.fetch_add(1, Relaxed) as u32;
            if batch >= batches || progress.cancel.load(Relaxed) {
                break;
            }
            t.ctx.rng = Twister::new(DEFAULT_SEED ^ (batch + 1).wrapping_mul(0x85eb_ca6b));
            let count = PHOTON_BATCH.min(op.photons - batch * PHOTON_BATCH);
            let mut photons = Vec::new();
            for _ in 0..count {
                let mut pick = t.random() * total_area;
                let mut target = &targets[targets.len() - 1];
                for candidate in &targets {
                    pick -= PI * candidate.radius * candidate.radius;
                    if pick <= 0.0 {
                        target = candidate;
                        break;
                    }
                }
                let start = t.point_on_light(target.light, target.center);
                let mut axis = target.center - start;
                axis.normalize();
                let (u, v) = (t.random(), t.random());
                let (side, up) = base(axis);
                let (disk, angle) = (target.radius * u.sqrt(), 2.0 * PI * v);
                let aim = target.center + side * (disk * angle.cos()) + up * (disk * angle.sin());
                let mut dir = aim - start;
                dir.normalize();
                let color = scene.materials[target.light.material].color;
                t.trace_photon(Ray { o: start, d: dir }, color * power_per_photon, &mut photons);
            }
            finished.lock().unwrap().push((batch, photons));
        }
        progress.rays.fetch_add(t.ctx.rays, Relaxed);
    };

    std::thread::scope(|s| {
        for _ in 1..op.threads.max(1) {
            s.spawn(worker);
        }
        worker();
    });

    let mut batches = finished.into_inner().unwrap();
    batches.sort_by_key(|(batch, _)| *batch);
    PhotonMap::new(batches.into_iter().flat_map(|(_, photons)| photons).collect(), radius)
}

/// Stride that walks the pixel strata in a scattered order, so a partial image
/// already covers the whole pixel in the first passes.
fn stratum_stride(n: u32) -> u32 {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    let start = ((n as f32 * 0.618) as u32).max(1);
    (start..n.max(2)).find(|&s| gcd(s, n) == 1).unwrap_or(1)
}

#[inline]
fn channel(v: f32) -> u32 {
    (v * 255.0 + 0.5).min(255.0) as u32
}

pub fn render(scene: &Scene, kd: Option<&KdTree>, op: &Options, image: &Image, progress: &Progress) {
    let (w, h) = (op.width, op.height);
    let (tiles_x, tiles_y) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let total_tiles = (tiles_x * tiles_y) as usize;
    let fixed = op.mode == Mode::Fixed;
    let samples = if fixed { 1 } else { op.samples.max(1) };
    // The strata form the smallest square grid that holds every sample. With a count
    // that is not a perfect square, some strata simply stay unused.
    let grid = (samples as f32).sqrt().ceil() as u32;
    let strata = grid * grid;
    progress.tiles_done.store(0, Relaxed);
    progress.tiles_total.store(total_tiles * samples as usize, Relaxed);
    progress.rays.store(0, Relaxed);

    let (half_width, half_height) = engine::view_half_size(w, h);
    let p1 = Vec3::new(-half_width, half_height, 0.0);
    let dx = Vec3::new(2.0 * half_width / w as f32, 0.0, 0.0);
    let dy = Vec3::new(0.0, -2.0 * half_height / h as f32, 0.0);
    let origin = Vec3::new(0.0, 0.0, -5.0);
    progress.pass_index.store(0, Relaxed);
    *progress.photon_stats.lock().unwrap() = None;
    progress.photon_lookups.store(0, Relaxed);
    progress.photons_gathered.store(0, Relaxed);
    let photon_map = (op.caustics && !fixed).then(|| {
        let start = Instant::now();
        let map = build_photon_map(scene, kd, op, progress);
        *progress.photon_stats.lock().unwrap() = Some(PhotonStats {
            emitted: op.photons,
            stored: map.len(),
            trace_ms: start.elapsed().as_millis() as u64,
            memory_bytes: map.memory_bytes(),
        });
        map
    });
    let caustics_only = op.caustics_only && photon_map.is_some();
    // With caustics, photons carry the light that goes through glass, so shadow rays
    // must not let it through as well.
    let has_glass = !fixed && photon_map.is_none() && scene.materials.iter().any(|m| m.refr > 0.0);
    // Fixed mode samples the same point of each pixel as Classic mode, so comparing
    // the two shows the bugs and nothing else.
    let pixel_center = if fixed { 0.0 } else { 0.5 };
    let sum: Vec<AtomicU32> = (0..w as usize * h as usize * 3).map(|_| AtomicU32::new(0)).collect();
    let stride = stratum_stride(strata);
    // Each light adds full brightness with no falloff. Compensating part of that
    // keeps scenes with several lights from turning white.
    let exposure = op.exposure / (scene.lights.len().max(1) as f32).powf(0.35);

    for pass_index in 0..samples {
        if progress.cancel.load(Relaxed) {
            break;
        }
        progress.pass_index.store(pass_index as usize + 1, Relaxed);
        let stratum = pass_index * stride % strata;
        let (sx, sy) = ((stratum % grid) as f32, (stratum / grid) as f32);
        // A different permutation, so the light stratum does not follow the pixel stratum.
        let light_stratum = (pass_index * stride + strata / 3) % strata;
        let light_stratum = ((light_stratum / grid) as f32, (light_stratum % grid) as f32);
        let next_tile = AtomicUsize::new(0);

        let worker = || {
            let ctx = Context::with_fixes(scene, kd, op.max_depth);
            let mut t = Tracer {
                ctx,
                fixed,
                has_glass,
                indirect: op.indirect && !fixed && !caustics_only,
                caustics: photon_map.as_ref(),
                caustic_strength: op.caustic_strength,
                caustics_only,
                photon_lookups: 0,
                photons_gathered: 0,
                stratum: light_stratum,
                grid: grid as f32,
                first_hit: false,
            };
            loop {
                if progress.cancel.load(Relaxed) {
                    break;
                }
                let i = next_tile.fetch_add(1, Relaxed);
                if i >= total_tiles {
                    break;
                }
                // The seed depends on the pass and the tile, so the image does not change with the threads.
                let id = pass_index.wrapping_mul(total_tiles as u32).wrapping_add(i as u32 + 1);
                t.ctx.rng = Twister::new(DEFAULT_SEED ^ id.wrapping_mul(0x9e37_79b9));
                let (x0, y0) = ((i as u32 % tiles_x) * TILE, (i as u32 / tiles_x) * TILE);
                for y in y0..(y0 + TILE).min(h) {
                    for x in x0..(x0 + TILE).min(w) {
                        let (ox, oy) = if samples == 1 {
                            (0.0, 0.0)
                        } else {
                            let (a, b) = (t.random(), t.random());
                            (tent((sx + a) / grid as f32), tent((sy + b) / grid as f32))
                        };
                        let target = p1 + (x as f32 + pixel_center + ox) * dx + (y as f32 + pixel_center + oy) * dy;
                        let mut d = target - origin;
                        d.normalize();
                        let (c, _) = t.trace(&Ray { o: origin, d }, 1, 0);
                        let p = (y * w + x) as usize;
                        if fixed {
                            let rgba = engine::channel(c.x)
                                | engine::channel(c.y) << 8
                                | engine::channel(c.z) << 16
                                | 0xff00_0000;
                            image.pixels[p].store(rgba, Relaxed);
                            continue;
                        }
                        // Compression comes before the average, so an edge next to a very bright area
                        // still has no jaggies.
                        let sample = [c.x, c.y, c.z].map(|v| shoulder((v * exposure).min(MAX_SAMPLE_RADIANCE)));
                        let mut rgba = 0xff00_0000u32;
                        for (k, v) in sample.into_iter().enumerate() {
                            let cell_sum = &sum[p * 3 + k];
                            let total = f32::from_bits(cell_sum.load(Relaxed)) + v;
                            cell_sum.store(total.to_bits(), Relaxed);
                            rgba |= channel(total / (pass_index + 1) as f32) << (8 * k);
                        }
                        image.pixels[p].store(rgba, Relaxed);
                    }
                }
                progress.tiles_done.fetch_add(1, Relaxed);
                progress.rays.fetch_add(t.ctx.rays, Relaxed);
                t.ctx.rays = 0;
            }
            progress.photon_lookups.fetch_add(t.photon_lookups, Relaxed);
            progress.photons_gathered.fetch_add(t.photons_gathered, Relaxed);
        };

        std::thread::scope(|s| {
            for _ in 1..op.threads.max(1) {
                s.spawn(worker);
            }
            worker();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shoulder_keeps_midtones_and_stays_below_one() {
        assert_eq!(shoulder(0.5), 0.5);
        assert_eq!(shoulder(f32::NAN), 0.0);
        assert!(shoulder(100.0) <= 1.0);
        assert!(shoulder(0.9) > 0.8 && shoulder(0.9) < 0.9);
    }

    #[test]
    fn tent_stays_in_range() {
        assert_eq!(tent(0.5), 0.0);
        assert!(tent(0.0) >= -1.0 && tent(0.999) <= 1.0);
    }

    #[test]
    fn basis_is_orthonormal() {
        for n in [Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.6, 0.0, 0.8)] {
            let (t, b) = base(n);
            assert!(t.dot(n).abs() < 1e-5 && b.dot(n).abs() < 1e-5 && t.dot(b).abs() < 1e-5);
            assert!((t.length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn strata_cover_all_values() {
        for n in [4u32, 16, 64, 256] {
            let stride = stratum_stride(n);
            let mut seen: Vec<u32> = (0..n).map(|i| i * stride % n).collect();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), n as usize);
        }
    }
}

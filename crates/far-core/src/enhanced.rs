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

use crate::camera::{Pose, View};
use crate::caustics::{Photon, PhotonMap};
use crate::denoise::{self, Guide, Primary};
use crate::scene::{Scene, Sphere, Material, Primitive};
use crate::kdtree::KdTree;
use crate::engine::{self, Context, Image, Mode, Options, PhotonStats, Progress, Ray};
use crate::tonemap::{self, ToneMapping};
use crate::volume;
use crate::twister::{Twister, DEFAULT_SEED};
use crate::vector::{Color, Vec3, EPSILON, PI};
use std::collections::HashMap;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Instant;

const TILE: u32 = 32;
const MAX_DISTANCE: f32 = 10000.0;
/// Distance from the surface, so a ray does not hit its own starting point.
const SURFACE_OFFSET: f32 = 1e-3;
const ABSORPTION: f32 = 0.15;
/// Limits very bright samples in scenes without an HDR environment, which would become isolated
/// dots in the image. It applies before the exposure, so the light gathered can be shown again
/// at another exposure exactly.
const MAX_SAMPLE_RADIANCE: f32 = 6.0;
/// How many diffuse surfaces indirect light goes through when the scene has a 2008
/// light. Scenes lit only by physical emitters follow paths until Russian roulette ends
/// them, with `MAX_PATH_BOUNCES` only as a guard for the recursion.
const MAX_INDIRECT_BOUNCES: u32 = 2;
const MAX_PATH_BOUNCES: u32 = 64;
/// Bounces before Russian roulette starts. The first ones carry most of the light,
/// so ending them at random would only add noise.
const ROULETTE_AFTER: u32 = 3;
/// The 2008 sphere lights do not fall off with distance; without this scale bounced
/// light blows out. Scenes lit only by physical emitters (rectangles, bulbs and the
/// environment) use the full bounce, as the light then falls off correctly.
const INDIRECT_STRENGTH: f32 = 0.45;

/// How indirect light travels. The defaults depend on the lights of the scene; each
/// value can be overridden in `Options`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounces {
    /// Multiplier of each rebound. 1 is physically correct.
    pub strength: f32,
    /// Diffuse rebounds a path may take. From the third on, Russian roulette also
    /// ends paths at random, so a high limit costs little.
    pub max: u32,
    /// Most light one rebound may bring back, against isolated white dots.
    pub clamp: f32,
}

impl Bounces {
    /// Full path tracing when every light falls off with distance, and the cautious
    /// 2008-light settings otherwise.
    pub fn for_scene(scene: &Scene) -> Self {
        if scene.lights.is_empty() {
            Self { strength: 1.0, max: MAX_PATH_BOUNCES, clamp: MAX_PATH_RADIANCE }
        } else {
            Self { strength: INDIRECT_STRENGTH, max: MAX_INDIRECT_BOUNCES, clamp: MAX_INDIRECT_RADIANCE }
        }
    }

    pub fn resolve(scene: &Scene, op: &Options) -> Self {
        let auto = Self::for_scene(scene);
        Self {
            strength: op.indirect_strength.unwrap_or(auto.strength).max(0.0),
            max: op.max_bounces.unwrap_or(auto.max).min(MAX_PATH_BOUNCES),
            clamp: op.indirect_clamp.unwrap_or(auto.clamp).max(0.0),
        }
    }
}
const MAX_GLASS_CROSSINGS: u32 = 6;
/// Limits what one bounce brings back in scenes with a 2008 light: strong reflections
/// turn into grain without this.
const MAX_INDIRECT_RADIANCE: f32 = 1.5;
/// The same limit for full path tracing, far higher, as production renderers do. Only
/// rare paths reach it, such as floor, then a mirror ball, then a small bright bulb;
/// a single one of those would stay a white dot after hundreds of samples.
const MAX_PATH_RADIANCE: f32 = 10.0;
/// Photons per unit of work. Each batch has its own seed, so the result does not
/// depend on how batches are spread over threads.
const PHOTON_BATCH: u32 = 8192;
const MAX_PHOTON_BOUNCES: u32 = 10;
/// Rays per glossy reflection in Fixed mode, which has no passes to average them.
const FIXED_GLOSSY_SAMPLES: u32 = 128;
/// Most holes cut by alpha maps one ray looks through.
const MAX_CUTOUT_CROSSINGS: u32 = 16;
/// Most scattering events of light under a surface.
const MAX_SUBSURFACE_STEPS: u32 = 128;

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
    /// The material's GGX at this point, after its maps and with its surface direction.
    ggx: Option<crate::ggx::Ggx>,
    /// Light the surface gives off here.
    emission: Color,
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

/// The share of bulb `index`'s light that leaves along unit `direction`: 1 for a bare bulb,
/// less outside a spotlight's cone or an IES fixture's beam.
fn bulb_factor(scene: &Scene, index: usize, direction: Vec3) -> f32 {
    match scene.bulb_profiles.get(index) {
        Some(Some(profile)) => profile.factor(direction),
        _ => 1.0,
    }
}

/// Up to this many panels and bulbs, every point samples each of them. With more, as in a room
/// lit by strings of bulbs, each point samples one, picked by the light it likely brings, and
/// divides by the chance: one shadow ray instead of dozens, for far less noise in the same time.
const ALL_LIGHTS_UP_TO: usize = 4;

/// Panels and bulbs, panels first.
fn emitter_count(scene: &Scene) -> usize {
    scene.rect_lights.len() + scene.bulbs.len()
}

/// Roughly how much light panel or bulb `index` sends to `pos`: its brightness times the solid
/// angle it covers, and for a panel the side it faces.
fn emitter_weight(scene: &Scene, index: usize, pos: Vec3) -> f32 {
    let luminance = |c: Color| (0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z).max(0.0);
    if let Some(light) = scene.rect_lights.get(index) {
        let to_point = pos - light.proxy.center;
        let distance2 = to_point.dot(to_point).max(0.25 * light.area).max(1e-6);
        let facing = (light.normal.dot(to_point) / distance2.sqrt() + 0.1).clamp(0.0, 1.1);
        return luminance(scene.materials[light.proxy.material].color) * light.area * facing / distance2;
    }
    let bulb = index - scene.rect_lights.len();
    let light = &scene.bulbs[bulb];
    let to_point = pos - light.center;
    let distance2 = to_point.dot(to_point).max(light.sq_radius).max(1e-6);
    let aimed = bulb_factor(scene, bulb, to_point * (1.0 / distance2.sqrt()));
    luminance(scene.materials[light.material].color) * PI * light.sq_radius * aimed / distance2
}

/// Picks one panel or bulb for `pos` with `u` in [0, 1): its index and the chance it had.
fn pick_emitter(scene: &Scene, pos: Vec3, u: f32) -> Option<(usize, f32)> {
    let count = emitter_count(scene);
    let total: f32 = (0..count).map(|i| emitter_weight(scene, i, pos)).sum();
    if !(total > 0.0 && total.is_finite()) {
        return None;
    }
    let mut left = u * total;
    let mut last = None;
    for i in 0..count {
        let weight = emitter_weight(scene, i, pos);
        if weight <= 0.0 {
            continue;
        }
        last = Some((i, weight / total));
        left -= weight;
        if left < 0.0 {
            break;
        }
    }
    last
}

/// The chance `pick_emitter` picks panel or bulb `index` at `pos`: 1 when every light is sampled.
fn emitter_probability(scene: &Scene, pos: Vec3, index: usize) -> f32 {
    let count = emitter_count(scene);
    if count <= ALL_LIGHTS_UP_TO {
        return 1.0;
    }
    let total: f32 = (0..count).map(|i| emitter_weight(scene, i, pos)).sum();
    if total > 0.0 { emitter_weight(scene, index, pos) / total } else { 0.0 }
}

fn reflect(d: Vec3, n: Vec3) -> Vec3 {
    d - 2.0 * d.dot(n) * n
}

/// Adaptive sampling stops no pixel before this many passes.
const MIN_ADAPTIVE_PASSES: u32 = 16;

/// Per-pixel sums that estimate how noisy each pixel still is, and which pixels are done.
struct Adaptive {
    threshold: f32,
    min_passes: u32,
    /// Sum of the luminance of each pixel's samples, and of its square.
    sums: Vec<AtomicU32>,
    done: Vec<AtomicBool>,
}

impl Adaptive {
    fn new(op: &Options, fixed: bool, samples: u32, pixels: usize) -> Option<Self> {
        let min_passes = (samples / 16).max(MIN_ADAPTIVE_PASSES);
        (!fixed && op.noise_threshold > 0.0 && samples > min_passes).then(|| Self {
            threshold: op.noise_threshold,
            min_passes,
            sums: (0..pixels * 2).map(|_| AtomicU32::new(0)).collect(),
            done: (0..pixels).map(|_| AtomicBool::new(false)).collect(),
        })
    }

    /// Adds a sample, in displayed values. Each pixel belongs to one thread at a time.
    fn record(&self, pixel: usize, sample: [f32; 3]) {
        let l = denoise::luma(sample);
        for (k, v) in [l, l * l].into_iter().enumerate() {
            let cell = &self.sums[pixel * 2 + k];
            cell.store((f32::from_bits(cell.load(Relaxed)) + v).to_bits(), Relaxed);
        }
    }

    /// Estimated error of a pixel's mean: its standard error, relative to the square root
    /// of its brightness, as the eye sees noise more in dark areas than a plain ratio says.
    fn error(&self, pixel: usize, count: u32) -> f32 {
        if count < 2 {
            return f32::INFINITY;
        }
        let n = count as f32;
        let mean = f32::from_bits(self.sums[pixel * 2].load(Relaxed)) / n;
        let square = f32::from_bits(self.sums[pixel * 2 + 1].load(Relaxed)) / n;
        let variance = (square - mean * mean).max(0.0) * n / (n - 1.0);
        (variance / n).sqrt() / (mean.max(0.0) + 0.01).sqrt()
    }

    /// Marks done the pixels whose whole 3×3 neighborhood is below the threshold, so a
    /// pixel that was merely lucky in its first samples keeps going. Returns how many
    /// pixels of the rendered area still take samples.
    fn converge(&self, counts: &[AtomicU32], width: usize, rendered: engine::Region) -> usize {
        let (x0, y0, x1, y1) = (rendered.x0 as usize, rendered.y0 as usize, rendered.x1 as usize, rendered.y1 as usize);
        let errors: Vec<f32> = (y0..y1).into_par_iter().flat_map_iter(|y| (x0..x1).map(move |x| (x, y)).collect::<Vec<_>>())
            .map(|(x, y)| {
                let p = y * width + x;
                if self.done[p].load(Relaxed) { 0.0 } else { self.error(p, counts[p].load(Relaxed)) }
            })
            .collect();
        let (rw, rh) = (x1 - x0, y1 - y0);
        let mut active = 0;
        for ry in 0..rh {
            for rx in 0..rw {
                let p = (y0 + ry) * width + x0 + rx;
                if self.done[p].load(Relaxed) {
                    continue;
                }
                let mut worst = 0.0f32;
                for ny in ry.saturating_sub(1)..(ry + 2).min(rh) {
                    for nx in rx.saturating_sub(1)..(rx + 2).min(rw) {
                        worst = worst.max(errors[ny * rw + nx]);
                    }
                }
                if worst < self.threshold {
                    self.done[p].store(true, Relaxed);
                } else {
                    active += 1;
                }
            }
        }
        active
    }
}

/// Decisions of a camera sample, in order, that come from low-discrepancy sequences; later
/// ones, deep in a path, are plain random numbers.
const SEQUENCE_DIMENSIONS: u32 = 48;

/// The first two dimensions of the Sobol sequence, as 32-bit fractions.
fn sobol(index: u32) -> (u32, u32) {
    let (mut y, mut v) = (0u32, 1u32 << 31);
    let mut i = index;
    while i != 0 {
        if i & 1 != 0 {
            y ^= v;
        }
        v ^= v >> 1;
        i >>= 1;
    }
    (index.reverse_bits(), y)
}

/// Owen scrambling by hashing (Burley 2020, after Laine and Karras): a random permutation
/// that keeps the strata of a sequence, so each pixel gets its own, equally even, points.
fn owen(x: u32, seed: u32) -> u32 {
    let mut x = x.reverse_bits().wrapping_add(seed);
    x ^= x.wrapping_mul(0x6c50_b47c);
    x ^= x.wrapping_mul(0xb82f_1e52);
    x ^= x.wrapping_mul(0xc7af_e638);
    x ^= x.wrapping_mul(0x8d22_f6e6);
    x.reverse_bits()
}

fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9e37_79b1) ^ b.wrapping_mul(0x85eb_ca77).rotate_left(13);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

fn fraction(x: u32) -> f32 {
    (x >> 8) as f32 * (1.0 / (1u32 << 24) as f32)
}

/// Sample `index` of a pixel's sequence for one decision, `dimension`: a scrambled 2D Sobol
/// point. Each dimension also shuffles the order of the samples, so separate decisions, such
/// as which light and which bounce, do not move in step from pass to pass.
fn sequence_point(index: u32, pixel: u32, dimension: u32) -> (f32, f32) {
    let seed = hash(pixel, dimension);
    let shuffled = owen(index, hash(seed, 0x51ed));
    let (x, y) = sobol(shuffled);
    (fraction(owen(x, hash(seed, 0xa5a5))), fraction(owen(y, hash(seed, 0x3c3c))))
}

/// Share of light kept at each scattering under a surface, for a surface whose overall color
/// under many scatterings is `color` (Chiang, Kutz and Burley 2016).
fn scattering_albedo(color: Color) -> Color {
    let single = |a: f32| {
        let a = a.clamp(0.0, 0.999);
        1.0 - (4.09712 + 4.20863 * a - (9.59217 + 41.6808 * a + 17.7126 * a * a).sqrt()).powi(2)
    };
    Color::new(single(color.x), single(color.y), single(color.z))
}

/// Share of light left after `distance` inside a material. Glass with an absorption depth
/// keeps its color over that depth (Beer's law); the original's glass absorbs the more the
/// lighter its color, a habit kept for its scenes.
fn absorption(m: &Material, distance: f32) -> Color {
    let distance = distance.min(100.0);
    match m.glass.and_then(|g| g.absorption_depth) {
        Some(depth) => {
            let keep = |c: f32| (c.max(1e-4).ln() * distance / depth).exp();
            Color::new(keep(m.color.x), keep(m.color.y), keep(m.color.z))
        }
        None => {
            let absorbed = (Color::new(1.0, 1.0, 1.0) - m.color) * (ABSORPTION * -distance);
            Color::new(absorbed.x.exp(), absorbed.y.exp(), absorbed.z.exp())
        }
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
    bounces: Bounces,
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
    /// What the camera ray of the current sample hit, for the denoiser.
    primary: Primary,
    /// The wavelength this path follows, once it has crossed glass that disperses.
    wavelength: Option<f32>,
    /// Pass and pixel of the camera sample being traced: its random numbers come from
    /// low-discrepancy sequences, so successive passes cover each decision evenly.
    sequence: Option<(u32, u32)>,
    /// How many numbers this camera sample has drawn from its sequences.
    dimension: u32,
}

impl<'a> Tracer<'a> {
    fn random(&mut self) -> f32 {
        if let Some((pass, pixel)) = self.sequence {
            if self.dimension < SEQUENCE_DIMENSIONS {
                let d = self.dimension;
                self.dimension += 1;
                return sequence_point(pass, pixel, d).0;
            }
        }
        self.ctx.rng.rand()
    }

    /// Pair of numbers in [0, 1). A camera sample takes them from a two-dimensional
    /// low-discrepancy sequence; other paths, such as photons, are stratified at the first hit.
    fn pair(&mut self) -> (f32, f32) {
        if let Some((pass, pixel)) = self.sequence {
            if self.dimension < SEQUENCE_DIMENSIONS {
                let d = self.dimension;
                self.dimension += 1;
                return sequence_point(pass, pixel, d);
            }
        }
        let (a, b) = (self.ctx.rng.rand(), self.ctx.rng.rand());
        if self.first_hit {
            ((self.stratum.0 + a) / self.grid, (self.stratum.1 + b) / self.grid)
        } else {
            (a, b)
        }
    }

    fn nearest(&mut self, r: &Ray, limit: f32) -> Option<(usize, f32, i32)> {
        if !self.fixed && self.ctx.scene.cutouts {
            return self.nearest_through_cutouts(r, limit);
        }
        self.nearest_surface(r, limit)
    }

    /// Like `nearest`, but rays pass through the holes alpha maps cut, so leaves and grilles
    /// show what is behind them and let light through.
    fn nearest_through_cutouts(&mut self, r: &Ray, limit: f32) -> Option<(usize, f32, i32)> {
        let scene = self.ctx.scene;
        let mut skipped = 0.0;
        let mut ray = *r;
        for _ in 0..MAX_CUTOUT_CROSSINGS {
            let (prim, dist, result) = self.nearest_surface(&ray, limit - skipped)?;
            let p = &scene.prims[prim];
            let pos = ray.o + ray.d * dist;
            let open = scene.materials[p.material()].maps.alpha
                .and_then(|map| crate::bump::map_value(scene, p, pos, map))
                .is_some_and(|alpha| alpha < 0.5);
            if !open {
                return Some((prim, skipped + dist, result));
            }
            skipped += dist + SURFACE_OFFSET;
            ray = Ray { o: pos + ray.d * SURFACE_OFFSET, d: ray.d };
        }
        None
    }

    fn nearest_surface(&mut self, r: &Ray, limit: f32) -> Option<(usize, f32, i32)> {
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
        if !self.fixed && self.ctx.scene.cutouts {
            return self.nearest_through_cutouts(r, limit).is_some();
        }
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
            Primitive::Triangle(t) if scene.displaced_face(prim).is_some() => {
                // A displaced triangle shades as the original shape at the point under it, with
                // the normal of the displaced surface from the map itself, not from the facet.
                let face = scene.displaced_face(prim).expect("displaced");
                let source = &scene.displaced_sources[face.source as usize];
                let (base, normal) = face.base_at(self.ctx.uv[prim]);
                let smooth = crate::displace::displaced_normal(scene, source, base, normal);
                (scene.color_on(source, base), t.n, smooth)
            }
            Primitive::Triangle(t) => {
                let uv = self.ctx.uv[prim];
                let [a, b, c] = scene.corner_normals[prim];
                let mut n = a + uv[0] * (b - a) + uv[1] * (c - a);
                n.normalize();
                let n = if n.x.is_finite() { n } else { t.n };
                (scene.triangle_color_fixed(t, uv), t.n, n)
            }
            Primitive::Ground(g) => {
                let smooth = crate::displace::displaced_normal(scene, p, pos, g.normal);
                (scene.ground_color(g, pos), g.normal, smooth)
            }
        };
        // Maps read the original shape of a displaced triangle, at the point under the hit.
        let (shape, at) = match scene.displaced_face(prim) {
            Some(face) => (&scene.displaced_sources[face.source as usize], face.base_at(self.ctx.uv[prim]).0),
            None => (p, pos),
        };
        let smooth = if !self.fixed && m.bump.is_some() { crate::bump::bumped_normal(scene, shape, at, smooth) } else { smooth };
        let ggx = m.ggx.map(|mut g| {
            if self.fixed {
                return g;
            }
            if let Some(map) = m.maps.roughness {
                g.roughness *= crate::bump::map_value(scene, shape, at, map).unwrap_or(1.0);
            }
            if let Some(map) = m.maps.metallic {
                g.metallic *= crate::bump::map_value(scene, shape, at, map).unwrap_or(1.0);
            }
            if g.anisotropy != 0.0 {
                g.tangent = crate::bump::texture_tangent(scene, shape, at, smooth);
            }
            if let Some(sss) = m.subsurface {
                g.subsurface = sss.amount;
            }
            g
        });
        let emission = match m.maps.emission {
            Some(e) if !self.fixed => match e.map.zip(scene.texture_uv_on(shape, at)) {
                Some((map, (u, v))) => e.color * scene.textures[map].texel_fixed(u, v),
                None => e.color,
            },
            _ => Color::default(),
        };
        let original = smooth;
        let geo = if r.d.dot(geo) > 0.0 { -geo } else { geo };
        let smooth = if r.d.dot(smooth) > 0.0 { -smooth } else { smooth };
        // The smooth normal can point backward at grazing angles; the geometric one cannot.
        let smooth = if smooth.dot(geo) <= 0.0 { geo } else { smooth };
        let k = if self.fixed || ggx.is_some() { 1.0 } else { 1.0 / (m.diff + m.refl + m.refr).max(1.0).sqrt() };
        Hit {
            pos,
            dist,
            color,
            geo,
            smooth,
            original,
            entering: result > 0,
            diff: if !self.fixed { ggx.map_or(m.diff * k, |g| {
                let f = g.fresnel(color, m.ior, -r.d.dot(smooth));
                (1.0 - g.metallic) * (1.0 - f.x.max(f.y).max(f.z))
            }) } else { m.diff },
            refl: m.refl * k,
            refr: m.refr * k,
            m,
            ggx: if self.fixed { None } else { ggx },
            emission,
        }
    }

    /// How much light reaches the point. Glass lets part of the light through instead
    /// of casting a black shadow as in the original.
    /// Light that gets from `start` to `distance` along `dir`: blocked by surfaces, tinted by
    /// glass and dimmed by fog.
    fn visibility(&mut self, start: Vec3, dir: Vec3, distance: f32) -> Color {
        let through = self.surface_visibility(start, dir, distance);
        let volumes = &self.ctx.scene.volumes;
        if self.fixed || volumes.is_empty() || through.x.max(through.y).max(through.z) <= 0.0 {
            return through;
        }
        through * (-volume::optical_depth(volumes, start, dir, distance)).exp()
    }

    fn surface_visibility(&mut self, start: Vec3, dir: Vec3, distance: f32) -> Color {
        let white = Color::new(1.0, 1.0, 1.0);
        let limit = distance * (1.0 - 4.0 * EPSILON);
        let scene = self.ctx.scene;
        if scene.rect_lights.iter().any(|light| light.intersect(start, dir, limit).is_some())
            || scene.bulbs.iter().any(|light| light.intersect(start, dir, limit).is_some()) {
            return Color::default();
        }
        if !self.has_glass {
            let r = Ray { o: start, d: dir };
            return if self.blocked(&r, limit) { Color::default() } else { white };
        }
        let (mut o, mut remaining, mut transmitted) = (start, limit, white);
        // Glass with an absorption depth tints the shadow by the length of each crossing.
        let (mut traveled, mut entered) = (0.0f32, None);
        for _ in 0..MAX_GLASS_CROSSINGS {
            let r = Ray { o, d: dir };
            let Some((prim, dist, result)) = self.nearest(&r, remaining) else {
                return transmitted;
            };
            let m = &scene.materials[scene.prims[prim].material()];
            if m.refr <= 0.0 {
                return Color::default();
            }
            let transmittance = m.refr / (m.refr + m.diff);
            transmitted = transmitted * (Color::new(1.0, 1.0, 1.0) * transmittance);
            let here = traveled + dist;
            if result > 0 {
                entered = Some(here);
            } else if m.glass.is_some_and(|g| g.absorption_depth.is_some()) {
                transmitted = transmitted * absorption(m, here - entered.take().unwrap_or(0.0));
            }
            traveled = here + SURFACE_OFFSET;
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

    /// After a diffuse bounce, light that reached that surface through a mirror, glass
    /// or GGX lobe is already in the photon map, so tracing it again would add it twice.
    fn photons_carry(&self, bounces: u32) -> bool {
        self.caustics.is_some() && bounces > 0
    }

    fn direct_light(&mut self, a: &Hit, d: Vec3, depth: i32, bounces: u32) -> Color {
        let scene = self.ctx.scene;
        let photons_carry = self.photons_carry(bounces);
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
            if !self.fixed {
                if let Some(ggx) = a.ggx {
                    if l.dot(a.geo) > 0.0 {
                        acc += if photons_carry {
                            ggx.evaluate_diffuse(a.smooth, -d, l, a.color, a.m.ior)
                        } else {
                            ggx.evaluate(a.smooth, -d, l, a.color, a.m.ior, None)
                        } * light_color * PI;
                    }
                    continue;
                }
            }
            if a.diff > 0.0 && dot > 0.0 {
                acc += (dot * a.diff) * a.color * light_color;
            }
            // Not tied to `dot`: as in the original, the unlit side of a plane has
            // no diffuse light but still shows a highlight.
            if highlight > 0.0 {
                acc += (highlight * a.m.spec / (50.0 - 50.0 * highlight + highlight)) * light_color;
            }
        }
        // With many panels and bulbs, one is sampled, picked by the light it likely brings.
        let picked = if !self.fixed && emitter_count(scene) > ALL_LIGHTS_UP_TO {
            let u = self.random();
            match pick_emitter(scene, a.pos, u) {
                Some(pick) => Some(pick),
                None => Some((usize::MAX, 1.0)),
            }
        } else {
            None
        };
        let chosen = |index: usize| picked.is_none_or(|(k, _)| k == index);
        let chance = picked.map_or(1.0, |(_, p)| p);
        for (index, light) in scene.rect_lights.iter().enumerate() {
            if !chosen(index) { continue; }
            let (u, v) = if self.fixed { (0.5, 0.5) } else { self.pair() };
            let target = light.point(u, v);
            let delta = target - a.pos;
            let distance = delta.length();
            if distance <= SURFACE_OFFSET { continue; }
            let l = delta * (1.0 / distance);
            let emission_cos = -light.normal.dot(l);
            let dot = l.dot(a.original);
            if emission_cos <= 0.0 || dot <= 0.0 { continue; }
            // Uniform area sampling: geometry term divided by the area PDF.
            let weight = light.area * emission_cos / (PI * distance * distance);
            let start = a.pos + a.geo * SURFACE_OFFSET;
            let shadow = target - start;
            let shadow_distance = shadow.length();
            let transmitted = self.visibility(start, shadow * (1.0 / shadow_distance), shadow_distance);
            let radiance = scene.materials[light.proxy.material].color * transmitted * (weight / chance);
            let light_pdf = chance * distance * distance / (light.area * emission_cos);
            self.shade_area_light(&mut acc, a, d, depth, l, dot, radiance, Some(light_pdf), photons_carry);
        }
        for (index, light) in scene.bulbs.iter().enumerate() {
            if !chosen(scene.rect_lights.len() + index) { continue; }
            let target = self.point_on_light(light, a.pos);
            let delta = target - a.pos;
            let distance = delta.length();
            if distance <= light.radius { continue; }
            let l = delta * (1.0 / distance);
            let dot = l.dot(a.original);
            if dot <= 0.0 { continue; }
            let aimed = bulb_factor(scene, index, -l) / chance;
            if aimed <= 0.0 { continue; }
            // Fixed lights from the center, as a disc of the bulb's radius seen head-on.
            let (weight, light_pdf) = if self.fixed {
                (light.sq_radius / (distance * distance), None)
            } else {
                let emission_cos = -((target - light.center) * light.inv_radius).dot(l);
                if emission_cos <= 0.0 { continue; }
                // Uniform samples on the half of the sphere that faces the point.
                let area = 2.0 * PI * light.sq_radius;
                (area * emission_cos / (PI * distance * distance), Some(chance * distance * distance / (area * emission_cos)))
            };
            // Measured from the offset start, so the shadow ray stops short of the bulb itself.
            let start = a.pos + a.geo * SURFACE_OFFSET;
            let shadow = target - start;
            let shadow_length = shadow.length();
            let reach = if self.fixed { shadow_length - light.radius } else { shadow_length };
            let transmitted = self.visibility(start, shadow * (1.0 / shadow_length), reach);
            let radiance = scene.materials[light.material].color * transmitted * (weight * aimed);
            self.shade_area_light(&mut acc, a, d, depth, l, dot, radiance, light_pdf, photons_carry);
        }
        if !self.fixed {
            if let Some(environment) = scene.environment.as_ref().filter(|e| e.active()) {
                let (u, v) = self.pair();
                let (l, radiance, pdf) = environment.sample(u, v);
                let cosine = l.dot(a.original);
                if cosine > 0. && l.dot(a.geo) > 0. && pdf > 0. {
                    let transmitted = self.visibility(a.pos + a.geo * SURFACE_OFFSET, l, MAX_DISTANCE);
                    let response = if let Some(ggx) = a.ggx {
                        ggx.evaluate(a.smooth, -d, l, a.color, a.m.ior,
                            (depth < self.ctx.max_depth).then_some(pdf))
                    } else {
                        a.color * (a.diff * cosine / PI)
                    };
                    acc += response * radiance * transmitted * (1. / pdf);
                }
            }
        }
        acc
    }

    /// Adds the response of the surface to light arriving from an emitter sample.
    /// `light_pdf` is the solid-angle density of that sample, for MIS with GGX.
    #[allow(clippy::too_many_arguments)]
    fn shade_area_light(&self, acc: &mut Color, a: &Hit, d: Vec3, depth: i32, l: Vec3, dot: f32,
        radiance: Color, light_pdf: Option<f32>, photons_carry: bool) {
        if !self.fixed {
            if let Some(ggx) = a.ggx {
                if l.dot(a.geo) > 0.0 {
                    *acc += if photons_carry {
                        ggx.evaluate_diffuse(a.smooth, -d, l, a.color, a.m.ior)
                    } else {
                        ggx.evaluate(a.smooth, -d, l, a.color, a.m.ior, light_pdf.filter(|_| depth < self.ctx.max_depth))
                    } * radiance * PI;
                }
                return;
            }
        }
        *acc += radiance * a.color * (dot * a.diff);
        let highlight = d.dot(reflect(l, a.original));
        if highlight > 0.0 && a.m.spec > 0.0 {
            *acc += radiance * (highlight * a.m.spec / (50.0 - 49.0 * highlight));
        }
    }

    fn indirect_light(&mut self, a: &Hit, d: Vec3, depth: i32, bounces: u32) -> Color {
        let (u, v) = self.pair();
        let angle = 2.0 * PI * v;
        self.first_hit = false;
        let (t, b) = base(a.smooth);
        let (radius, z) = (u.sqrt(), (1.0 - u).max(0.0).sqrt());
        let dir = t * (radius * angle.cos()) + b * (radius * angle.sin()) + a.smooth * z;
        if dir.dot(a.geo) <= 0.0 {
            return Color::default();
        }
        // A GGX surface weighs the bounce by its diffuse lobes, sheen and coat included.
        let weight = match a.ggx {
            Some(ggx) => ggx.diffuse_weight(a.smooth, -d, dir, a.color, a.m.ior),
            None => a.diff * a.color,
        };
        // Russian roulette: a dark surface passes on little light, so its path usually
        // ends here. A path that survives is divided by its chance to survive, so the
        // average stays exact while paths stay short.
        let mut survival = 1.0;
        if bounces >= ROULETTE_AFTER {
            survival = weight.x.max(weight.y).max(weight.z).clamp(0.05, 0.95);
            if self.random() >= survival {
                return Color::default();
            }
        }
        let r = Ray { o: a.pos + a.geo * SURFACE_OFFSET, d: dir };
        let (c, _) = self.trace(&r, depth, bounces + 1, false);
        let limit = self.bounces.clamp;
        let c = Color::new(c.x.min(limit), c.y.min(limit), c.z.min(limit));
        weight * c * (self.bounces.strength / survival)
    }

    fn reflection(&mut self, a: &Hit, d: Vec3, depth: i32, bounces: u32) -> Color {
        if !self.fixed {
            if let Some(ggx) = a.ggx {
                let (u, v) = self.pair();
                let Some((dir, weight)) = ggx.sample(a.smooth, -d, u, v, self.random(), a.color, a.m.ior) else { return Color::default(); };
                if dir.dot(a.geo) <= 0.0 { return Color::default(); }
                let ray = Ray { o: a.pos + a.geo * SURFACE_OFFSET, d: dir };
                let (color, distance) = self.trace(&ray, depth + 1, bounces, true);
                let mut mis = 1.0;
                if distance == MAX_DISTANCE {
                    if let Some(environment) = self.ctx.scene.environment.as_ref().filter(|e| e.active()) {
                        mis = crate::ggx::power_weight(ggx.pdf(a.smooth, -d, dir, a.color, a.m.ior), environment.pdf(dir));
                    }
                }
                let scene = self.ctx.scene;
                for (index, light) in scene.rect_lights.iter().enumerate() {
                    if let Some(hit) = light.intersect(ray.o, dir, distance + SURFACE_OFFSET) {
                        if (hit - distance).abs() < SURFACE_OFFSET && -dir.dot(light.normal) > 0.0 {
                            let light_pdf = emitter_probability(scene, a.pos, index) * hit * hit / (light.area * -dir.dot(light.normal));
                            mis = crate::ggx::power_weight(ggx.pdf(a.smooth, -d, dir, a.color, a.m.ior), light_pdf);
                        }
                    }
                }
                for (index, light) in scene.bulbs.iter().enumerate() {
                    if let Some(hit) = light.intersect(ray.o, dir, distance + SURFACE_OFFSET) {
                        let cosine = -dir.dot((ray.o + dir * hit - light.center) * light.inv_radius);
                        if (hit - distance).abs() < SURFACE_OFFSET && cosine > 0.0 {
                            let light_pdf = emitter_probability(scene, a.pos, scene.rect_lights.len() + index)
                                * hit * hit / (2.0 * PI * light.sq_radius * cosine);
                            mis = crate::ggx::power_weight(ggx.pdf(a.smooth, -d, dir, a.color, a.m.ior), light_pdf);
                        }
                    }
                }
                return color * weight * mis;
            }
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
        let (c, _) = self.trace(&r, depth + 1, bounces, true);
        a.refl * c * a.color
    }

    /// Glass. The original uses the same index entering and leaving, so the ray does
    /// not bend on the way out, and it absorbs more the lighter the color is. Here the
    /// bend changes direction, absorption applies only inside the material, and the
    /// reflected share follows Schlick's Fresnel approximation. Enhanced glass can also be
    /// rough, tinted by depth, and split light by wavelength.
    fn refraction(&mut self, a: &Hit, d: Vec3, depth: i32, bounces: u32) -> Color {
        let glass = a.m.glass.filter(|_| !self.fixed);
        // A path that first meets dispersive glass here follows one wavelength from now on,
        // and carries that wavelength's color.
        let mut chosen = None;
        let ior = match glass.and_then(|g| g.abbe) {
            Some(abbe) => {
                let wavelength = match self.wavelength {
                    Some(w) => w,
                    None => {
                        let u = self.pair().0;
                        let w = crate::spectral::sample(u);
                        self.wavelength = Some(w);
                        chosen = Some(w);
                        w
                    }
                };
                crate::spectral::ior(a.m.ior, abbe, wavelength)
            }
            None => a.m.ior,
        };
        let c = self.refract_at(a, d, depth, bounces, ior, glass.map_or(0.0, |g| g.roughness));
        match chosen {
            Some(w) => {
                // Light from other branches of the path does not depend on this wavelength.
                self.wavelength = None;
                c * crate::spectral::weight(w)
            }
            None => c,
        }
    }

    fn refract_at(&mut self, a: &Hit, d: Vec3, depth: i32, bounces: u32, ior: f32, roughness: f32) -> Color {
        // Rough glass bends light at a microfacet seen from the ray, not at the smooth surface.
        let rough = (roughness > 0.0).then(|| crate::ggx::Ggx::new(roughness, 0.0));
        let n = match rough {
            Some(ggx) => {
                let (u, v) = self.pair();
                ggx.sample_normal(a.smooth, -d, u, v).unwrap_or(a.smooth)
            }
            None => a.smooth,
        };
        // Microfacets hide part of the light that leaves them.
        let masking = |out: Vec3, side: Vec3| rough.map_or(1.0, |ggx| ggx.g1(side, out));
        let eta = if a.entering { 1.0 / ior } else { ior };
        let cos_i = -n.dot(d);
        let cos_t2 = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
        let reflected = |s: &mut Self| {
            let dir = reflect(d, n);
            if dir.dot(a.geo) <= 0.0 {
                return Color::default();
            }
            let r = Ray { o: a.pos + a.geo * SURFACE_OFFSET, d: dir };
            s.trace(&r, depth + 1, bounces, true).0 * masking(dir, a.smooth)
        };
        if cos_t2 <= 0.0 {
            return a.refr * reflected(self);
        }
        let cos_t = cos_t2.sqrt();
        let f0 = ((ior - 1.0) / (ior + 1.0)).powi(2);
        let cos_f = if a.entering { cos_i } else { cos_t };
        let fresnel = if self.fixed { 0.0 } else { f0 + (1.0 - f0) * (1.0 - cos_f).powi(5) };

        let transmit = |s: &mut Self| {
            let mut dir = eta * d + (eta * cos_i - cos_t) * n;
            dir.normalize();
            if dir.dot(a.geo) >= 0.0 {
                return Color::default();
            }
            let r = Ray { o: a.pos - a.geo * SURFACE_OFFSET, d: dir };
            // After a diffuse bounce, shadow rays have already brought this light through
            // the glass, so the transmitted ray must not see the emitter as well.
            let (c, dist) = s.trace(&r, depth + 1, bounces, !(s.has_glass && bounces > 0));
            let c = c * masking(dir, -a.smooth);
            if !a.entering {
                return c;
            }
            c * absorption(a.m, dist)
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
        let mut inside: Option<&Material> = None;
        self.wavelength = None;
        for bounce in 0..MAX_PHOTON_BOUNCES {
            let Some((prim, dist, result)) = self.nearest(&ray, MAX_DISTANCE) else {
                return;
            };
            let scene = self.ctx.scene;
            if scene.rect_lights.iter().any(|light| light.intersect(ray.o, ray.d, dist).is_some())
                || scene.bulbs.iter().any(|light| light.intersect(ray.o, ray.d, dist).is_some()) {
                return;
            }
            if let Some(m) = inside {
                power = power * absorption(m, dist);
            }
            let a = self.hit_at(&ray, prim, dist, result);
            if bounce > 0 && a.diff > 0.0 && ray.d.dot(a.original) < 0.0 {
                photons.push(Photon { pos: a.pos, normal: a.original, power });
            }
            if let Some(ggx) = a.ggx {
                let (u, v) = (self.random(), self.random());
                let Some((dir, weight)) = ggx.sample(a.smooth, -ray.d, u, v, self.random(), a.color, a.m.ior) else { return; };
                if dir.dot(a.geo) <= 0.0 { return; }
                power = power * weight;
                ray = Ray { o: a.pos + a.geo * SURFACE_OFFSET, d: dir };
                continue;
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
            // Dispersive glass gives the photon one wavelength, and its color, for the rest of
            // its way: this is what spreads a caustic into a spectrum.
            let ior = match a.m.glass.and_then(|g| g.abbe) {
                Some(abbe) => {
                    let wavelength = match self.wavelength {
                        Some(w) => w,
                        None => {
                            let w = crate::spectral::sample(self.random());
                            power = power * crate::spectral::weight(w);
                            self.wavelength = Some(w);
                            w
                        }
                    };
                    crate::spectral::ior(a.m.ior, abbe, wavelength)
                }
                None => a.m.ior,
            };
            // Rough glass bends at a microfacet.
            let n = match a.m.glass.filter(|g| g.roughness > 0.0) {
                Some(g) => {
                    let (u, v) = (self.random(), self.random());
                    crate::ggx::Ggx::new(g.roughness, 0.0).sample_normal(a.smooth, -ray.d, u, v).unwrap_or(a.smooth)
                }
                None => a.smooth,
            };
            let bounce_off = Ray { o: a.pos + a.geo * SURFACE_OFFSET, d: reflect(ray.d, n) };
            let eta = if a.entering { 1.0 / ior } else { ior };
            let cos_i = -n.dot(ray.d);
            let cos_t2 = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
            if cos_t2 <= 0.0 {
                ray = bounce_off;
                continue;
            }
            let cos_t = cos_t2.sqrt();
            let f0 = ((ior - 1.0) / (ior + 1.0)).powi(2);
            let cos_f = if a.entering { cos_i } else { cos_t };
            if self.random() < f0 + (1.0 - f0) * (1.0 - cos_f).powi(5) {
                ray = bounce_off;
                continue;
            }
            let mut dir = eta * ray.d + (eta * cos_i - cos_t) * n;
            dir.normalize();
            if dir.dot(a.geo) >= 0.0 {
                return;
            }
            ray = Ray { o: a.pos - a.geo * SURFACE_OFFSET, d: dir };
            inside = a.entering.then_some(a.m);
        }
    }

    /// Light that enters the surface, scatters inside and leaves it elsewhere: a random walk
    /// through the material, as in wax, skin or marble. Each scattering keeps a share of
    /// the light set by the surface color; where the walk leaves, the light arriving there is
    /// gathered as on a white matte surface.
    fn subsurface(&mut self, a: &Hit, d: Vec3, sss: crate::surface::Subsurface, ggx: crate::ggx::Ggx,
        depth: i32, bounces: u32) -> Color {
        let f = ggx.fresnel(a.color, a.m.ior, -d.dot(a.smooth));
        let enters = (1.0 - ggx.metallic) * (1.0 - f.x.max(f.y).max(f.z)) * sss.amount;
        if enters <= 0.0 {
            return Color::default();
        }
        let albedo = scattering_albedo(a.color);
        let (u, v) = self.pair();
        self.first_hit = false;
        // Into the surface, spread like light through a matte face.
        let (t, b) = base(-a.smooth);
        let angle = 2.0 * PI * v;
        let (radius, z) = (u.sqrt(), (1.0 - u).max(0.0).sqrt());
        let mut dir = t * (radius * angle.cos()) + b * (radius * angle.sin()) - a.smooth * z;
        if dir.dot(a.geo) >= 0.0 {
            return Color::default();
        }
        let mut pos = a.pos - a.geo * SURFACE_OFFSET;
        let mut carried = Color::new(enters, enters, enters);
        for step in 0..MAX_SUBSURFACE_STEPS {
            let length = -(1.0 - self.random()).max(1e-12).ln() * sss.radius;
            let ray = Ray { o: pos, d: dir };
            if let Some((prim, dist, result)) = self.nearest(&ray, length) {
                let exit = self.hit_at(&ray, prim, dist, result);
                // The hit normal faces the walk, which is inside; outside is the other way.
                let out = -exit.geo;
                return carried * self.light_leaving(exit.pos + out * SURFACE_OFFSET, out, depth, bounces + 1);
            }
            pos = pos + dir * length;
            carried = carried * albedo;
            // Isotropic scattering: any direction.
            let (u, v) = (self.random(), self.random());
            let z = 1.0 - 2.0 * u;
            let ring = (1.0 - z * z).max(0.0).sqrt();
            dir = Vec3::new(ring * (2.0 * PI * v).cos(), ring * (2.0 * PI * v).sin(), z);
            // Russian roulette keeps long walks in dim material short.
            if step >= 8 {
                let survival = carried.x.max(carried.y).max(carried.z).clamp(0.05, 0.95);
                if self.random() >= survival {
                    return Color::default();
                }
                carried = carried * (1.0 / survival);
            }
        }
        Color::default()
    }

    /// Light arriving at a point of a surface, as a white matte surface there returns it:
    /// where a subsurface walk leaves the material.
    fn light_leaving(&mut self, pos: Vec3, n: Vec3, depth: i32, bounces: u32) -> Color {
        static MATTE: std::sync::OnceLock<Material> = std::sync::OnceLock::new();
        let matte = MATTE.get_or_init(|| Material {
            ggx: None, surface: None, color: Color::new(1.0, 1.0, 1.0), refl: 0.0, diff: 1.0, spec: 0.0,
            refr: 0.0, ior: 1.0, drefl: 0.0, texture: None, ru_scale: 1.0, rv_scale: 1.0, bump: None,
            displacement: None, glass: None, subsurface: None, maps: crate::scene::Maps::default(),
        });
        let at = Hit {
            pos, dist: 0.0, color: Color::new(1.0, 1.0, 1.0), geo: n, smooth: n, original: n, entering: false,
            diff: 1.0, refl: 0.0, refr: 0.0, m: matte, ggx: None, emission: Color::default(),
        };
        let mut light = self.direct_light(&at, -n, depth, bounces);
        if self.indirect && bounces < self.bounces.max {
            light += self.indirect_light(&at, -n, depth, bounces);
        }
        light
    }

    /// Returns the color seen by the ray and the distance to the hit point.
    /// Light arriving along `r`, through any fog on the way.
    ///
    /// Without fog this is the light of the surface the ray hits. With fog, light also
    /// scatters toward the ray from the medium in front of the surface, and the surface's light
    /// is dimmed on its way. Camera rays, and mirror and glass rays from them, estimate both
    /// parts every time, so light shafts come out smooth; deeper rays pick one by the chance
    /// the light gets through.
    fn trace(&mut self, r: &Ray, depth: i32, bounces: u32, allow_emission: bool) -> (Color, f32) {
        let scene = self.ctx.scene;
        if self.fixed || self.caustics_only || scene.volumes.is_empty() {
            return self.trace_surface(r, depth, bounces, allow_emission, None);
        }
        let hit = self.nearest(r, MAX_DISTANCE);
        let mut closest = hit.map_or(MAX_DISTANCE, |(_, dist, _)| dist);
        for light in &scene.rect_lights {
            if let Some(dist) = light.intersect(r.o, r.d, closest) { closest = dist; }
        }
        for light in &scene.bulbs {
            if let Some(dist) = light.intersect(r.o, r.d, closest) { closest = dist; }
        }
        let depth_to_surface = volume::optical_depth(&scene.volumes, r.o, r.d, closest);
        if depth_to_surface <= 0.0 {
            return self.trace_surface(r, depth, bounces, allow_emission, Some(hit));
        }
        let through = (-depth_to_surface).exp();
        let (u, pick) = (self.random(), self.random());
        if bounces == 0 {
            // Scatters somewhere before the surface, with the density of first collisions there.
            let target = -(1.0 - u * (1.0 - through)).max(1e-30).ln();
            let mut acc = Color::default();
            if let Some((t, v)) = volume::distance_at_depth(&scene.volumes, r.o, r.d, closest, target, pick) {
                acc += self.in_scattered(r.o + r.d * t, r.d, v, depth, bounces) * (1.0 - through);
            }
            let (surface, dist) = self.trace_surface(r, depth, bounces, allow_emission, Some(hit));
            return (acc + surface * through, dist);
        }
        let target = -(1.0 - u).max(1e-30).ln();
        match volume::distance_at_depth(&scene.volumes, r.o, r.d, closest, target, pick) {
            Some((t, v)) => (self.in_scattered(r.o + r.d * t, r.d, v, depth, bounces), t),
            None => self.trace_surface(r, depth, bounces, allow_emission, Some(hit)),
        }
    }

    /// Light a fog particle at `pos`, hit by a ray going along `d`, scatters back along the
    /// ray: the light that reaches it directly, and with indirect light on, what one more
    /// scattering brings.
    fn in_scattered(&mut self, pos: Vec3, d: Vec3, index: usize, depth: i32, bounces: u32) -> Color {
        let scene = self.ctx.scene;
        let medium = scene.volumes[index];
        let g = medium.anisotropy;
        let mut acc = Color::default();
        // Each light as a diffuse surface facing it would see it, `PI` times the program's light
        // units, weighed by the phase function instead of the surface's response.
        for light in &scene.lights {
            let target = self.point_on_light(light, pos);
            let l = target - pos;
            let distance = l.length();
            let l = l * (1.0 / distance);
            let transmitted = self.visibility(pos, l, distance);
            acc += scene.materials[light.material].color * transmitted * (PI * volume::phase(l.dot(d), g));
        }
        let picked = if emitter_count(scene) > ALL_LIGHTS_UP_TO {
            let u = self.random();
            Some(pick_emitter(scene, pos, u).unwrap_or((usize::MAX, 1.0)))
        } else {
            None
        };
        let chosen = |index: usize| picked.is_none_or(|(k, _)| k == index);
        let chance = picked.map_or(1.0, |(_, p)| p);
        for (index, light) in scene.rect_lights.iter().enumerate() {
            if !chosen(index) { continue; }
            let (u, v) = self.pair();
            let target = light.point(u, v);
            let delta = target - pos;
            let distance = delta.length();
            let l = delta * (1.0 / distance);
            let emission_cos = -light.normal.dot(l);
            if emission_cos <= 0.0 { continue; }
            let weight = light.area * emission_cos / (distance * distance * chance);
            let transmitted = self.visibility(pos, l, distance);
            acc += scene.materials[light.proxy.material].color * transmitted * (weight * volume::phase(l.dot(d), g));
        }
        for (index, light) in scene.bulbs.iter().enumerate() {
            if !chosen(scene.rect_lights.len() + index) { continue; }
            let target = self.point_on_light(light, pos);
            let delta = target - pos;
            let distance = delta.length();
            if distance <= light.radius { continue; }
            let l = delta * (1.0 / distance);
            let emission_cos = -((target - light.center) * light.inv_radius).dot(l);
            if emission_cos <= 0.0 { continue; }
            let aimed = bulb_factor(scene, index, -l) / chance;
            if aimed <= 0.0 { continue; }
            let weight = 2.0 * PI * light.sq_radius * emission_cos * aimed / (distance * distance);
            let transmitted = self.visibility(pos, l, distance);
            acc += scene.materials[light.material].color * transmitted * (weight * volume::phase(l.dot(d), g));
        }
        if let Some(environment) = scene.environment.as_ref().filter(|e| e.active()) {
            let (u, v) = self.pair();
            let (l, radiance, pdf) = environment.sample(u, v);
            if pdf > 0.0 {
                let transmitted = self.visibility(pos, l, MAX_DISTANCE);
                acc += radiance * transmitted * (volume::phase(l.dot(d), g) / pdf);
            }
        }
        if self.indirect && bounces < self.bounces.max {
            let mut survival = 1.0;
            let albedo = medium.albedo;
            if bounces >= ROULETTE_AFTER {
                survival = albedo.x.max(albedo.y).max(albedo.z).clamp(0.05, 0.95);
                if self.random() >= survival {
                    return acc * medium.albedo;
                }
            }
            let (u, v) = self.pair();
            let dir = volume::sample_phase(d, g, u, v);
            let (c, _) = self.trace(&Ray { o: pos, d: dir }, depth, bounces + 1, false);
            let limit = self.bounces.clamp;
            acc += Color::new(c.x.min(limit), c.y.min(limit), c.z.min(limit)) * (self.bounces.strength / survival);
        }
        acc * medium.albedo
    }

    fn trace_surface(&mut self, r: &Ray, depth: i32, bounces: u32, allow_emission: bool,
        found: Option<Option<(usize, f32, i32)>>) -> (Color, f32) {
        let hit = match found {
            Some(hit) => hit,
            None => self.nearest(r, MAX_DISTANCE),
        };
        let mut closest = hit.map_or(MAX_DISTANCE, |(_, dist, _)| dist);
        let mut emitter = None;
        for light in &self.ctx.scene.rect_lights {
            if let Some(dist) = light.intersect(r.o, r.d, closest) {
                closest = dist;
                emitter = Some(light);
            }
        }
        let mut bulb = None;
        for (index, light) in self.ctx.scene.bulbs.iter().enumerate() {
            if let Some(dist) = light.intersect(r.o, r.d, closest) {
                closest = dist;
                bulb = Some((index, light));
                emitter = None;
            }
        }
        if let Some((index, light)) = bulb {
            // Like panels, but a sphere emits from every side, as its profile lets it.
            let emits = allow_emission && !self.caustics_only && !self.photons_carry(bounces);
            let color = if emits {
                self.ctx.scene.materials[light.material].color * bulb_factor(self.ctx.scene, index, -r.d)
            } else {
                Color::default()
            };
            return (color, closest);
        }
        if let Some(light) = emitter {
            // Direct diffuse hits are already estimated by area-light sampling.
            let emits = allow_emission && !self.caustics_only && !self.photons_carry(bounces)
                && r.d.dot(light.normal) < 0.0;
            let color = if emits { self.ctx.scene.materials[light.proxy.material].color } else { Color::default() };
            return (color, closest);
        }
        let Some((prim, dist, result)) = hit else {
            let mut color = Color::default();
            if !self.fixed && !self.caustics_only && allow_emission {
                if let Some(environment) = &self.ctx.scene.environment {
                    // Only camera misses obey background visibility. Reflection/refraction
                    // still see the environment; diffuse misses are estimated by NEE.
                    if environment.config.visible || depth > 1 || bounces > 0 {
                        color = environment.radiance(r.d);
                    }
                }
            }
            return (color, MAX_DISTANCE);
        };
        let a = self.hit_at(r, prim, dist, result);
        self.first_hit = depth == 1 && bounces == 0;
        if self.first_hit {
            self.primary = Primary { albedo: a.color.array(), normal: a.smooth.array(), depth: dist };
        }
        let mut acc = if self.caustics_only { Color::default() } else { self.direct_light(&a, r.d, depth, bounces) + a.emission };
        if let Some(map) = self.caustics {
            if bounces == 0 && a.diff > 0.0 && r.d.dot(a.original) < 0.0 {
                let (light, found) = map.irradiance(a.pos, a.original);
                self.photon_lookups += 1;
                self.photons_gathered += found as u64;
                acc += (self.caustic_strength * a.diff) * a.color * light;
            }
        }
        // The side that receives no direct light does not bounce light either.
        let front = r.d.dot(a.original) < 0.0;
        let indirect = self.indirect && bounces < self.bounces.max && a.diff > 0.0 && front;
        let subsurface = match (a.m.subsurface, a.ggx) {
            (Some(sss), Some(ggx)) if !self.caustics_only && bounces < self.bounces.max && front => Some((sss, ggx)),
            _ => None,
        };
        let deeper = depth < self.ctx.max_depth;
        let reflects = deeper && a.refl > 0.0;
        let refracts = deeper && a.refr > 0.0;
        // Where the camera first hits, every kind of light leaving the surface is traced, since
        // that is what shows the most. Past it, one kind is drawn at random, weighed by how
        // much light it carries, and divided by its chance. Tracing all of them at every
        // bounce made the rays per pixel grow about threefold per level of depth.
        let lobes = [
            indirect.then(|| a.diff * a.color.x.max(a.color.y).max(a.color.z)),
            subsurface.map(|(sss, ggx)| sss.amount * (1.0 - ggx.metallic) * a.color.x.max(a.color.y).max(a.color.z)),
            reflects.then_some(a.refl),
            refracts.then_some(a.refr),
        ];
        let all = self.fixed || (depth == 1 && bounces == 0);
        let (chosen, chance) = if all { (None, 1.0) } else { self.pick_lobe(&lobes) };
        let traced = |k: usize| lobes[k].is_some() && chosen.is_none_or(|c| c == k);
        let scale = 1.0 / chance;
        if traced(0) {
            acc += self.indirect_light(&a, r.d, depth, bounces) * scale;
        }
        if let Some((sss, ggx)) = subsurface.filter(|_| traced(1)) {
            acc += self.subsurface(&a, r.d, sss, ggx, depth, bounces) * scale;
        }
        if traced(2) {
            self.first_hit = depth == 1 && bounces == 0;
            acc += self.reflection(&a, r.d, depth, bounces) * scale;
        }
        if traced(3) {
            self.first_hit = depth == 1 && bounces == 0;
            acc += self.refraction(&a, r.d, depth, bounces) * scale;
        }
        (acc, a.dist)
    }

    /// Draws one of the lobes that can be traced, by the light each carries, with every lobe
    /// keeping at least a small chance so a dim one cannot leave rare, very bright samples.
    /// Returns the index and its chance; with no lobe, the index is past the end.
    fn pick_lobe(&mut self, lobes: &[Option<f32>; 4]) -> (Option<usize>, f32) {
        const LEAST: f32 = 0.1;
        let count = lobes.iter().flatten().count();
        if count == 0 {
            return (Some(lobes.len()), 1.0);
        }
        if count == 1 {
            return (lobes.iter().position(Option::is_some), 1.0);
        }
        let total: f32 = lobes.iter().flatten().map(|w| w.max(0.0)).sum();
        let mut chances = [0.0f32; 4];
        for (chance, lobe) in chances.iter_mut().zip(lobes) {
            if let Some(w) = lobe {
                let share = if total > 0.0 { w.max(0.0) / total } else { 1.0 / count as f32 };
                *chance = share.max(LEAST);
            }
        }
        let sum: f32 = chances.iter().sum();
        let mut u = self.random() * sum;
        let last = chances.iter().rposition(|&c| c > 0.0).unwrap_or(0);
        for (k, &c) in chances.iter().enumerate() {
            if c > 0.0 && (u < c || k == last) {
                return (Some(k), c / sum);
            }
            u -= c;
        }
        (Some(last), chances[last] / sum)
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
            // Photons aim at bounded glass and mirrors; an infinite mirror has no target.
            Primitive::Ground(_) => {}
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

/// Legacy spheres aim photons at specular objects with their historical no-falloff
/// weighting. Rectangular panels emit from uniform positions and cosine-weighted
/// directions over their front hemisphere, with power proportional to their area.
fn build_photon_map(scene: &Scene, kd: Option<&KdTree>, op: &Options, progress: &Progress) -> PhotonMap {
    let targets = photon_targets(scene);
    let sphere_area: f32 = targets.iter().map(|t| PI * t.radius * t.radius).sum();
    let rectangle_area: f32 = scene.rect_lights.iter().map(|light| PI * light.area).sum();
    let total_area = sphere_area + rectangle_area;
    let radius = op.caustic_radius.max(1e-3);
    if total_area <= 0.0 || op.photons == 0 {
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
            bounces: Bounces::for_scene(scene),
            caustics: None,
            caustic_strength: 0.0,
            caustics_only: false,
            photon_lookups: 0,
            photons_gathered: 0,
            stratum: (0.0, 0.0),
            grid: 1.0,
            first_hit: false,
            primary: Primary::default(),
            wavelength: None,
            sequence: None,
            dimension: 0,
        };
        loop {
            let batch = next_batch.fetch_add(1, Relaxed) as u32;
            if batch >= batches || progress.cancel.load(Relaxed) {
                break;
            }
            t.ctx.rng = Twister::new(DEFAULT_SEED ^ (batch + 1).wrapping_mul(0x85eb_ca6b));
            let count = PHOTON_BATCH.min(op.photons - batch * PHOTON_BATCH);
            let mut photons = Vec::new();
            for photon_index in 0..count {
                if photon_index % 64 == 0 && progress.cancel.load(Relaxed) { break; }
                let mut pick = t.random() * total_area;
                // The legacy RNG can round to 1.0. At the upper endpoint a
                // sphere-only scene must still select its last spherical target.
                if !scene.rect_lights.is_empty() && pick >= sphere_area {
                    pick -= sphere_area;
                    let mut light = &scene.rect_lights[scene.rect_lights.len() - 1];
                    for candidate in &scene.rect_lights {
                        pick -= PI * candidate.area;
                        if pick <= 0.0 { light = candidate; break; }
                    }
                    let start = light.point(t.random(), t.random());
                    let (u, v) = (t.random(), t.random());
                    let (side, up) = base(light.normal);
                    let radius = u.sqrt();
                    let angle = 2.0 * PI * v;
                    let dir = side * (radius * angle.cos()) + up * (radius * angle.sin())
                        + light.normal * (1.0 - u).sqrt();
                    // Cosine-weighted emission gives flux Le * area * PI.
                    // The legacy gather omits Lambert's 1/PI, so include it here.
                    let color = scene.materials[light.proxy.material].color;
                    t.trace_photon(Ray { o: start + light.normal * SURFACE_OFFSET, d: dir },
                        color * (power_per_photon / PI), &mut photons);
                    continue;
                }
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

    if progress.cancel.load(Relaxed) { return PhotonMap::new(Vec::new(), radius); }
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

/// How samples become screen colors.
#[derive(Clone, Copy)]
/// Samples are averaged as linear light, and the mean goes through the tone mapping curve: the
/// light of a small bright source keeps its energy, and exposure and the curve can change
/// afterwards without rendering again.
struct Display {
    curve: ToneMapping,
    /// The brightest a sample may be, before exposure; None in scenes lit by an HDR image or a
    /// sky, whose sun is far brighter than any lamp.
    cap: Option<f32>,
}

impl Display {
    fn sample(self, radiance: f32, exposure: f32) -> f32 {
        (self.cap.map_or(radiance, |cap| radiance.min(cap)) * exposure).max(0.)
    }

    /// Screen values of a pixel's mean sample.
    fn show(self, mean: [f32; 3]) -> [f32; 3] {
        tonemap::display(self.curve, mean)
    }
}

/// Filters the accumulated image and writes it over the progressive one.
/// Filters the rendered part of the image: all of it, or a region and its ring of tiles.
/// `scale` multiplies the light, for a new exposure of linear samples.
#[allow(clippy::too_many_arguments)]
fn apply_denoiser(sum: &[AtomicU32], guide: &Guide, counts: &[AtomicU32], scale: f32, display: Display, image: &Image,
    progress: &Progress, rendered: engine::Region) {
    let width = image.width as usize;
    let (x0, y0, x1, y1) = (rendered.x0 as usize, rendered.y0 as usize, rendered.x1 as usize, rendered.y1 as usize);
    let whole = x0 == 0 && y0 == 0 && x1 == width && y1 == image.height as usize;
    let cropped = (!whole).then(|| guide.crop(width, x0, y0, x1, y1));
    let guide = cropped.as_ref().unwrap_or(guide);
    let indices: Vec<usize> = (y0..y1).flat_map(|y| (x0..x1).map(move |x| y * width + x)).collect();
    let mut color = Vec::with_capacity(indices.len());
    let mut variance = Vec::with_capacity(indices.len());
    let pixel_counts: Vec<u32> = indices.iter().map(|&p| counts[p].load(Relaxed).max(1)).collect();
    if crate::oidn::available() {
        // The network takes the mean color, linear when the scene is lit by an HDR image, and
        // the albedo and normal of the first hits, which show it where edges and textures are.
        let mean = |q: usize, p: usize| [0, 1, 2].map(|k| f32::from_bits(sum[p * 3 + k].load(Relaxed)) * scale / pixel_counts[q] as f32);
        let color: Vec<[f32; 3]> = indices.iter().enumerate().map(|(q, &p)| mean(q, p)).collect();
        let (albedo, normal): (Vec<[f32; 3]>, Vec<[f32; 3]>) =
            (0..indices.len()).map(|q| guide.albedo_normal(q, pixel_counts[q])).unzip();
        if let Some(result) = crate::oidn::denoise(&color, &albedo, &normal, x1 - x0, y1 - y0, true) {
            for (&p, c) in indices.iter().zip(result) {
                let c = display.show(c);
                image.pixels[p].store(0xff00_0000 | channel(c[0]) | channel(c[1]) << 8 | channel(c[2]) << 16, Relaxed);
            }
            *progress.denoiser.lock().unwrap() = Some(engine::Denoiser::Oidn);
            return;
        }
    }
    for (q, &p) in indices.iter().enumerate() {
        let count = pixel_counts[q];
        let mean = [0, 1, 2].map(|k| f32::from_bits(sum[p * 3 + k].load(Relaxed)) * scale / count as f32);
        let brightness = denoise::luma(mean);
        let spread = (guide.mean_luminance2(q, count) * scale * scale - brightness * brightness).max(0.) / (count.max(2) - 1) as f32;
        // The filter works on displayed values, where the curve compresses the noise.
        let shown = |b: f32| display.show([b; 3])[1];
        let slope = (shown(brightness + 0.01) - shown(brightness)) / 0.01;
        color.push(display.show(mean));
        variance.push(spread * slope * slope);
    }
    let Some(result) = denoise::denoise(color, variance, guide, &pixel_counts, x1 - x0, y1 - y0, &progress.cancel) else { return };
    *progress.denoiser.lock().unwrap() = Some(engine::Denoiser::Builtin);
    for (&p, c) in indices.iter().zip(result) {
        image.pixels[p].store(0xff00_0000 | channel(c[0]) | channel(c[1]) << 8 | channel(c[2]) << 16, Relaxed);
    }
}

/// Exposed radiance above this spills into the halo; the rest of the image stays put.
const BLOOM_THRESHOLD: f32 = 1.0;
/// Caps one sample, so a stray bright path does not paint a disc.
const BLOOM_MAX: f32 = 64.0;
const BLOOM_STRENGTH: f32 = 0.6;
/// Halo sizes as a fraction of the longer image side, and their share of the light:
/// a tight core, a soft glow and a faint wide haze, as in a real lens.
const BLOOM_SCALES: [(f32, f32); 3] = [(0.003, 0.5), (0.012, 0.3), (0.04, 0.2)];

/// Box blur of one axis of an RGB buffer, with the edge pixels repeated.
fn box_blur(buffer: &mut [f32], width: usize, height: usize, radius: usize, horizontal: bool) {
    let (lines, length, step, line_step) =
        if horizontal { (height, width, 3, width * 3) } else { (width, height, width * 3, 3) };
    let mut line = vec![0f32; length];
    for index in 0..lines {
        for channel in 0..3 {
            let at = |i: usize| index * line_step + i * step + channel;
            for (i, value) in line.iter_mut().enumerate() {
                *value = buffer[at(i)];
            }
            let sample = |i: isize| line[i.clamp(0, length as isize - 1) as usize];
            let r = radius as isize;
            let mut sum: f32 = (-r..=r).map(sample).sum();
            let scale = 1.0 / (2 * radius + 1) as f32;
            for i in 0..length as isize {
                buffer[at(i as usize)] = sum * scale;
                sum += sample(i + r + 1) - sample(i - r);
            }
        }
    }
}

/// Spreads the light above the threshold into a halo and screens it over the image.
/// `counts` gives each pixel's samples when they differ; otherwise every pixel took `count`.
fn apply_bloom(glow: &[AtomicU32], counts: Option<&[AtomicU32]>, count: u32, image: &Image) {
    let (width, height) = (image.width as usize, image.height as usize);
    let samples = |i: usize| counts.map_or(count, |c| c[i / 3].load(Relaxed).max(1));
    let source: Vec<f32> = glow.iter().enumerate()
        .map(|(i, v)| f32::from_bits(v.load(Relaxed)) / samples(i) as f32)
        .map(|v| if v.is_finite() { v } else { 0.0 })
        .collect();
    if source.iter().all(|v| *v <= 0.0) {
        return;
    }
    let mut halo = vec![0f32; source.len()];
    for (size, share) in BLOOM_SCALES {
        let radius = ((size * width.max(height) as f32).round() as usize).max(1);
        let mut layer = source.clone();
        // Three box blurs are close to a Gaussian.
        for _ in 0..3 {
            box_blur(&mut layer, width, height, radius, true);
            box_blur(&mut layer, width, height, radius, false);
        }
        for (h, l) in halo.iter_mut().zip(layer) {
            *h += share * l;
        }
    }
    for (p, pixel) in image.pixels.iter().enumerate() {
        let rgba = pixel.load(Relaxed);
        let mut out = 0xff00_0000u32;
        for k in 0..3 {
            let shown = ((rgba >> (8 * k)) & 0xff) as f32 / 255.0;
            let light = 1.0 - (-BLOOM_STRENGTH * halo[p * 3 + k]).exp();
            out |= channel(shown + light - shown * light) << (8 * k);
        }
        pixel.store(out, Relaxed);
    }
}

/// Adds a sample to a pixel's sums and returns the pixel's screen color.
fn accumulate(sum: &[AtomicU32], p: usize, color: Vec3, exposure: f32, count: u32, display: Display) -> u32 {
    let radiance = [color.x, color.y, color.z];
    let mean = [0, 1, 2].map(|k| {
        let total = &sum[p * 3 + k];
        let value = f32::from_bits(total.load(Relaxed)) + display.sample(radiance[k], exposure);
        total.store(value.to_bits(), Relaxed);
        value / count as f32
    });
    let shown = display.show(mean);
    0xff00_0000 | channel(shown[0]) | channel(shown[1]) << 8 | channel(shown[2]) << 16
}

pub fn render(scene: &Scene, kd: Option<&KdTree>, op: &Options, image: &Image, progress: &Progress) {
    render_continuing(scene, kd, op, image, progress, None);
}

/// What a finished Enhanced render accumulated: the sums of its samples, the denoiser's guide
/// and the light above the bloom threshold, per pixel. The next render can continue from it
/// when only the number of passes grows, and the image can be shown again with another
/// exposure, tone mapping, denoise or bloom without tracing a ray.
pub struct Accumulation {
    width: u32,
    height: u32,
    rendered: engine::Region,
    passes: u32,
    sum: Vec<AtomicU32>,
    counts: Vec<AtomicU32>,
    /// Kept when the render denoised, so denoising can be turned off and on again.
    guide: Option<Guide>,
    /// Kept when the render had bloom.
    glow: Option<Vec<AtomicU32>>,
    /// The exposure the samples were taken with, before the scene's light compensation.
    exposure: f32,
}

impl Accumulation {
    pub fn passes(&self) -> u32 {
        self.passes
    }
}

/// The part of the image a render traces: the whole of it, or a region and a ring of tiles
/// around it, which the denoiser filters against.
fn rendered_area(op: &Options, fixed: bool) -> (u32, u32, u32, u32) {
    let (tiles_x, tiles_y) = (op.width.div_ceil(TILE), op.height.div_ceil(TILE));
    match op.region.and_then(|r| r.within(op.width, op.height)) {
        Some(r) => r.tiles(TILE, tiles_x, tiles_y, (!fixed) as u32),
        None => (0, 0, tiles_x, tiles_y),
    }
}

fn display_for(scene: &Scene, op: &Options) -> Display {
    let hdr = scene.environment.as_ref().is_some_and(|e| e.active());
    Display { curve: op.tone_mapping, cap: (!hdr).then_some(MAX_SAMPLE_RADIANCE) }
}

/// Shows `previous` again with `op`'s exposure, tone mapping, denoise and bloom, without
/// rendering. False when that cannot give the image a render would, and a render is needed:
/// another size, region, camera or scene is the caller's to rule out; here, a change between
/// the 2008 display and linear light, or of exposure where samples went through the 2008
/// shoulder or bloom was taken.
pub fn redisplay(previous: &Accumulation, scene: &Scene, op: &Options, image: &Image, progress: &Progress) -> bool {
    let fixed = op.mode == Mode::Fixed;
    let (tx0, ty0, tx1, ty1) = rendered_area(op, fixed);
    let rendered = engine::Region { x0: tx0 * TILE, y0: ty0 * TILE, x1: (tx1 * TILE).min(op.width), y1: (ty1 * TILE).min(op.height) };
    let display = display_for(scene, op);
    let same_exposure = op.exposure == previous.exposure;
    if fixed || previous.width != op.width || previous.height != op.height || previous.rendered != rendered
        || (op.bloom && !same_exposure)
        || previous.passes == 0 || (op.denoise && previous.guide.is_none()) || (op.bloom && previous.glow.is_none()) {
        return false;
    }
    *progress.denoiser.lock().unwrap() = None;
    let scale = op.exposure / previous.exposure;
    paint(&previous.sum, &previous.counts, scale, display, image, rendered);
    if let Some(guide) = previous.guide.as_ref().filter(|_| op.denoise && previous.passes >= denoise::MIN_PASSES) {
        apply_denoiser(&previous.sum, guide, &previous.counts, scale, display, image, progress, rendered);
    }
    if let Some(glow) = previous.glow.as_ref().filter(|_| op.bloom) {
        apply_bloom(glow, Some(&previous.counts), previous.passes, image);
    }
    true
}

/// Writes the mean of each pixel's samples, times `scale`, as screen colors.
fn paint(sum: &[AtomicU32], counts: &[AtomicU32], scale: f32, display: Display, image: &Image, rendered: engine::Region) {
    let width = image.width as usize;
    for y in rendered.y0 as usize..rendered.y1 as usize {
        for x in rendered.x0 as usize..rendered.x1 as usize {
            let p = y * width + x;
            let count = counts[p].load(Relaxed);
            if count == 0 {
                continue;
            }
            let mean = [0, 1, 2].map(|k| f32::from_bits(sum[p * 3 + k].load(Relaxed)) * scale / count as f32);
            let shown = display.show(mean);
            image.pixels[p].store(0xff00_0000 | channel(shown[0]) | channel(shown[1]) << 8 | channel(shown[2]) << 16, Relaxed);
        }
    }
}

/// Renders like `render`, continuing from `previous` when it was the same render with fewer
/// passes: the passes it holds are the ones a full render takes first, so the result is the
/// same, for the cost of the new passes only. Returns what this render accumulated, unless it
/// was cancelled or in Fixed mode.
pub fn render_continuing(scene: &Scene, kd: Option<&KdTree>, op: &Options, image: &Image, progress: &Progress,
    previous: Option<Accumulation>) -> Option<Accumulation> {
    let (w, h) = (op.width, op.height);
    let (tiles_x, tiles_y) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let fixed = op.mode == Mode::Fixed;
    let samples = if fixed { 1 } else { op.samples.max(1) };
    let denoising = op.denoise && !fixed && samples >= denoise::MIN_PASSES;
    // A region renders its tiles, plus a ring of tiles around it, so the denoiser filters its
    // edge against real neighbors instead of black, now or when it is turned on later.
    let (tx0, ty0, tx1, ty1) = rendered_area(op, fixed);
    let columns = tx1 - tx0;
    let total_tiles = (columns * (ty1 - ty0)) as usize;
    let all_tiles = tiles_x * tiles_y;
    let rendered = engine::Region { x0: tx0 * TILE, y0: ty0 * TILE, x1: (tx1 * TILE).min(w), y1: (ty1 * TILE).min(h) };
    // The strata form the smallest square grid that holds every sample. With a count
    // that is not a perfect square, some strata simply stay unused.
    let grid = (samples as f32).sqrt().ceil() as u32;
    let strata = grid * grid;
    let display = display_for(scene, op);
    // Continues from `previous` only when it is this render cut short: the same image, area,
    // display and exposure, with fewer passes. The caller makes sure the scene, camera and
    // other settings are the same.
    // The light gathered can be taken over at another exposure by scaling it; bloom cannot.
    let previous = previous.filter(|p| {
        !fixed && op.noise_threshold <= 0.0 && p.width == w && p.height == h && p.rendered == rendered
            && p.guide.is_some() == denoising && p.glow.is_some() == op.bloom
            && p.passes < samples && (p.exposure == op.exposure || !op.bloom)
            // A single pass samples pixel centers, which longer renders do not.
            && p.passes > 1
    });
    if let Some(p) = &previous {
        if p.exposure != op.exposure {
            let factor = op.exposure / p.exposure;
            for cell in &p.sum {
                cell.store((f32::from_bits(cell.load(Relaxed)) * factor).to_bits(), Relaxed);
            }
            if let Some(guide) = &p.guide {
                guide.scale_luminance(factor);
            }
        }
    }
    let first_pass = previous.as_ref().map_or(0, |p| p.passes);
    progress.tiles_done.store(0, Relaxed);
    progress.tiles_total.store(total_tiles * (samples - first_pass) as usize, Relaxed);
    progress.rays.store(0, Relaxed);

    let pose = if fixed { Pose::default() } else { op.pose.unwrap_or(scene.camera.pose()) };
    let view = View::new(&pose, w, h);
    let p1 = view.plane_point(-1.0, 1.0);
    let dx = view.right * (2.0 * view.half_width / w as f32);
    let dy = view.up * (-2.0 * view.half_height / h as f32);
    let origin = view.origin;
    progress.pass_index.store(0, Relaxed);
    *progress.photon_stats.lock().unwrap() = None;
    *progress.denoiser.lock().unwrap() = None;
    *progress.photon_preview.lock().unwrap() = None;
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
        if progress.capture_photon_preview.load(Relaxed) && !map.is_empty() && !progress.cancel.load(Relaxed) {
            let scale = 1024.0 / op.width.max(op.height) as f32;
            let scale = scale.min(1.0);
            let width = (op.width as f32 * scale).round().max(1.0) as u32;
            let height = (op.height as f32 * scale).round().max(1.0) as u32;
            *progress.photon_preview.lock().unwrap() = Some(map.preview(&pose, width, height));
        }
        map
    });
    let camera = crate::camera::Camera {
        aperture: if fixed { 0.0 } else { op.aperture.unwrap_or(scene.camera.aperture) },
        focus_distance: op.focus_distance.unwrap_or(scene.camera.focus_distance),
        blades: op.blades.unwrap_or(scene.camera.blades),
        blade_rotation: op.blade_rotation.unwrap_or(scene.camera.blade_rotation),
        ..crate::camera::Camera::default()
    };
    let caustics_only = op.caustics_only && photon_map.is_some();
    // With caustics, photons carry the light that goes through glass, so shadow rays
    // must not let it through as well.
    let has_glass = !fixed && photon_map.is_none() && scene.materials.iter().any(|m| m.refr > 0.0);
    // Fixed mode samples the same point of each pixel as Classic mode, so comparing
    // the two shows the bugs and nothing else.
    let pixel_center = if fixed { 0.0 } else { 0.5 };
    let pixels = w as usize * h as usize;
    let (sum, counts, guide, glow) = match previous {
        Some(p) => (p.sum, p.counts, p.guide, p.glow),
        None => (
            (0..pixels * 3).map(|_| AtomicU32::new(0)).collect::<Vec<_>>(),
            (0..pixels).map(|_| AtomicU32::new(0)).collect::<Vec<_>>(),
            denoising.then(|| Guide::new(pixels)),
            (op.bloom && !fixed).then(|| (0..pixels * 3).map(|_| AtomicU32::new(0)).collect::<Vec<_>>()),
        ),
    };
    let stride = stratum_stride(strata);
    // Each light adds full brightness with no falloff. Compensating part of that
    // keeps scenes with several lights from turning white.
    let exposure = op.exposure / (scene.lights.len().max(1) as f32).powf(0.35);
    let adaptive = Adaptive::new(op, fixed, samples, pixels);
    // A continued render starts from the image so far.
    if first_pass > 0 {
        paint(&sum, &counts, 1.0, display, image, rendered);
    }

    for pass_index in first_pass..samples {
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
                bounces: Bounces::resolve(scene, op),
                caustics: photon_map.as_ref(),
                caustic_strength: op.caustic_strength,
                caustics_only,
                photon_lookups: 0,
                photons_gathered: 0,
                // Camera samples draw their decisions from sequences of their pass, which do not
                // depend on how many passes there are; the grid of strata, which does, stays
                // with Fixed mode, so a render continued with more passes gives the same image.
                stratum: if fixed { light_stratum } else { (0.0, 0.0) },
                grid: if fixed { grid as f32 } else { 1.0 },
                first_hit: false,
                primary: Primary::default(),
                wavelength: None,
                sequence: None,
                dimension: 0,
            };
            loop {
                if progress.cancel.load(Relaxed) {
                    break;
                }
                let i = next_tile.fetch_add(1, Relaxed);
                if i >= total_tiles {
                    break;
                }
                let (tx, ty) = (tx0 + i as u32 % columns, ty0 + i as u32 / columns);
                // The seed depends on the pass and the tile, so the image does not change with
                // the threads, and a region renders as it would in the whole image.
                let id = pass_index.wrapping_mul(all_tiles).wrapping_add(ty * tiles_x + tx + 1);
                t.ctx.rng = Twister::new(DEFAULT_SEED ^ id.wrapping_mul(0x9e37_79b9));
                let (x0, y0) = (tx * TILE, ty * TILE);
                for y in y0..(y0 + TILE).min(h) {
                    for x in x0..(x0 + TILE).min(w) {
                        let p = (y * w + x) as usize;
                        if adaptive.as_ref().is_some_and(|a| a.done[p].load(Relaxed)) {
                            continue;
                        }
                        // A render cut short in the middle of a pass and continued here already
                        // has this pass in some pixels.
                        if counts[p].load(Relaxed) > pass_index {
                            continue;
                        }
                        // Each camera sample draws from its own pixel's sequences, from the start.
                        if !fixed {
                            t.sequence = Some((pass_index, y * w + x));
                            t.dimension = 0;
                        }
                        let (ox, oy) = if samples == 1 {
                            (0.0, 0.0)
                        } else if !fixed {
                            // The first pair of the sequence places the sample in the pixel.
                            let (a, b) = t.pair();
                            (tent(a), tent(b))
                        } else {
                            let (a, b) = (t.random(), t.random());
                            (tent((sx + a) / grid as f32), tent((sy + b) / grid as f32))
                        };
                        let target = p1 + (x as f32 + pixel_center + ox) * dx + (y as f32 + pixel_center + oy) * dy;
                        let mut d = target - origin;
                        d.normalize();
                        let ray = if camera.aperture > 0.0 {
                            let (u, v) = crate::camera::lens_sample(pass_index, y * w + x);
                            camera.ray(&view, d, u, v)
                        } else {
                            Ray { o: origin, d }
                        };
                        t.primary = Primary::default();
                        t.wavelength = None;
                        let (c, _) = t.trace(&ray, 1, 0, true);
                        let count = counts[p].fetch_add(1, Relaxed) + 1;
                        if let Some(guide) = &guide {
                            let sample = [c.x, c.y, c.z].map(|v| display.sample(v, exposure));
                            guide.record(p, denoise::luma(sample), &t.primary);
                        }
                        if fixed {
                            let rgba = engine::channel(c.x)
                                | engine::channel(c.y) << 8
                                | engine::channel(c.z) << 16
                                | 0xff00_0000;
                            image.pixels[p].store(rgba, Relaxed);
                            continue;
                        }
                        // HDR needs a linear mean: compressing individual samples loses
                        // energy from small bright sources and dulls rough reflections.
                        // Keep the historical display pipeline for scenes without HDR.
                        if let Some(glow) = &glow {
                            for (k, radiance) in [c.x, c.y, c.z].into_iter().enumerate() {
                                // `max` and `min` drop a NaN sample, which would spread over the whole halo.
                                let excess = (radiance * exposure - BLOOM_THRESHOLD).max(0.0).min(BLOOM_MAX);
                                let total = &glow[p * 3 + k];
                                total.store((f32::from_bits(total.load(Relaxed)) + excess).to_bits(), Relaxed);
                            }
                        }
                        if let Some(adaptive) = &adaptive {
                            adaptive.record(p, [c.x, c.y, c.z].map(|v| display.sample(v, exposure)));
                        }
                        image.pixels[p].store(accumulate(&sum, p, c, exposure, count, display), Relaxed);
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
        // Once every pixel has converged, the remaining passes would add nothing.
        if let Some(adaptive) = &adaptive {
            if pass_index + 1 >= adaptive.min_passes && !progress.cancel.load(Relaxed)
                && adaptive.converge(&counts, w as usize, rendered) == 0 {
                break;
            }
        }
    }
    if fixed || adaptive.is_some() {
        return None;
    }
    // A cancelled render keeps what it gathered, so the next one continues from it, as when
    // only the exposure changes while it runs: the passes every pixel finished count, and the
    // pixels already through the next one skip it.
    if progress.cancel.load(Relaxed) {
        let width = w as usize;
        let finished = (rendered.y0..rendered.y1)
            .flat_map(|y| (rendered.x0..rendered.x1).map(move |x| y as usize * width + x as usize))
            .map(|p| counts[p].load(Relaxed)).min().unwrap_or(0);
        return Some(Accumulation {
            width: w, height: h, rendered, passes: finished, sum, counts, guide, glow, exposure: op.exposure,
        });
    }
    if let Some(guide) = &guide {
        apply_denoiser(&sum, guide, &counts, 1.0, display, image, progress, rendered);
    }
    if let Some(glow) = &glow {
        apply_bloom(glow, Some(&counts), samples, image);
    }
    Some(Accumulation {
        width: w, height: h, rendered, passes: samples, sum, counts, guide, glow, exposure: op.exposure,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tonemap::shoulder;

    fn example(name: &str) -> Scene {
        Scene::open(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes").join(name)).unwrap()
    }

    fn small(samples: u32) -> Options {
        Options { mode: Mode::Enhanced, width: 64, height: 48, samples, threads: 4, ..Options::default() }
    }

    fn rendered(scene: &Scene, op: &Options, previous: Option<Accumulation>) -> (Vec<u32>, Option<Accumulation>) {
        let kd = KdTree::build_corrected(scene);
        let image = Image::new(op.width, op.height);
        let acc = render_continuing(scene, Some(&kd), op, &image, &Progress::default(), previous);
        (image.pixels.iter().map(|p| p.load(Relaxed)).collect(), acc)
    }

    /// Largest difference of one color channel between two images.
    fn largest_difference(a: &[u32], b: &[u32]) -> u32 {
        a.iter().zip(b).map(|(x, y)| (0..3).map(|k| ((x >> (8 * k)) & 0xff).abs_diff((y >> (8 * k)) & 0xff)).max().unwrap()).max().unwrap()
    }

    #[test]
    fn a_continued_render_is_the_render_with_all_its_passes() {
        for name in ["cornell-box.far", "materials/glass.far", "materials/coatings.far", "techniques/smoke.far"] {
            let scene = example(name);
            let (_, first) = rendered(&scene, &small(4), None);
            let (continued, _) = rendered(&scene, &small(8), first);
            assert_eq!(continued, rendered(&scene, &small(8), None).0, "{name}");
        }
        let scene = example("materials/coatings.far");
        let (_, first) = rendered(&scene, &small(4), None);
        assert_eq!(first.as_ref().unwrap().passes(), 4);
        let (continued, acc) = rendered(&scene, &small(8), first);
        assert_eq!(acc.unwrap().passes(), 8);
        let (fresh, _) = rendered(&scene, &small(8), None);
        assert_eq!(continued, fresh);
        // Another exposure starts over.
        let (_, first) = rendered(&scene, &small(4), None);
        let brighter = Options { exposure: 1.5, ..small(8) };
        let (restarted, _) = rendered(&scene, &brighter, first);
        assert_eq!(restarted, rendered(&scene, &brighter, None).0);
    }

    #[test]
    fn a_cancelled_render_continues_to_the_same_image() {
        let scene = example("materials/coatings.far");
        let kd = KdTree::build_corrected(&scene);
        let op = Options { width: 128, height: 96, ..small(24) };
        for (delay, exposure) in [(15, 1.0), (40, 1.0), (25, 1.6)] {
            // Cancelled after a while, wherever the render then is.
            let progress = Progress::default();
            let image = Image::new(op.width, op.height);
            let partial = std::thread::scope(|scope| {
                scope.spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                    progress.cancel.store(true, Relaxed);
                });
                render_continuing(&scene, Some(&kd), &op, &image, &progress, None)
            }).expect("a cancelled render keeps what it gathered");
            // On a fast machine the render may finish first; then it simply is not continued.
            let next = Options { exposure, ..op };
            let (continued, acc) = rendered(&scene, &next, Some(partial));
            assert_eq!(acc.unwrap().passes(), 24);
            let (fresh, _) = rendered(&scene, &next, None);
            assert!(largest_difference(&continued, &fresh) <= 1, "delay {delay} exposure {exposure}");
        }
    }

    #[test]
    fn redisplay_shows_what_a_new_render_would() {
        let scene = example("materials/coatings.far");
        let agx = Options { tone_mapping: ToneMapping::Agx, denoise: true, bloom: true, ..small(6) };
        let (_, acc) = rendered(&scene, &agx, None);
        let acc = acc.unwrap();
        let shown = |op: &Options| {
            let image = Image::new(op.width, op.height);
            redisplay(&acc, &scene, op, &image, &Progress::default())
                .then(|| image.pixels.iter().map(|p| p.load(Relaxed)).collect::<Vec<u32>>())
        };
        // Exposure, curve and denoise on linear light.
        for op in [
            Options { exposure: 1.7, bloom: false, ..agx },
            Options { tone_mapping: ToneMapping::Aces, exposure: 0.6, bloom: false, ..agx },
            Options { denoise: false, ..agx },
            Options { bloom: false, tone_mapping: ToneMapping::AgxPunchy, ..agx },
        ] {
            let again = shown(&op).expect("shown again");
            let (fresh, _) = rendered(&scene, &op, None);
            assert!(largest_difference(&again, &fresh) <= 1, "{:?} {}", op.tone_mapping, op.exposure);
        }
        // The 2008 display cannot become linear light, and bloom taken at one exposure cannot
        // show another.
        // A scene without an HDR environment, with the Standard curve, too.
        let cornell = example("cornell-box.far");
        let (_, kept) = rendered(&cornell, &small(6), None);
        let brighter = Options { exposure: 1.8, ..small(6) };
        let image = Image::new(64, 48);
        assert!(redisplay(kept.as_ref().unwrap(), &cornell, &brighter, &image, &Progress::default()));
        let again: Vec<u32> = image.pixels.iter().map(|p| p.load(Relaxed)).collect();
        assert!(largest_difference(&again, &rendered(&cornell, &brighter, None).0) <= 1);
        assert!(shown(&Options { exposure: 2.0, ..agx }).is_none());
        assert!(shown(&Options { width: 32, ..agx }).is_none());
    }

    #[test]
    fn many_lights_are_picked_by_the_light_they_bring() {
        let christmas = Scene::open(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/christmas.far"))).unwrap();
        assert!(emitter_count(&christmas) > ALL_LIGHTS_UP_TO);
        let pos = Vec3::new(0.0, 0.0, 9.0);
        let count = emitter_count(&christmas);
        let total: f32 = (0..count).map(|i| emitter_probability(&christmas, pos, i)).sum();
        assert!((total - 1.0).abs() < 1e-4, "{total}");
        // Picks follow the chances.
        let samples = 20_000;
        let mut picked = vec![0u32; count];
        for k in 0..samples {
            let (i, p) = pick_emitter(&christmas, pos, (k as f32 + 0.5) / samples as f32).unwrap();
            assert!((p - emitter_probability(&christmas, pos, i)).abs() < 1e-5);
            picked[i] += 1;
        }
        for (i, &n) in picked.iter().enumerate() {
            let expected = emitter_probability(&christmas, pos, i) * samples as f32;
            assert!((n as f32 - expected).abs() <= 2.0, "light {i}: {n} vs {expected}");
        }
        // Nearer bulbs are likelier.
        let near = (0..count).max_by(|&a, &b| emitter_probability(&christmas, pos, a).total_cmp(&emitter_probability(&christmas, pos, b))).unwrap();
        assert!(emitter_probability(&christmas, pos, near) > 1.0 / count as f32);
        // A few lights are all sampled every time.
        let cornell = Scene::open(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/cornell-box.far"))).unwrap();
        assert!(emitter_count(&cornell) <= ALL_LIGHTS_UP_TO);
        assert_eq!(emitter_probability(&cornell, pos, 0), 1.0);
    }

    #[test]
    fn bounce_settings_follow_the_lights_unless_overridden() {
        let physical = test_scene(r#"{"lights":[{"type":"sphere","position":[0,2,0],"radius":0.1}],"objects":[]}"#);
        let legacy = test_scene(r#"{"lights":[{"position":[0,2,0],"radius":0.1}],"objects":[]}"#);
        assert_eq!(Bounces::for_scene(&physical), Bounces { strength: 1.0, max: MAX_PATH_BOUNCES, clamp: MAX_PATH_RADIANCE });
        assert_eq!(Bounces::for_scene(&legacy), Bounces { strength: INDIRECT_STRENGTH, max: MAX_INDIRECT_BOUNCES, clamp: MAX_INDIRECT_RADIANCE });
        let op = Options { indirect_strength: Some(0.8), max_bounces: Some(500), indirect_clamp: Some(f32::INFINITY), ..Options::default() };
        assert_eq!(Bounces::resolve(&legacy, &op), Bounces { strength: 0.8, max: MAX_PATH_BOUNCES, clamp: f32::INFINITY });
        let op = Options { max_bounces: Some(0), ..Options::default() };
        assert_eq!(Bounces::resolve(&physical, &op).max, 0);
        assert_eq!(Bounces::resolve(&physical, &op).strength, 1.0);
    }

    #[test]
    fn full_paths_keep_the_energy_of_endless_bounces() {
        // A bulb halfway between two infinite white planes that face each other. Light
        // bounces between them without end, and in the end all of it is absorbed:
        // flux = (1 - albedo) / albedo * (radiosity summed over both planes). With
        // albedo 0.5 and the planes alike, each plane sums to half the flux.
        let (radius, emitted) = (0.05f32, 40.0f32);
        let mut scene = test_scene(&format!(r#"{{"lights":[{{"type":"sphere","position":[0,0,0],"radius":{radius},"intensity":{emitted}}}],"objects":[
            {{"type":"ground","position":[0,-1,0],"normal":[0,1,0],"material":{{"color":[0.5,0.5,0.5],"specular":0}}}},
            {{"type":"ground","position":[0,1,0],"normal":[0,-1,0],"material":{{"color":[0.5,0.5,0.5],"specular":0}}}}]}}"#));
        scene.environment = None;
        let flux = 4.0 * PI * PI * radius * radius * emitted;
        let summed = |full: bool| {
            let mut tracer = environment_tracer(&scene, 5);
            tracer.indirect = true;
            tracer.bounces = if full { Bounces::for_scene(&scene) } else {
                Bounces { strength: INDIRECT_STRENGTH, max: MAX_INDIRECT_BOUNCES, clamp: MAX_INDIRECT_RADIANCE }
            };
            let mut rng = Twister::new(7);
            let n = 200_000;
            let mut sum = 0.0f64;
            for _ in 0..n {
                // Points where uniform directions from the bulb meet the floor, so the
                // density is 1 / (2 pi) * h / d^3 with h = 1.
                let (z, phi) = (rng.rand(), 2.0 * PI * rng.rand());
                let s = (1.0 - z * z).max(0.0).sqrt();
                let x = Vec3::new(s * phi.cos() / z.max(1e-6), -1.0, s * phi.sin() / z.max(1e-6));
                let d = (x.x * x.x + x.z * x.z + 1.0).sqrt();
                let ray = Ray { o: Vec3::new(x.x, -0.99, x.z), d: Vec3::new(0.0, -1.0, 0.0) };
                let radiance = tracer.trace(&ray, 1, 0, true).0.x;
                sum += (PI * radiance * 2.0 * PI * d * d * d) as f64;
            }
            (sum / n as f64) as f32
        };
        let full = summed(true);
        assert!((full / (0.5 * flux) - 1.0).abs() < 0.04, "summed radiosity {full}, expected {}", 0.5 * flux);
        let limited = summed(false);
        assert!(limited < 0.8 * full, "two bounces at 45% should lose much of it: {limited} vs {full}");
    }

    fn test_scene(text: &str) -> Scene {
        Scene::from_far(text.as_bytes(), std::path::Path::new("test.far")).unwrap()
    }

    fn center_radiance(scene: &Scene, allow_emission: bool) -> Color {
        let mut tracer = Tracer {
            ctx: Context::with_fixes(scene, None, 5), fixed: true,
            has_glass: false, indirect: false, bounces: Bounces::for_scene(scene), caustics: None, caustic_strength: 1.0,
            caustics_only: false, photon_lookups: 0, photons_gathered: 0,
            stratum: (0.0, 0.0), grid: 1.0, first_hit: false, primary: Primary::default(), wavelength: None, sequence: None, dimension: 0,
        };
        tracer.trace(&Ray { o: Vec3::new(0.0, 0.0, -5.0), d: Vec3::new(0.0, 0.0, 1.0) }, 1, 0, allow_emission).0
    }

    fn environment_tracer(scene: &Scene, depth: i32) -> Tracer<'_> {
        Tracer {
            ctx: Context::with_fixes(scene, None, depth), fixed: false,
            has_glass: false, indirect: false, bounces: Bounces::for_scene(scene), caustics: None, caustic_strength: 1.,
            caustics_only: false, photon_lookups: 0, photons_gathered: 0,
            stratum: (0., 0.), grid: 1., first_hit: false, primary: Primary::default(), wavelength: None, sequence: None, dimension: 0,
        }
    }

    fn set_constant_environment(scene: &mut Scene, visible: bool) {
        scene.environment = Some(crate::environment::Environment {
            config: crate::environment::EnvironmentConfig { visible, ..Default::default() },
            map: std::sync::Arc::new(crate::environment::EnvironmentMap::new(
                8, 4, vec![Color::new(0.25, 0.25, 0.25); 32]).unwrap()),
        });
    }

    #[test]
    fn hidden_environment_still_appears_in_secondary_rays() {
        let mut scene = test_scene(r#"{"lights":[],"objects":[]}"#);
        set_constant_environment(&mut scene, false);
        let mut tracer = environment_tracer(&scene, 5);
        let ray = Ray { o: Vec3::default(), d: Vec3::new(0., 0., 1.) };
        assert_eq!(tracer.trace(&ray, 1, 0, true).0, Color::default());
        assert_eq!(tracer.trace(&ray, 2, 0, true).0, Color::new(0.25, 0.25, 0.25));
        assert_eq!(tracer.trace(&ray, 2, 1, false).0, Color::default());
        tracer.fixed = true;
        assert_eq!(tracer.trace(&ray, 2, 0, true).0, Color::default());
        tracer.fixed = false;
        tracer.caustics_only = true;
        assert_eq!(tracer.trace(&ray, 2, 0, true).0, Color::default());
        scene.environment.as_mut().unwrap().config.visible = true;
        assert_eq!(environment_tracer(&scene, 5).trace(&ray, 1, 0, true).0, Color::new(0.25, 0.25, 0.25));
    }

    #[test]
    fn environment_diffuse_is_not_counted_twice_with_indirect_light() {
        let mut scene = test_scene(r#"{"lights":[],"objects":[{"type":"plane","points":[[-100,-100,5],[100,-100,5],[100,100,5],[-100,100,5]],"material":{"color":[1,1,1],"diffuse":1,"specular":0}}]}"#);
        set_constant_environment(&mut scene, true);
        let ray = Ray { o: Vec3::new(0., 0., -5.), d: Vec3::new(0., 0., 1.) };
        for indirect in [false, true] {
            let mut tracer = environment_tracer(&scene, 5);
            tracer.indirect = indirect;
            let mut total = 0.;
            for _ in 0..20000 { total += tracer.trace(&ray, 1, 0, true).0.x; }
            assert!((total / 20000. - 0.25).abs() < 0.006);
        }
    }

    #[test]
    fn glass_keeps_its_color_over_its_absorption_depth() {
        let mut m = test_scene(r#"{"lights":[],"objects":[{"type":"sphere","center":[0,0,5],"radius":1,"material":{"color":[0.2,0.6,0.9],"surface":{"preset":"glass","absorption_depth":0.5}}}]}"#).materials[0].clone();
        let kept = absorption(&m, 0.5);
        assert!((kept - Color::new(0.2, 0.6, 0.9)).length() < 1e-4);
        assert!((absorption(&m, 1.0) - Color::new(0.04, 0.36, 0.81)).length() < 1e-4);
        m.glass = None;
        assert!(absorption(&m, 1.0).x < 1.0, "the original's glass still absorbs");
    }

    #[test]
    fn scattering_albedo_is_monotone_and_spans_black_to_white() {
        let low = scattering_albedo(Color::new(0.0, 0.0, 0.0));
        let high = scattering_albedo(Color::new(0.999, 0.999, 0.999));
        assert!(low.x.abs() < 1e-3 && high.x > 0.999);
        let mut last = -1.0;
        for i in 0..=20 {
            let a = scattering_albedo(Color::new(i as f32 / 20.0, 0.0, 0.0)).x;
            assert!(a >= last && a <= 1.0);
            last = a;
        }
    }

    #[test]
    fn rays_pass_through_holes_an_alpha_map_cuts() {
        let mut scene = test_scene(r#"{"lights":[],"objects":[{"type":"plane","points":[[-18,-18,5],[18,-18,5],[18,18,5],[-18,18,5]],"material":{"color":[1,1,1]}},{"type":"plane","points":[[-30,-30,9],[30,-30,9],[30,30,9],[-30,30,9]],"material":{"color":[1,1,1]}}]}"#);
        // Half of each texture repeat open, half solid, along u; a plane's texture repeats
        // every 18 units, so this plane spans two repeats.
        let pixels = (0..64).map(|i| { let v = if i % 8 < 4 { 0.0 } else { 1.0 }; Color::new(v, v, v) }).collect();
        scene.textures.push(crate::texture::Texture { width: 8, height: 8, image: pixels });
        scene.materials[0].maps.alpha = Some(scene.textures.len() - 1);
        scene.cutouts = true;
        let mut tracer = environment_tracer(&scene, 1);
        let mut through = 0;
        for i in 0..20 {
            let x = -16.15 + 1.7 * i as f32;
            let ray = Ray { o: Vec3::new(x, 0.3, 0.0), d: Vec3::new(0.0, 0.0, 1.0) };
            let (_, dist, _) = tracer.nearest(&ray, 100.0).expect("one of the planes");
            if dist > 6.0 { through += 1; }
        }
        assert!(through > 4 && through < 16, "{through} of 20 rays passed through the holes");
    }

    #[test]
    fn environment_ggx_mis_matches_single_brdf_estimator_including_depth_limit() {
        let mut scene = test_scene(r#"{"lights":[],"objects":[{"type":"plane","points":[[-100,-100,5],[100,-100,5],[100,100,5],[-100,100,5]],"material":{"color":[0.8,0.8,0.8],"surface":{"preset":"metal","roughness":0.6}}}]}"#);
        set_constant_environment(&mut scene, true);
        let ray = Ray { o: Vec3::new(0., 0., -5.), d: Vec3::new(0., 0., 1.) };
        let ggx = crate::ggx::Ggx::new(0.6, 1.);
        let n = Vec3::new(0., 0., -1.);
        let mut expected = 0.;
        let count = 30000;
        for i in 0..count {
            if let Some((_, weight)) = ggx.sample(n, n, (i as f32 + 0.5) / count as f32,
                0.37, 0.5, Color::new(0.8, 0.8, 0.8), 1.5) {
                expected += weight.x * 0.25 / count as f32;
            }
        }
        for depth in [1, 5] {
            let mut tracer = environment_tracer(&scene, depth);
            let mut total = 0.;
            for _ in 0..count { total += tracer.trace(&ray, 1, 0, true).0.x; }
            let measured = total / count as f32;
            assert!((measured - expected).abs() < 0.005, "depth={depth}: {measured} vs {expected}");
        }
    }

    #[test]
    fn photon_light_is_not_traced_again_after_a_diffuse_bounce() {
        let scene = test_scene(r#"{"lights":[{"type":"rectangle","position":[0,0,4],"edge_u":[2,0,0],"edge_v":[0,-2,0],"intensity":3}],"objects":[]}"#);
        let map = PhotonMap::new(Vec::new(), 0.1);
        let ray = Ray { o: Vec3::new(0., 0., -5.), d: Vec3::new(0., 0., 1.) };
        let mut tracer = environment_tracer(&scene, 5);
        assert_eq!(tracer.trace(&ray, 2, 1, true).0, Color::new(3., 3., 3.));
        tracer.caustics = Some(&map);
        assert_eq!(tracer.trace(&ray, 2, 1, true).0, Color::default());
        assert_eq!(tracer.trace(&ray, 2, 0, true).0, Color::new(3., 3., 3.), "mirrors still show the panel");
    }

    #[test]
    fn environment_through_glass_is_not_counted_twice_by_indirect_light() {
        let mut scene = test_scene(r#"{"lights":[],"objects":[
            {"type":"plane","points":[[-200,-2,-100],[200,-2,-100],[200,-2,300],[-200,-2,300]],"material":{"color":[0.8,0.8,0.8],"specular":0}},
            {"type":"sphere","center":[0,0,5],"radius":1.5,"material":{"color":[1,1,1],"diffuse":0,"specular":0,"refraction":1,"ior":1.5}}]}"#);
        set_constant_environment(&mut scene, false);
        let mut d = Vec3::new(0., -1.6, 10.);
        d.normalize();
        let ray = Ray { o: Vec3::new(0., 0., -5.), d };
        let mean = |indirect| {
            let mut tracer = environment_tracer(&scene, 6);
            tracer.has_glass = true;
            tracer.indirect = indirect;
            (0..40000).map(|_| tracer.trace(&ray, 1, 0, true).0.x).sum::<f32>() / 40000.
        };
        let (direct, both) = (mean(false), mean(true));
        // A little extra is real: the glass also reflects the environment onto the floor,
        // about 0.005 here at full bounce strength. Counting the environment twice
        // would add tens of times more.
        assert!(both <= direct + 0.008, "direct {direct}, with indirect {both}");
    }

    #[test]
    fn rectangle_emission_is_one_sided_occluded_and_not_counted_twice() {
        let text = r#"{"lights":[{"type":"rectangle","position":[0,0,4],"edge_u":[2,0,0],"edge_v":[0,-2,0],"intensity":3}],"objects":[]}"#;
        let scene = test_scene(text);
        assert_eq!(center_radiance(&scene, true), Color::new(3.0, 3.0, 3.0));
        assert_eq!(center_radiance(&scene, false), Color::default());
        let reversed = test_scene(&text.replace("[0,-2,0]", "[0,2,0]"));
        assert_eq!(center_radiance(&reversed, true), Color::default());
        let blocked = test_scene(&text.replace("\"objects\":[]", r#""objects":[{"type":"sphere","center":[0,0,1],"radius":1,"material":{"diffuse":0,"specular":0}}]"#));
        assert_eq!(center_radiance(&blocked, true), Color::default());
    }

    #[test]
    fn rectangle_lighting_scales_with_area_and_inverse_square_distance() {
        let text = r#"{"lights":[{"type":"rectangle","position":[2,0,0],"edge_u":[0.2,0,0],"edge_v":[0,0.2,0]}],"objects":[{"type":"plane","points":[[-10,-10,5],[10,-10,5],[10,10,5],[-10,10,5]],"material":{"specular":0}}]}"#;
        let near = center_radiance(&test_scene(text), true).x;
        let far = center_radiance(&test_scene(&text.replace("[2,0,0]", "[4,0,-5]")), true).x;
        let larger = center_radiance(&test_scene(&text.replace("[0.2,0,0]", "[0.4,0,0]")), true).x;
        assert!(near > 0.0);
        assert!((near / far - 4.0).abs() < 1e-4);
        assert!((larger / near - 2.0).abs() < 1e-4);
        let reversed = center_radiance(&test_scene(&text.replace("[0,0.2,0]", "[0,-0.2,0]")), true).x;
        assert_eq!(reversed, 0.0);
    }

    #[test]
    fn fog_dims_what_is_behind_it_by_its_transmittance() {
        // A lit wall behind a slab of black smoke, which absorbs and scatters nothing back.
        let text = r#"{"lights":[{"type":"rectangle","position":[0,0,4],"edge_u":[0.4,0,0],"edge_v":[0,-0.4,0],"intensity":3}],
            "objects":[{"type":"plane","points":[[-10,-10,5],[10,-10,5],[10,10,5],[-10,10,5]],"material":{"specular":0}},
            {"type":"volume","center":[0,0,0],"size":[4,4,2],"density":0.5,"color":[0,0,0]}]}"#;
        let ray = Ray { o: Vec3::new(0.0, 0.0, -5.0), d: Vec3::new(0.0, 0.0, 1.0) };
        let foggy = test_scene(text);
        let clear = test_scene(&text.replace("\"density\":0.5", "\"density\":0"));
        let seen = environment_tracer(&foggy, 1).trace(&ray, 1, 0, true).0.x;
        let plain = environment_tracer(&clear, 1).trace(&ray, 1, 0, true).0.x;
        assert!(plain > 0.0);
        // The camera ray crosses 2 units of smoke; the light does not.
        assert!((seen / plain - (-1.0f32).exp()).abs() < 1e-4, "{seen} {plain}");
        // Fixed mode keeps the 2008 renderer, which has no fog.
        let mut fixed = environment_tracer(&foggy, 1);
        fixed.fixed = true;
        let mut fixed_clear = environment_tracer(&clear, 1);
        fixed_clear.fixed = true;
        assert_eq!(fixed.trace(&ray, 1, 0, true).0, fixed_clear.trace(&ray, 1, 0, true).0);
    }

    #[test]
    fn fog_scatters_a_light_beside_the_ray_toward_the_camera() {
        // Nothing behind the fog: all the light seen comes from scattering.
        let text = r#"{"lights":[{"type":"sphere","position":[2,0,0],"radius":0.2,"intensity":4}],
            "objects":[{"type":"volume","center":[0,0,0],"radius":1,"density":0.02}]}"#;
        let ray = Ray { o: Vec3::new(0.0, 0.0, -5.0), d: Vec3::new(0.0, 0.0, 1.0) };
        let mean = |text: &str| {
            let scene = test_scene(text);
            let mut tracer = environment_tracer(&scene, 1);
            (0..4000).map(|_| tracer.trace(&ray, 1, 0, false).0.x).sum::<f32>() / 4000.0
        };
        let thin = mean(text);
        let thick = mean(&text.replace("\"density\":0.02", "\"density\":0.04"));
        assert!(thin > 0.0);
        // Thin fog scatters nearly in proportion to its density.
        assert!(thick / thin > 1.8 && thick / thin < 2.05, "{thin} {thick}");
        let forward = mean(&text.replace("\"density\":0.02", "\"density\":0.02,\"anisotropy\":0.8"));
        // Strong forward scattering sends little light sideways, toward this camera.
        assert!(forward < 0.5 * thin, "{forward} {thin}");
    }

    #[test]
    fn spherical_light_is_visible_occludes_and_is_not_counted_twice() {
        let text = r#"{"lights":[{"type":"sphere","position":[0,0,4],"radius":1,"intensity":3}],"objects":[]}"#;
        let scene = test_scene(text);
        assert_eq!(center_radiance(&scene, true), Color::new(3.0, 3.0, 3.0));
        assert_eq!(center_radiance(&scene, false), Color::default());
        let blocked = test_scene(&text.replace("\"objects\":[]", r#""objects":[{"type":"sphere","center":[0,0,1],"radius":1,"material":{"diffuse":0,"specular":0}}]"#));
        assert_eq!(center_radiance(&blocked, true), Color::default());
    }

    #[test]
    fn spherical_light_matches_the_exact_irradiance_of_a_sphere() {
        // A sphere of radiance L fully above the horizon gives E = L π (r/d)² cos θ,
        // so a white diffuse surface returns L (r/d)² cos θ.
        let text = r#"{"lights":[{"type":"sphere","position":[2,0,3],"radius":0.5,"intensity":1}],"objects":[{"type":"plane","points":[[-10,-10,5],[10,-10,5],[10,10,5],[-10,10,5]],"material":{"diffuse":1,"specular":0}}]}"#;
        let scene = test_scene(text);
        let expected = 0.25 / 8.0 * (2.0 / 8f32.sqrt());
        assert!((center_radiance(&scene, true).x - expected).abs() < 1e-5, "Fixed lights from the center");
        let mut tracer = environment_tracer(&scene, 1);
        let ray = Ray { o: Vec3::new(0.0, 0.0, -5.0), d: Vec3::new(0.0, 0.0, 1.0) };
        let runs = 20000;
        let mean = (0..runs).map(|_| tracer.trace(&ray, 1, 0, true).0.x).sum::<f32>() / runs as f32;
        assert!((mean / expected - 1.0).abs() < 0.03, "mean {mean}, expected {expected}");
    }

    #[test]
    fn bloom_spreads_only_light_above_white() {
        let image = Image::new(64, 64);
        for pixel in image.pixels.iter() { pixel.store(0xff20_2020, Relaxed); }
        let glow: Vec<AtomicU32> = (0..64 * 64 * 3).map(|_| AtomicU32::new(0)).collect();
        apply_bloom(&glow, None, 1, &image);
        assert!(image.pixels.iter().all(|p| p.load(Relaxed) == 0xff20_2020), "nothing above white, nothing changes");
        let center = (32 * 64 + 32) * 3;
        for k in 0..3 { glow[center + k].store(200f32.to_bits(), Relaxed); }
        apply_bloom(&glow, None, 1, &image);
        let red = |x: usize, y: usize| image.pixels[y * 64 + x].load(Relaxed) & 0xff;
        assert!(red(32, 32) > red(34, 32) && red(34, 32) > red(40, 32) && red(40, 32) > 0x20);
        assert_eq!(red(31, 32), red(33, 32), "the halo is symmetric");
        glow[center].store(f32::NAN.to_bits(), Relaxed);
        apply_bloom(&glow, None, 1, &image);
        assert!(red(60, 60) < 0xff, "an invalid value does not spread");
    }

    #[test]
    fn samples_keep_their_energy_before_the_curve() {
        // A bright source covering 1/32 of the samples has mean radiance 0.5: averaged before
        // the curve, it shows as 0.5 whatever the order of the samples.
        let red = |rgba: u32| rgba & 0xff;
        let display = Display { curve: ToneMapping::Standard, cap: None };
        let bright = Vec3::new(16., 0., 0.);
        let dark = Vec3::new(0., 0., 0.);
        let run = |display: Display, order: &[Vec3]| {
            let sum: Vec<AtomicU32> = (0..3).map(|_| AtomicU32::new(0)).collect();
            order.iter().enumerate().map(|(i, &c)| red(accumulate(&sum, 0, c, 1., i as u32 + 1, display))).last().unwrap()
        };
        let first: Vec<Vec3> = std::iter::once(bright).chain(std::iter::repeat_n(dark, 31)).collect();
        let last: Vec<Vec3> = std::iter::repeat_n(dark, 31).chain(std::iter::once(bright)).collect();
        assert_eq!(run(display, &first), 128);
        assert_eq!(run(display, &last), 128);
        // Without an HDR environment, a sample is capped at 6 before the average.
        assert_eq!(run(Display { cap: Some(MAX_SAMPLE_RADIANCE), ..display }, &first), 48);
    }

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

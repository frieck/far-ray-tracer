//! Ray tracing engine, Classic mode.
//!
//! Each thread has its own `Context`. In the original, barycentric coordinates
//! lived inside each triangle and the KD-Tree stack and random generator lived in
//! the engine, which ruled out rendering in parallel.

use crate::scene::{Scene, Sphere, Primitive, Triangle};
use crate::kdtree::KdTree;
use crate::twister::{Twister, DEFAULT_SEED};
use crate::vector::{Color, Vec3, EPSILON};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

const SAMPLES: usize = 128;
const MAX_TILE_SIZE: u32 = 32;
const AXIS_MOD: [usize; 5] = [0, 1, 2, 0, 1];
const NO_NODE: u32 = u32::MAX;
const STACK_SIZE: usize = 128;

/// `Classic` reproduces the 2008 program pixel by pixel, defects included.
/// `Fixed` keeps the 2008 algorithm and only removes its bugs.
/// `Enhanced` also adds distributed ray tracing and indirect light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Classic,
    Fixed,
    Enhanced,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Filters the finished image. Enhanced mode only.
    pub denoise: bool,
    /// Spreads very bright light into a soft halo. Enhanced mode only.
    pub bloom: bool,
    /// Camera overrides; None uses the scene settings. Enhanced mode only.
    pub pose: Option<crate::camera::Pose>,
    pub aperture: Option<f32>,
    pub focus_distance: Option<f32>,
    pub mode: Mode,
    /// Samples per pixel in Enhanced mode, which is also the number of passes.
    pub samples: u32,
    /// Turns on indirect light in Enhanced mode.
    pub indirect: bool,
    /// Indirect light settings in Enhanced mode; None picks a value from the scene's
    /// lights (see `Bounces::for_scene`).
    pub indirect_strength: Option<f32>,
    pub max_bounces: Option<u32>,
    /// Most light one rebound may bring back; infinity removes the limit.
    pub indirect_clamp: Option<f32>,
    /// Brightness multiplier in Enhanced mode.
    pub exposure: f32,
    /// Traces photons so glass and mirrors focus light, in Enhanced mode.
    pub caustics: bool,
    /// Photons emitted for caustics. More photons allow a smaller radius.
    pub photons: u32,
    /// Radius of the photon lookup. Smaller is sharper but noisier.
    pub caustic_radius: f32,
    /// Multiplier of the caustic light.
    pub caustic_strength: f32,
    /// Shows only the caustic light, to see what the photon map holds.
    pub caustics_only: bool,
    pub width: u32,
    pub height: u32,
    pub use_kd: bool,
    pub max_depth: i32,
    pub threads: usize,
    /// Uses a single random sequence in tile order, like the original.
    /// It only makes sense with one thread and exists for pixel-exact comparison.
    pub continuous_rng: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            denoise: false,
            bloom: false,
            pose: None,
            aperture: None,
            focus_distance: None,
            mode: Mode::Classic,
            samples: 16,
            indirect: true,
            indirect_strength: None,
            max_bounces: None,
            indirect_clamp: None,
            exposure: 1.0,
            caustics: false,
            photons: 1_000_000,
            caustic_radius: 0.12,
            caustic_strength: 1.0,
            caustics_only: false,
            width: 800,
            height: 600,
            use_kd: true,
            max_depth: 3,
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
            continuous_rng: false,
        }
    }
}

/// RGBA image that worker threads write while another thread may read it.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub(crate) pixels: Vec<AtomicU32>,
}

impl Image {
    pub fn new(width: u32, height: u32) -> Self {
        let pixels = (0..width as usize * height as usize).map(|_| AtomicU32::new(0xff00_0000)).collect();
        Self { width, height, pixels }
    }

    /// Copies the pixels as RGBA. `dest` must hold `width * height * 4` bytes.
    pub fn copy_rgba(&self, dest: &mut [u8]) {
        for (d, p) in dest.chunks_exact_mut(4).zip(&self.pixels) {
            d.copy_from_slice(&p.load(Ordering::Relaxed).to_le_bytes());
        }
    }

    pub fn rgba(&self) -> Vec<u8> {
        let mut v = vec![0; self.pixels.len() * 4];
        self.copy_rgba(&mut v);
        v
    }
}

/// What the photon pass did, for reports.
#[derive(Clone, Copy, Debug)]
pub struct PhotonStats {
    pub emitted: u32,
    pub stored: usize,
    pub trace_ms: u64,
    pub memory_bytes: usize,
}

#[derive(Default)]
pub struct Progress {
    pub tiles_done: AtomicUsize,
    pub tiles_total: AtomicUsize,
    /// Current pass in Enhanced mode, starting at 1.
    pub pass_index: AtomicUsize,
    pub rays: AtomicU64,
    pub cancel: AtomicBool,
    pub photon_stats: Mutex<Option<PhotonStats>>,
    /// Optional diagnostic point projection, consumed once by the desktop app.
    pub capture_photon_preview: AtomicBool,
    pub photon_preview: Mutex<Option<Image>>,
    /// Reads of the photon map during the render, and the photons they used.
    pub photon_lookups: AtomicU64,
    pub photons_gathered: AtomicU64,
}

/// Largest tile, up to 32 pixels, that divides the image exactly.
pub fn tile_size(width: u32, height: u32) -> u32 {
    (1..=MAX_TILE_SIZE).rev().find(|t| width % t == 0 && height % t == 0).unwrap_or(1)
}

/// Half size of the image plane. The original always uses 8 x 6, which stretches any
/// image that is not 4:3. Here the plane keeps those proportions as a minimum and grows
/// along one axis to match the image, so nothing distorts and 4:3 stays exactly as it was.
pub(crate) fn view_half_size(width: u32, height: u32) -> (f32, f32) {
    let (w, h) = (width as u64, height as u64);
    if w * 3 > h * 4 {
        (3.0 * width as f32 / height as f32, 3.0)
    } else if w * 3 < h * 4 {
        (4.0, 4.0 * height as f32 / width as f32)
    } else {
        (4.0, 3.0)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Ray {
    pub(crate) o: Vec3,
    pub(crate) d: Vec3,
}

#[derive(Clone, Copy, Default)]
struct StackEntry {
    node: u32,
    t: f32,
    pb: Vec3,
    prev: usize,
}

#[inline(always)]
fn intersect_sphere(e: &Sphere, r: &Ray, dist: &mut f32, corrected: bool) -> i32 {
    let oc = e.center - r.o;
    let l2oc = oc.dot(oc);
    let tca = oc.dot(r.d);
    // Bug of the original, kept for Classic mode: this test was meant for a ray outside
    // the sphere, but it stops rays inside it that head away from the center, which
    // do have a way out. The other cases already fall through to a miss below.
    if !corrected && l2oc < e.sq_radius && tca < 0.0 {
        return 0;
    }
    let t2hc = e.sq_radius - l2oc + (tca * tca);
    if t2hc < 0.0 {
        return 0;
    }
    let t2hc = t2hc.sqrt();
    let (i1, i2) = (tca - t2hc, tca + t2hc);
    if i2 > 0.0 {
        if i1 < 0.0 {
            if i2 < *dist {
                *dist = i2;
                return -1;
            }
        } else if i1 < *dist {
            *dist = i1;
            return 1;
        }
    }
    0
}

/// As in the original, this writes `uv` even when the ray misses. After a shadow
/// ray, a normal read again on the same triangle uses another ray's coordinates,
/// and that changes how meshes look. That is why `uv` is thread state instead of
/// an intersection result.
#[inline(always)]
fn intersect_triangle(t: &Triangle, uv: &mut [f32; 2], r: &Ray, dist: &mut f32) -> i32 {
    let (o, d, a) = (r.o.array(), r.d.array(), t.a.array());
    let (k, ku, kv) = (t.k, AXIS_MOD[t.k + 1], AXIS_MOD[t.k + 2]);
    let lnd = 1.0 / (d[k] + t.nu * d[ku] + t.nv * d[kv]);
    let dt = (t.nd - o[k] - t.nu * o[ku] - t.nv * o[kv]) * lnd;
    if !(*dist > dt && dt > 0.0) {
        return 0;
    }
    let hu = o[ku] + dt * d[ku] - a[ku];
    let hv = o[kv] + dt * d[kv] - a[kv];
    let beta = hv * t.bnu + hu * t.bnv;
    uv[0] = beta;
    if beta < 0.0 {
        return 0;
    }
    let gamma = hu * t.cnu + hv * t.cnv;
    uv[1] = gamma;
    if gamma < 0.0 {
        return 0;
    }
    if uv[0] + uv[1] > 1.0 {
        return 0;
    }
    *dist = dt;
    if r.d.dot(t.n) > 0.0 { -1 } else { 1 }
}

pub(crate) struct Context<'a> {
    pub(crate) scene: &'a Scene,
    pub(crate) kd: Option<&'a KdTree>,
    pub(crate) max_depth: i32,
    pub(crate) uv: Vec<[f32; 2]>,
    stack: [StackEntry; STACK_SIZE],
    pub(crate) rng: Twister,
    pub(crate) rays: u64,
    /// Fixed and Enhanced modes turn this on to use the corrected sphere test.
    pub(crate) corrected: bool,
}

impl<'a> Context<'a> {
    pub(crate) fn new(scene: &'a Scene, kd: Option<&'a KdTree>, max_depth: i32) -> Self {
        Self {
            scene,
            kd,
            max_depth,
            uv: vec![[0.0; 2]; scene.prims.len()],
            stack: [StackEntry::default(); STACK_SIZE],
            rng: Twister::new(DEFAULT_SEED),
            rays: 0,
            corrected: false,
        }
    }

    pub(crate) fn with_fixes(scene: &'a Scene, kd: Option<&'a KdTree>, max_depth: i32) -> Self {
        Self { corrected: true, ..Self::new(scene, kd, max_depth) }
    }

    #[inline(always)]
    pub(crate) fn intersect(&mut self, i: usize, r: &Ray, dist: &mut f32) -> i32 {
        match &self.scene.prims[i] {
            Primitive::Sphere(e) => intersect_sphere(e, r, dist, self.corrected),
            Primitive::Solid(s) => s.intersect(r.o, r.d, dist),
            Primitive::Triangle(t) => intersect_triangle(t, &mut self.uv[i], r, dist),
            Primitive::Ground(g) => match self.scene.materials[g.material].displacement {
                Some(d) => crate::displace::intersect_ground(self.scene, i, g, &d, r, dist),
                None => g.intersect(r.o, r.d, dist),
            },
        }
    }

    fn color(&self, i: usize, pos: Vec3) -> Color {
        match &self.scene.prims[i] {
            Primitive::Sphere(e) => self.scene.sphere_color(e, pos),
            Primitive::Solid(s) => self.scene.solid_color(s, pos),
            Primitive::Triangle(t) => self.scene.triangle_color(t, self.uv[i]),
            Primitive::Ground(g) => self.scene.ground_color(g, pos),
        }
    }

    fn normal(&self, i: usize, pos: Vec3) -> Vec3 {
        match &self.scene.prims[i] {
            Primitive::Sphere(e) => (pos - e.center) * e.inv_radius,
            Primitive::Solid(s) => s.normal(pos),
            Primitive::Triangle(t) => self.scene.triangle_normal(t, self.uv[i]),
            Primitive::Ground(g) => g.normal,
        }
    }

    /// Descends to the next leaf, pushing the far children.
    #[inline(always)]
    fn descend(&mut self, kd: &KdTree, mut node: u32, r: &Ray, entry: usize, exit: &mut usize) -> u32 {
        let (o, d) = (r.o.array(), r.d.array());
        loop {
            let n = kd.nodes[node as usize];
            if n.is_leaf() {
                return node;
            }
            let (posdiv, axis, esq) = (n.split_position(), n.axis(), n.left());
            let far_child;
            if self.stack[entry].pb.cell(axis) <= posdiv {
                if self.stack[*exit].pb.cell(axis) <= posdiv {
                    node = esq;
                    continue;
                }
                node = esq;
                far_child = esq + 1;
            } else {
                if self.stack[*exit].pb.cell(axis) > posdiv {
                    node = esq + 1;
                    continue;
                }
                far_child = esq;
                node = esq + 1;
            }
            let t = (posdiv - o[axis]) / d[axis];
            let previous = *exit;
            *exit += 1;
            if *exit == entry {
                *exit += 1;
            }
            let (next_axis, prev_axis) = (AXIS_MOD[axis + 1], AXIS_MOD[axis + 2]);
            let item = &mut self.stack[*exit];
            item.prev = previous;
            item.t = t;
            item.node = far_child;
            item.pb.set_cell(axis, posdiv);
            item.pb.set_cell(next_axis, o[next_axis] + t * d[next_axis]);
            item.pb.set_cell(prev_axis, o[prev_axis] + t * d[prev_axis]);
        }
    }

    /// Closest hit through the KD-Tree: returns the result and the primitive index.
    pub(crate) fn closest(&mut self, kd: &KdTree, r: &Ray, max_dist: &mut f32) -> (i32, usize) {
        // Infinite planes are outside the tree. Testing them first shortens the ray,
        // so the walk below stops at whatever is in front of them.
        let mut ground = (0, 0);
        for &i in &self.scene.grounds {
            let res = self.intersect(i, r, max_dist);
            if res != 0 {
                ground = (res, i);
            }
        }
        let (mut tnear, mut tfar) = (0.0f32, *max_dist);
        let p1 = self.scene.extends.pos.array();
        let p2 = (self.scene.extends.pos + self.scene.extends.size).array();
        let (d, o) = (r.d.array(), r.o.array());
        // The entry point below was only ever exercised from inside the scene, where it
        // is never used. A moved camera can start outside, so it needs a real box test.
        if self.corrected && (0..3).any(|i| o[i] < p1[i] || o[i] > p2[i]) {
            for i in 0..3 {
                if d[i] == 0.0 {
                    if o[i] < p1[i] || o[i] > p2[i] {
                        return ground;
                    }
                    continue;
                }
                let (a, b) = ((p1[i] - o[i]) / d[i], (p2[i] - o[i]) / d[i]);
                tnear = tnear.max(a.min(b));
                tfar = tfar.min(a.max(b));
            }
            if tnear > tfar {
                return ground;
            }
        } else {
            for i in 0..3 {
                if d[i] < 0.0 {
                    if o[i] < p1[i] {
                        return ground;
                    }
                } else if o[i] > p2[i] {
                    return ground;
                }
            }
            for i in 0..3 {
                let pos = o[i] + tfar * d[i];
                if d[i] < 0.0 {
                    if pos < p1[i] {
                        tfar = tnear + (tfar - tnear) * ((o[i] - p1[i]) / (o[i] - pos));
                    }
                    if o[i] > p2[i] {
                        tnear += (tfar - tnear) * ((o[i] - p2[i]) / (tfar * d[i]));
                    }
                } else {
                    if pos > p2[i] {
                        tfar = tnear + (tfar - tnear) * ((p2[i] - o[i]) / (pos - o[i]));
                    }
                    if o[i] < p1[i] {
                        tnear += (tfar - tnear) * ((p1[i] - o[i]) / (tfar * d[i]));
                    }
                }
                if tnear > tfar {
                    return ground;
                }
            }
        }

        let (mut entry, mut exit) = (0usize, 1usize);
        self.stack[0].t = tnear;
        self.stack[0].pb = if tnear > 0.0 { r.o + r.d * tnear } else { r.o };
        self.stack[1].t = tfar;
        self.stack[1].pb = r.o + r.d * tfar;
        self.stack[1].node = NO_NODE;
        let mut node = 0u32;
        while node != NO_NODE {
            node = self.descend(kd, node, r, entry, &mut exit);
            let mut dist = self.stack[exit].t;
            let (mut result, mut prim) = (0, 0);
            for k in kd.nodes[node as usize].span() {
                let i = kd.list[k] as usize;
                let res = self.intersect(i, r, &mut dist);
                if res != 0 {
                    result = res;
                    *max_dist = dist;
                    prim = i;
                }
            }
            if result != 0 {
                return (result, prim);
            }
            entry = exit;
            node = self.stack[exit].node;
            exit = self.stack[entry].prev;
        }
        ground
    }

    /// Looks for anything blocking the way to a light.
    pub(crate) fn occluded(&mut self, kd: &KdTree, r: &Ray, max_dist: f32) -> bool {
        for &i in &self.scene.grounds {
            let mut dist = max_dist;
            if self.intersect(i, r, &mut dist) != 0 {
                return true;
            }
        }
        let (mut entry, mut exit) = (0usize, 1usize);
        self.stack[0].t = EPSILON;
        self.stack[0].pb = r.o;
        self.stack[1].t = max_dist;
        self.stack[1].pb = r.o + r.d * max_dist;
        self.stack[1].node = NO_NODE;
        let mut node = 0u32;
        while node != NO_NODE {
            node = self.descend(kd, node, r, entry, &mut exit);
            let mut dist = max_dist;
            for k in kd.nodes[node as usize].span() {
                if self.intersect(kd.list[k] as usize, r, &mut dist) != 0 {
                    return true;
                }
            }
            entry = exit;
            node = self.stack[exit].node;
            exit = self.stack[entry].prev;
        }
        false
    }

    /// Shadow test for one light.
    fn shadow(&mut self, kd: &KdTree, light: &Sphere, ip: Vec3, dir: &mut Vec3) -> f32 {
        *dir = light.center - ip;
        let mut tdist = dir.length();
        *dir *= 1.0 / tdist;
        tdist *= 1.0 - 4.0 * EPSILON;
        self.rays += 1;
        let r = Ray { o: ip + *dir * EPSILON, d: *dir };
        if self.occluded(kd, &r, tdist) { 0.0 } else { 1.0 }
    }

    /// Path with the KD-Tree.
    fn raytrace_k(&mut self, kd: &KdTree, r: &Ray, acc: &mut Color, depth: i32, current_ior: f32, dist: &mut f32, sscale: f32) {
        let scene = self.scene;
        *dist = 10000.0;
        let (result, prim) = self.closest(kd, r, dist);
        if result == 0 {
            return;
        }
        let m = &scene.materials[scene.prims[prim].material()];
        let pi = r.o + r.d * *dist;
        let color = self.color(prim, pi);
        let n = self.normal(prim, pi);

        for light in scene.lights.iter().chain(scene.rect_lights.iter().map(|r| &r.proxy)).chain(&scene.bulbs) {
            let mut l = Vec3::default();
            let shade = self.shadow(kd, light, pi, &mut l);
            if shade > 0.0 {
                let light_color = scene.sphere_color(light, pi);
                if m.diff > 0.0 {
                    let dot = l.dot(n);
                    if dot > 0.0 {
                        let diff = dot * m.diff * shade;
                        *acc += diff * color * light_color;
                    }
                }
                if m.spec > 0.0 {
                    let reflected = l - 2.0 * l.dot(n) * n;
                    let dot = r.d.dot(reflected);
                    if dot > 0.0 {
                        let spec = dot * m.spec * shade / (50.0 - 50.0 * dot + dot);
                        *acc += spec * light_color;
                    }
                }
            }
        }

        let mut refl = m.refl;
        if refl > 0.0 && depth < self.max_depth {
            let drefl = m.drefl;
            if drefl > 0.0 && depth < 3 {
                let rp = r.d - 2.0 * r.d.dot(n) * n;
                let rn1 = Vec3::new(rp.z, rp.y, -rp.x);
                let rn2 = rp.cross(rn1);
                refl *= sscale;
                for _ in 0..SAMPLES {
                    let (mut xoffs, mut yoffs);
                    loop {
                        xoffs = (self.rng.rand() - 0.5) * drefl;
                        yoffs = (self.rng.rand() - 0.5) * drefl;
                        if xoffs * xoffs + yoffs * yoffs <= drefl * drefl {
                            break;
                        }
                    }
                    let mut d = rp + rn1 * xoffs + rn2 * yoffs * drefl;
                    d.normalize();
                    let (mut traced, mut traced_dist) = (Color::default(), 0.0);
                    self.raytrace_k(kd, &Ray { o: pi + d * EPSILON, d }, &mut traced, depth + 1, current_ior, &mut traced_dist, sscale * 4.0);
                    self.rays += 1;
                    *acc += refl * traced * color;
                }
            } else {
                let n2 = self.normal(prim, pi);
                let d = r.d - 2.0 * r.d.dot(n2) * n2;
                let (mut traced, mut traced_dist) = (Color::default(), 0.0);
                self.raytrace_k(kd, &Ray { o: pi + d * EPSILON, d }, &mut traced, depth + 1, current_ior, &mut traced_dist, sscale * 2.0);
                self.rays += 1;
                *acc += refl * traced * color;
            }
        }

        if m.refr > 0.0 && depth < self.max_depth {
            let ior = m.ior;
            let eta = current_ior / ior;
            let n2 = self.normal(prim, pi) * result as f32;
            let cos_i = -n2.dot(r.d);
            let cos_t2 = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
            if cos_t2 > 0.0 {
                let d = (eta * r.d) + (eta * cos_i - cos_t2.sqrt()) * n2;
                let (mut traced, mut traced_dist) = (Color::default(), 0.0);
                self.raytrace_k(kd, &Ray { o: pi + d * EPSILON, d }, &mut traced, depth + 1, ior, &mut traced_dist, sscale * 2.0);
                self.rays += 1;
                let absorbed = m.color * 0.15 * -traced_dist;
                *acc += traced * Color::new(absorbed.x.exp(), absorbed.y.exp(), absorbed.z.exp());
            }
        }
    }

    /// Brute force path, without the KD-Tree. Shading differs from the KD-Tree
    /// path, as it does in the original.
    fn raytrace(&mut self, r: &Ray, acc: &mut Color, depth: i32, current_ior: f32, max_dist: &mut f32) {
        if depth > self.max_depth {
            return;
        }
        let scene = self.scene;
        *max_dist = 1_000_000.0;
        let (mut prim, mut result) = (None, 0);
        for i in 0..scene.prims.len() {
            let res = self.intersect(i, r, max_dist);
            if res != 0 {
                prim = Some(i);
                result = res;
            }
        }
        let Some(prim) = prim else {
            *acc = Color::default();
            return;
        };

        let m = &scene.materials[scene.prims[prim].material()];
        let pi = r.o + r.d * *max_dist;
        let color = self.color(prim, pi);

        for light in scene.lights.iter().chain(scene.rect_lights.iter().map(|r| &r.proxy)).chain(&scene.bulbs) {
            let mut l = light.center - pi;
            let mut tdist = l.length();
            l *= 1.0 / tdist;
            let shadow = Ray { o: pi + l * EPSILON, d: l };
            let mut shade = 1.0f32;
            for s in 0..scene.prims.len() {
                if self.intersect(s, &shadow, &mut tdist) != 0 {
                    shade = 0.0;
                    break;
                }
            }
            if shade > 0.0 {
                let mut l = light.center - pi;
                l.normalize();
                let n = self.normal(prim, pi);
                let light_color = scene.materials[light.material].color;
                if m.diff > 0.0 {
                    let dot = l.dot(n);
                    if dot > 0.0 {
                        let diff = dot * m.diff * shade;
                        *acc += diff * color * light_color;
                    }
                }
                if m.spec > 0.0 {
                    let reflected = l - 2.0 * l.dot(n) * n;
                    let dot = r.d.dot(reflected);
                    if dot > 0.0 {
                        let spec = dot.powf(20.0) * m.spec * shade;
                        *acc += spec * light_color;
                    }
                }
            }
        }

        let mut refl = m.refl;
        if refl > 0.0 && depth < self.max_depth {
            let drefl = m.drefl;
            if drefl > 0.0 && depth < 2 {
                let n = self.normal(prim, pi);
                let rp = r.d - 2.0 * r.d.dot(n) * n;
                let rn1 = Vec3::new(rp.z, rp.y, -rp.x);
                let rn2 = rp.cross(rn1);
                // The original reads a field here that is never initialized (`m_SScale`).
                // The mean of the samples is the only value that makes sense.
                refl *= 1.0 / SAMPLES as f32;
                for _ in 0..SAMPLES {
                    let (mut xoffs, mut yoffs);
                    loop {
                        xoffs = self.rng.rand() * drefl;
                        yoffs = self.rng.rand() * drefl;
                        if xoffs * xoffs + yoffs * yoffs <= drefl * drefl {
                            break;
                        }
                    }
                    let mut d = rp + rn1 * xoffs + rn2 * yoffs * drefl;
                    d.normalize();
                    let (mut traced, mut traced_dist) = (Color::default(), 0.0);
                    self.raytrace(&Ray { o: pi + d * EPSILON, d }, &mut traced, depth + 1, current_ior, &mut traced_dist);
                    *acc += refl * traced * color;
                }
            } else {
                let n = self.normal(prim, pi);
                let d = r.d - 2.0 * r.d.dot(n) * n;
                let (mut traced, mut traced_dist) = (Color::default(), 0.0);
                self.raytrace(&Ray { o: pi + d * EPSILON, d }, &mut traced, depth + 1, current_ior, &mut traced_dist);
                *acc += refl * traced * color;
            }
        }

        if m.refr > 0.0 && depth < self.max_depth {
            let ior = m.ior;
            let eta = current_ior / ior;
            let n = self.normal(prim, pi) * result as f32;
            let cos_i = -n.dot(r.d);
            let cos_t2 = 1.0 - eta * eta * (1.0 - cos_i * cos_i);
            if cos_t2 > 0.0 {
                let d = (eta * r.d) + (eta * cos_i - cos_t2.sqrt()) * n;
                let (mut traced, mut traced_dist) = (Color::default(), 0.0);
                self.raytrace(&Ray { o: pi + d * EPSILON, d }, &mut traced, depth + 1, ior, &mut traced_dist);
                let absorbed = m.color * 0.15 * -traced_dist;
                *acc += traced * Color::new(absorbed.x.exp(), absorbed.y.exp(), absorbed.z.exp());
            }
        }
    }

    /// Traces the primary ray of a pixel.
    fn pixel(&mut self, pos: Vec3) -> Color {
        let origin = Vec3::new(0.0, 0.0, -5.0);
        let mut d = pos - origin;
        d.normalize();
        let r = Ray { o: origin, d };
        self.rays += 1;
        let (mut acc, mut dist) = (Color::default(), 0.0);
        match self.kd {
            Some(kd) => self.raytrace_k(kd, &r, &mut acc, 1, 1.0, &mut dist, 1.0 / SAMPLES as f32),
            None => self.raytrace(&r, &mut acc, 1, 1.0, &mut dist),
        }
        acc
    }
}

#[inline]
pub(crate) fn channel(v: f32) -> u32 {
    let c = v * 256.0;
    (if c > 255.0 { 255.0 } else { c }) as u8 as u32
}

/// Renders the scene, writing each tile to `image` as soon as it finishes.
/// Tiles run in reading order and each thread takes the next free one.
pub fn render(scene: &Scene, kd: Option<&KdTree>, op: &Options, image: &Image, progress: &Progress) {
    if op.mode != Mode::Classic {
        crate::enhanced::render(scene, kd, op, image, progress);
        return;
    }
    let (w, h) = (op.width, op.height);
    let tile = tile_size(w, h);
    let (tiles_x, tiles_y) = (w / tile, h / tile);
    let total = (tiles_x * tiles_y) as usize;
    progress.tiles_done.store(0, Ordering::Relaxed);
    progress.tiles_total.store(total, Ordering::Relaxed);
    progress.rays.store(0, Ordering::Relaxed);

    // The camera is fixed, as in the original.
    let (half_width, half_height) = view_half_size(w, h);
    let p1 = Vec3::new(-half_width, half_height, 0.0);
    let p2 = Vec3::new(half_width, half_height, 0.0);
    let p4 = Vec3::new(-half_width, -half_height, 0.0);
    let dx = (p2 - p1) * (1.0 / w as f32);
    let dy = (p4 - p1) * (1.0 / h as f32);

    let next_tile = AtomicUsize::new(0);
    let worker = || {
        let mut ctx = Context::new(scene, kd, op.max_depth);
        loop {
            if progress.cancel.load(Ordering::Relaxed) {
                break;
            }
            let i = next_tile.fetch_add(1, Ordering::Relaxed);
            if i >= total {
                break;
            }
            if !op.continuous_rng {
                // The seed depends only on the tile, so the image does not change with the thread count.
                ctx.rng = Twister::new(DEFAULT_SEED ^ (i as u32).wrapping_mul(0x9e37_79b9));
            }
            let (tx, ty) = (i as u32 % tiles_x, i as u32 / tiles_x);
            // Sums in the original order so the points come out identical.
            let mut tdir = p1 + (ty * tile) as f32 * dy;
            for _ in 0..tx {
                tdir += dx * tile as f32;
            }
            let mut ldir = tdir;
            for y in 0..tile {
                let mut pdir = ldir;
                let row = ((ty * tile + y) * w + tx * tile) as usize;
                for x in 0..tile as usize {
                    let c = ctx.pixel(pdir);
                    let rgba = channel(c.x) | channel(c.y) << 8 | channel(c.z) << 16 | 0xff00_0000;
                    image.pixels[row + x].store(rgba, Ordering::Relaxed);
                    pdir += dx;
                }
                ldir += dy;
            }
            progress.tiles_done.fetch_add(1, Ordering::Relaxed);
            progress.rays.fetch_add(ctx.rays, Ordering::Relaxed);
            ctx.rays = 0;
        }
    };

    std::thread::scope(|s| {
        for _ in 1..op.threads.max(1) {
            s.spawn(worker);
        }
        worker();
    });
}

#[cfg(test)]
mod tests {
    use super::{view_half_size, Context, Ray};
    use crate::vector::Vec3;
    use crate::{KdTree, Scene};

    #[test]
    fn the_corrected_tree_keeps_a_wall_that_lies_on_a_split_plane() {
        // The back wall of the Cornell box sits exactly on a split plane. Rounding the
        // child boxes left it out of both of them, so rays went through it.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/cornell-box.far");
        let scene = Scene::open(std::path::Path::new(path)).unwrap();
        let o = Vec3::new(0.0, 0.0, -9.6);
        let misses = |kd: &KdTree| {
            let mut ctx = Context::with_fixes(&scene, Some(kd), 1);
            let mut misses = 0;
            for i in 0..40 {
                for j in 0..40 {
                    let target = Vec3::new(-2.6 + 0.13 * i as f32, -2.6 + 0.13 * j as f32, 5.5);
                    let mut d = target - o;
                    d.normalize();
                    let r = Ray { o, d };
                    let (mut tree, mut brute) = (10000.0, 10000.0);
                    let found = ctx.closest(kd, &r, &mut tree).0 != 0;
                    for k in 0..scene.prims.len() {
                        ctx.intersect(k, &r, &mut brute);
                    }
                    if !found || (tree - brute).abs() > 1e-3 {
                        misses += 1;
                    }
                }
            }
            misses
        };
        assert!(misses(&KdTree::build(&scene)) > 0, "the original tree loses the wall; Classic keeps that");
        assert_eq!(misses(&KdTree::build_corrected(&scene)), 0);
    }

    #[test]
    fn a_large_displaced_scene_finds_the_same_hits_with_and_without_the_tree() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/materials/displacement.far");
        let scene = Scene::open(std::path::Path::new(path)).unwrap();
        let kd = KdTree::build_corrected(&scene);
        let mut ctx = Context::with_fixes(&scene, Some(&kd), 1);
        let o = Vec3::new(0.0, 2.1, -6.4);
        let mut wrong = Vec::new();
        for target in [Vec3::new(-1.6, 0.65, 0.6), Vec3::new(0.9, 0.15, 1.0), Vec3::new(2.5, 0.05, -0.6)] {
            let mut d = target - o;
            d.normalize();
            let r = Ray { o, d };
            let (mut tree, mut brute) = (10000.0, 10000.0);
            let (_, prim) = ctx.closest(&kd, &r, &mut tree);
            let mut nearest = 0;
            for k in 0..scene.prims.len() {
                if ctx.intersect(k, &r, &mut brute) != 0 {
                    nearest = k;
                }
            }
            if (tree - brute).abs() > 1e-3 {
                wrong.push((target.array(), tree, prim, brute, nearest, scene.grounds.clone()));
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    #[test]
    fn the_ground_is_hit_outside_the_tree_and_hides_nothing_in_front_of_it() {
        let text = br#"{"lights":[],"objects":[{"type":"ground","position":[0,-2,0]},{"type":"sphere","center":[0,0,5],"radius":1}]}"#;
        let scene = Scene::from_far(text, std::path::Path::new("test.far")).unwrap();
        assert_eq!(scene.grounds, [0]);
        let kd = KdTree::build(&scene);
        assert!(!kd.list.contains(&0));
        let mut down = Vec3::new(0.0, -1.0, 1.0);
        down.normalize();
        for mut ctx in [Context::new(&scene, Some(&kd), 1), Context::with_fixes(&scene, Some(&kd), 1)] {
            // Far outside the scene bounds, where the tree has nothing: only the ground.
            let mut dist = 10000.0;
            let far_away = Ray { o: Vec3::new(500.0, 8.0, 900.0), d: down };
            assert_eq!(ctx.closest(&kd, &far_away, &mut dist), (1, 0));
            assert!((dist - 10.0 * 2f32.sqrt()).abs() < 1e-3);
            // The sphere is in front of the ground, so it wins.
            let mut dist = 10000.0;
            let at_sphere = Ray { o: Vec3::new(0.0, 0.0, 0.0), d: Vec3::new(0.0, 0.0, 1.0) };
            assert_eq!(ctx.closest(&kd, &at_sphere, &mut dist), (1, 1));
            assert!((dist - 4.0).abs() < 1e-4);
            // From below, the ground is still solid.
            let mut dist = 10000.0;
            let up = Ray { o: Vec3::new(3.0, -5.0, 0.0), d: Vec3::new(0.0, 1.0, 0.0) };
            assert_eq!(ctx.closest(&kd, &up, &mut dist), (-1, 0));
            assert!(ctx.occluded(&kd, &up, 10.0));
            assert!(!ctx.occluded(&kd, &up, 2.0));
        }
    }

    #[test]
    fn four_by_three_keeps_the_original_plane() {
        assert_eq!(view_half_size(800, 600), (4.0, 3.0));
        assert_eq!(view_half_size(1600, 1200), (4.0, 3.0));
    }

    #[test]
    fn other_proportions_grow_one_axis_without_distortion() {
        let (w, h) = view_half_size(1920, 1080);
        assert_eq!(h, 3.0);
        assert!((w / h - 1920.0 / 1080.0).abs() < 1e-5);
        let (w, h) = view_half_size(600, 800);
        assert_eq!(w, 4.0);
        assert!((w / h - 600.0 / 800.0).abs() < 1e-5);
    }
}


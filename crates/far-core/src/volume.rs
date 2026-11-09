//! Participating media: fog, smoke and haze that light scatters in, which turns beams of light
//! into visible shafts.
//!
//! Each volume is a medium in a box or a sphere. `density` is the extinction
//! coefficient, the chance per scene unit that light hits a particle; `color` is the albedo,
//! the share of that light scattered rather than absorbed; `anisotropy` is the Henyey-Greenstein
//! g, from back scattering (negative) through even (0) to forward scattering (positive), as in
//! haze lit from behind. Where volumes overlap their densities add.
//!
//! A volume is even unless it thins with height (`falloff`, as ground fog and haze do) or is
//! shaped by noise (`noise`, for smoke and clouds). An even volume and one that only thins
//! with height are integrated exactly; noise is integrated by marching along the ray.

use crate::vector::{Color, Vec3, PI};

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    /// `axes` are the box's unit axes after rotation; `half` its half sizes along them.
    Box { center: Vec3, axes: [Vec3; 3], half: Vec3 },
    Sphere { center: Vec3, radius: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Volume {
    pub shape: Shape,
    pub density: f32,
    pub albedo: Color,
    pub anisotropy: f32,
    /// The density falls by e every `scale` units above `base`, the height where it is
    /// `density`: (base, scale).
    pub falloff: Option<(f32, f32)>,
    pub noise: Option<Noise>,
}

/// Billowing density from fractal noise: puffs about `scale` units across, where `coverage`
/// is the share of space they fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    pub scale: f32,
    pub octaves: u32,
    pub coverage: f32,
    pub seed: u32,
}

impl Noise {
    fn factor(&self, p: Vec3) -> f32 {
        let p = p * (1.0 / self.scale);
        let (mut sum, mut amplitude, mut frequency, mut norm) = (0.0, 1.0, 1.0, 0.0);
        for octave in 0..self.octaves {
            sum += amplitude * gradient_noise(p * frequency, self.seed.wrapping_add(octave * 7919));
            norm += amplitude;
            amplitude *= 0.5;
            frequency *= 2.03;
        }
        // Fractal gradient noise keeps close to zero; spread it over [0, 1], so that about
        // `coverage` of space lies above the threshold, with a soft edge around it.
        let n = (0.5 + 1.6 * sum / norm).clamp(0.0, 1.0);
        let t = ((n - (1.0 - self.coverage)) / 0.2 + 0.5).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

fn hash3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6b343) ^ (y as u32).wrapping_mul(0xd8163841)
        ^ (z as u32).wrapping_mul(0xcb1ab31f) ^ seed.wrapping_mul(0x9e3779b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a2d39);
    h ^ (h >> 15)
}

/// Perlin's gradient noise: smooth, about zero on average, near [-0.7, 0.7].
fn gradient_noise(p: Vec3, seed: u32) -> f32 {
    const GRADIENTS: [[f32; 3]; 12] = [[1., 1., 0.], [-1., 1., 0.], [1., -1., 0.], [-1., -1., 0.], [1., 0., 1.], [-1., 0., 1.],
        [1., 0., -1.], [-1., 0., -1.], [0., 1., 1.], [0., -1., 1.], [0., 1., -1.], [0., -1., -1.]];
    let (fx, fy, fz) = (p.x.floor(), p.y.floor(), p.z.floor());
    let (x, y, z) = (fx as i32, fy as i32, fz as i32);
    let (tx, ty, tz) = (p.x - fx, p.y - fy, p.z - fz);
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v, w) = (fade(tx), fade(ty), fade(tz));
    let corner = |i: i32, j: i32, k: i32| {
        let g = GRADIENTS[(hash3(x + i, y + j, z + k, seed) % 12) as usize];
        g[0] * (tx - i as f32) + g[1] * (ty - j as f32) + g[2] * (tz - k as f32)
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    lerp(
        lerp(lerp(corner(0, 0, 0), corner(1, 0, 0), u), lerp(corner(0, 1, 0), corner(1, 1, 0), u), v),
        lerp(lerp(corner(0, 0, 1), corner(1, 0, 1), u), lerp(corner(0, 1, 1), corner(1, 1, 1), u), v),
        w,
    )
}

impl Volume {
    /// An even volume.
    pub fn even(shape: Shape, density: f32, albedo: Color, anisotropy: f32) -> Self {
        Self { shape, density, albedo, anisotropy, falloff: None, noise: None }
    }

    /// The lowest height inside the volume.
    pub fn bottom(&self) -> f32 {
        match self.shape {
            Shape::Sphere { center, radius } => center.y - radius,
            Shape::Box { center, axes, half } => {
                center.y - axes[0].y.abs() * half.x - axes[1].y.abs() * half.y - axes[2].y.abs() * half.z
            }
        }
    }

    /// The density at a point inside the volume.
    pub fn density_at(&self, p: Vec3) -> f32 {
        let mut density = self.density;
        if let Some((base, scale)) = self.falloff {
            density *= (-(p.y - base) / scale).exp();
        }
        if let Some(noise) = &self.noise {
            density *= noise.factor(p) * self.edge_fade(p);
        }
        density
    }

    /// Smoke and cloud thin out toward the edges of their box or sphere, so its shape never
    /// shows: 1 inside, falling to 0 over the outer third.
    fn edge_fade(&self, p: Vec3) -> f32 {
        let fade = |x: f32| {
            let t = ((1.0 - x) / 0.35).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        match self.shape {
            Shape::Sphere { center, radius } => fade((p - center).length() / radius),
            Shape::Box { center, axes, half } => {
                let rel = p - center;
                axes.iter().zip(half.array()).map(|(axis, size)| fade(rel.dot(*axis).abs() / size)).product()
            }
        }
    }

    /// The optical depth along the ray between distances `a` and `b`, both inside the volume,
    /// when it can be computed exactly.
    fn exact_depth(&self, o: Vec3, d: Vec3, a: f32, b: f32) -> Option<f32> {
        if self.noise.is_some() {
            return None;
        }
        Some(match self.falloff {
            None => self.density * (b - a),
            Some((base, scale)) => {
                let at = |t: f32| (-(o.y + d.y * t - base) / scale).exp();
                if d.y.abs() < 1e-6 {
                    self.density * at(a) * (b - a)
                } else {
                    self.density * scale / d.y * (at(a) - at(b))
                }
            }
        })
    }

    /// The part of the ray from `o` along unit `d`, between 0 and `limit`, inside the volume.
    pub fn interval(&self, o: Vec3, d: Vec3, limit: f32) -> Option<(f32, f32)> {
        let (mut near, mut far) = (0.0f32, limit);
        match self.shape {
            Shape::Sphere { center, radius } => {
                let rel = o - center;
                let b = rel.dot(d);
                let c = rel.dot(rel) - radius * radius;
                let disc = b * b - c;
                if disc <= 0.0 {
                    return None;
                }
                let root = disc.sqrt();
                near = near.max(-b - root);
                far = far.min(-b + root);
            }
            Shape::Box { center, axes, half } => {
                let rel = o - center;
                for (axis, size) in axes.iter().zip(half.array()) {
                    let (start, speed) = (rel.dot(*axis), d.dot(*axis));
                    if speed.abs() < 1e-12 {
                        if start.abs() > size {
                            return None;
                        }
                        continue;
                    }
                    let (a, b) = ((-size - start) / speed, (size - start) / speed);
                    near = near.max(a.min(b));
                    far = far.min(a.max(b));
                }
            }
        }
        (near < far).then_some((near, far))
    }
}

/// Optical depth of the volumes along the ray up to `limit`: light keeps `exp(-depth)`.
pub fn optical_depth(volumes: &[Volume], o: Vec3, d: Vec3, limit: f32) -> f32 {
    if volumes.iter().all(|v| v.noise.is_none() && v.falloff.is_none()) {
        return volumes.iter().filter_map(|v| v.interval(o, d, limit).map(|(a, b)| v.density * (b - a))).sum();
    }
    match march(volumes, o, d, limit, f32::INFINITY, 0.0) {
        March::Through(depth) => depth,
        March::Stopped(..) => unreachable!("an infinite target is never reached"),
    }
}

/// Steps per volume crossing when its density varies; noise also takes at least four steps per
/// puff.
const MARCH_STEPS: f32 = 48.0;
const MAX_MARCH_STEPS: usize = 1024;

enum March {
    /// The ray left every volume with this optical depth.
    Through(f32),
    /// The depth reached the target at this distance, in this volume.
    Stopped(f32, usize),
}

/// Walks the ray through volumes whose density varies, adding up optical depth until it
/// reaches `target`. Parts it can integrate exactly it adds at once; noise it samples at the
/// middle of short steps, and inside a step the depth grows evenly.
fn march(volumes: &[Volume], o: Vec3, d: Vec3, limit: f32, target: f32, pick: f32) -> March {
    let spans: Vec<(f32, f32, usize)> = volumes.iter().enumerate()
        .filter(|(_, v)| v.density > 0.0)
        .filter_map(|(i, v)| v.interval(o, d, limit).map(|(a, b)| (a, b, i)))
        .collect();
    let mut stops: Vec<f32> = spans.iter().flat_map(|&(a, b, _)| [a, b]).collect();
    stops.sort_by(f32::total_cmp);
    let mut depth = 0.0;
    for pair in stops.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if end <= start {
            continue;
        }
        let middle = 0.5 * (start + end);
        let inside: Vec<usize> = spans.iter().filter(|&&(a, b, _)| a <= middle && middle < b).map(|s| s.2).collect();
        if inside.is_empty() {
            continue;
        }
        let step = inside.iter().map(|&i| {
            let v = &volumes[i];
            let crossing = spans.iter().find(|s| s.2 == i).map_or(end - start, |s| s.1 - s.0);
            let mut step = crossing / MARCH_STEPS;
            if let Some(noise) = &v.noise { step = step.min(noise.scale * 0.35); }
            step
        }).fold(f32::INFINITY, f32::min);
        let exact = inside.iter().all(|&i| volumes[i].noise.is_none());
        let steps = if exact { 1 } else { (((end - start) / step).ceil() as usize).clamp(1, MAX_MARCH_STEPS) };
        let dt = (end - start) / steps as f32;
        for k in 0..steps {
            let (a, b) = (start + k as f32 * dt, start + (k + 1) as f32 * dt);
            let p = o + d * (0.5 * (a + b));
            let densities: Vec<f32> = inside.iter().map(|&i| volumes[i].density_at(p)).collect();
            let added: f32 = if exact {
                inside.iter().map(|&i| volumes[i].exact_depth(o, d, a, b).unwrap_or(0.0)).sum()
            } else {
                densities.iter().sum::<f32>() * (b - a)
            };
            if depth + added >= target && added > 0.0 {
                let share = (target - depth) / added;
                // Within an exact falloff span, invert its integral; otherwise move evenly.
                let t = if exact && inside.len() == 1 {
                    invert_exact(&volumes[inside[0]], o, d, a, b, target - depth).unwrap_or(a + share * (b - a))
                } else {
                    a + share * (b - a)
                };
                let total: f32 = densities.iter().sum();
                let mut left = pick * total;
                let mut chosen = inside[0];
                for (&i, &density) in inside.iter().zip(&densities) {
                    chosen = i;
                    left -= density;
                    if left < 0.0 { break; }
                }
                return March::Stopped(t, chosen);
            }
            depth += added;
        }
    }
    March::Through(depth)
}

/// The distance after `a` where an exactly integrated volume adds `depth`.
fn invert_exact(v: &Volume, o: Vec3, d: Vec3, a: f32, b: f32, depth: f32) -> Option<f32> {
    match v.falloff {
        None => Some(a + depth / v.density),
        Some((base, scale)) => {
            let at = |t: f32| (-(o.y + d.y * t - base) / scale).exp();
            if d.y.abs() < 1e-6 {
                return Some(a + depth / (v.density * at(a)));
            }
            // density * scale / dy * (at(a) - at(t)) = depth.
            let inner = at(a) - depth * d.y / (v.density * scale);
            (inner > 0.0).then(|| base - scale * inner.ln() - o.y).map(|y| (y / d.y).clamp(a, b))
        }
    }
}

/// Where along the ray the optical depth reaches `target`, and in which volume the light
/// scatters there, picked by its share of the density with `pick` in [0, 1). None if the ray
/// leaves every volume before `limit` with less depth than that.
pub fn distance_at_depth(volumes: &[Volume], o: Vec3, d: Vec3, limit: f32, target: f32, pick: f32)
    -> Option<(f32, usize)> {
    if volumes.iter().any(|v| v.noise.is_some() || v.falloff.is_some()) {
        return match march(volumes, o, d, limit, target, pick) {
            March::Stopped(t, i) => Some((t, i)),
            March::Through(_) => None,
        };
    }
    let spans: Vec<(f32, f32, usize)> = volumes.iter().enumerate()
        .filter(|(_, v)| v.density > 0.0)
        .filter_map(|(i, v)| v.interval(o, d, limit).map(|(a, b)| (a, b, i)))
        .collect();
    if spans.is_empty() {
        return None;
    }
    let mut stops: Vec<f32> = spans.iter().flat_map(|&(a, b, _)| [a, b]).collect();
    stops.sort_by(f32::total_cmp);
    let mut left = target;
    for pair in stops.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if end <= start {
            continue;
        }
        let middle = 0.5 * (start + end);
        let inside = || spans.iter().filter(move |&&(a, b, _)| a <= middle && middle < b);
        let density: f32 = inside().map(|&(_, _, i)| volumes[i].density).sum();
        if density <= 0.0 {
            continue;
        }
        let depth = density * (end - start);
        if left < depth {
            let t = start + left / density;
            let mut share = pick * density;
            let mut chosen = None;
            for &(_, _, i) in inside() {
                chosen = Some(i);
                share -= volumes[i].density;
                if share < 0.0 {
                    break;
                }
            }
            return chosen.map(|i| (t, i));
        }
        left -= depth;
    }
    None
}

/// Henyey-Greenstein phase function: the density of scattering by an angle whose cosine is
/// `cos`, measured between the light's travel before and after. It integrates to 1 over the
/// sphere.
pub fn phase(cos: f32, g: f32) -> f32 {
    let denom = (1.0 + g * g - 2.0 * g * cos).max(1e-6);
    (1.0 - g * g) / (4.0 * PI * denom * denom.sqrt())
}

/// A direction scattered from travel direction `d`, distributed by the phase function.
pub fn sample_phase(d: Vec3, g: f32, u: f32, v: f32) -> Vec3 {
    let cos = if g.abs() < 1e-3 {
        1.0 - 2.0 * u
    } else {
        let s = (1.0 - g * g) / (1.0 - g + 2.0 * g * u);
        ((1.0 + g * g - s * s) / (2.0 * g)).clamp(-1.0, 1.0)
    };
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    let angle = 2.0 * PI * v;
    let helper = if d.x.abs() > 0.9 { Vec3::new(0.0, 1.0, 0.0) } else { Vec3::new(1.0, 0.0, 0.0) };
    let mut t = helper.cross(d);
    t.normalize();
    let b = d.cross(t);
    t * (sin * angle.cos()) + b * (sin * angle.sin()) + d * cos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fog(shape: Shape, density: f32) -> Volume {
        Volume::even(shape, density, Color::new(1.0, 1.0, 1.0), 0.0)
    }

    fn unit_box(center: Vec3) -> Shape {
        Shape::Box { center, axes: [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)],
            half: Vec3::new(1.0, 1.0, 1.0) }
    }

    #[test]
    fn intervals_clip_to_the_shape_and_the_ray() {
        let d = Vec3::new(0.0, 0.0, 1.0);
        let sphere = fog(Shape::Sphere { center: Vec3::new(0.0, 0.0, 5.0), radius: 1.0 }, 1.0);
        assert_eq!(sphere.interval(Vec3::default(), d, 100.0), Some((4.0, 6.0)));
        assert_eq!(sphere.interval(Vec3::default(), d, 5.0), Some((4.0, 5.0)));
        assert_eq!(sphere.interval(Vec3::new(0.0, 0.0, 5.0), d, 100.0), Some((0.0, 1.0)));
        assert_eq!(sphere.interval(Vec3::new(3.0, 0.0, 0.0), d, 100.0), None);
        let cube = fog(unit_box(Vec3::new(0.0, 0.0, 5.0)), 1.0);
        assert_eq!(cube.interval(Vec3::default(), d, 100.0), Some((4.0, 6.0)));
        assert_eq!(cube.interval(Vec3::new(0.0, 0.0, 10.0), d, 100.0), None);
    }

    #[test]
    fn depths_add_where_volumes_overlap_and_sampling_inverts_them() {
        let d = Vec3::new(0.0, 0.0, 1.0);
        let volumes = [fog(unit_box(Vec3::new(0.0, 0.0, 5.0)), 0.5), fog(unit_box(Vec3::new(0.0, 0.0, 6.0)), 1.5)];
        // [4, 5) at 0.5, [5, 6) at 2, [6, 7) at 1.5.
        let total = optical_depth(&volumes, Vec3::default(), d, 100.0);
        assert!((total - 4.0).abs() < 1e-5);
        assert_eq!(distance_at_depth(&volumes, Vec3::default(), d, 100.0, 0.25, 0.0), Some((4.5, 0)));
        let (t, _) = distance_at_depth(&volumes, Vec3::default(), d, 100.0, 1.5, 0.0).unwrap();
        assert!((t - 5.5).abs() < 1e-5);
        // In the overlap the pick chooses by density: a quarter for the first volume.
        assert_eq!(distance_at_depth(&volumes, Vec3::default(), d, 100.0, 1.5, 0.2).unwrap().1, 0);
        assert_eq!(distance_at_depth(&volumes, Vec3::default(), d, 100.0, 1.5, 0.3).unwrap().1, 1);
        assert_eq!(distance_at_depth(&volumes, Vec3::default(), d, 100.0, 4.5, 0.0), None);
        for target in [0.1, 0.9, 2.2, 3.9] {
            let (t, _) = distance_at_depth(&volumes, Vec3::default(), d, 100.0, target, 0.5).unwrap();
            assert!((optical_depth(&volumes, Vec3::default(), d, t) - target).abs() < 1e-4);
        }
    }

    #[test]
    fn phase_integrates_to_one_and_sampling_follows_it() {
        for g in [-0.6, 0.0, 0.3, 0.85] {
            let n = 20000;
            let integral: f32 = (0..n).map(|i| {
                let cos = -1.0 + 2.0 * (i as f32 + 0.5) / n as f32;
                phase(cos, g) * 2.0 * PI * (2.0 / n as f32)
            }).sum();
            assert!((integral - 1.0).abs() < 1e-3, "g {g}: {integral}");
            // The mean cosine of sampled directions is g.
            let d = Vec3::new(0.0, 0.6, 0.8);
            let samples = 4000;
            let mean: f32 = (0..samples).map(|i| {
                let w = sample_phase(d, g, (i as f32 + 0.5) / samples as f32, (i as f32 * 0.618).fract());
                assert!((w.length() - 1.0).abs() < 1e-4);
                w.dot(d)
            }).sum::<f32>() / samples as f32;
            assert!((mean - g).abs() < 0.02, "g {g}: mean {mean}");
        }
    }

    #[test]
    fn height_fog_thins_upward_and_its_depth_is_exact() {
        let mut v = fog(unit_box(Vec3::new(0.0, 0.0, 0.0)), 2.0);
        v.falloff = Some((v.bottom(), 0.5));
        assert_eq!(v.bottom(), -1.0);
        assert!((v.density_at(Vec3::new(0.0, -1.0, 0.0)) - 2.0).abs() < 1e-6);
        assert!(v.density_at(Vec3::new(0.0, 0.5, 0.0)) < 0.2);
        // A slanted ray through the box: compare with a fine numerical integral.
        let o = Vec3::new(-3.0, -0.9, -0.2);
        let mut d = Vec3::new(1.0, 0.3, 0.1);
        d.normalize();
        let (a, b) = v.interval(o, d, 100.0).unwrap();
        let n = 100_000;
        let numeric: f32 = (0..n).map(|k| v.density_at(o + d * (a + (k as f32 + 0.5) * (b - a) / n as f32))).sum::<f32>()
            * (b - a) / n as f32;
        let exact = optical_depth(&[v], o, d, 100.0);
        assert!((exact - numeric).abs() < 1e-3 * numeric, "{exact} {numeric}");
        for target in [0.05, 0.3, 0.9 * exact] {
            let (t, _) = distance_at_depth(&[v], o, d, 100.0, target, 0.5).unwrap();
            assert!((optical_depth(&[v], o, d, t) - target).abs() < 2e-3, "target {target}");
        }
        assert_eq!(distance_at_depth(&[v], o, d, 100.0, exact * 1.1, 0.5), None);
    }

    #[test]
    fn noise_makes_puffs_and_marching_inverts_its_depth() {
        let noise = Noise { scale: 0.4, octaves: 4, coverage: 0.5, seed: 1 };
        let samples = 20_000;
        let mut filled = 0;
        for k in 0..samples {
            let p = Vec3::new((k as f32 * 0.618).fract() * 9.0, (k as f32 * 0.414).fract() * 9.0, (k as f32 * 0.732).fract() * 9.0);
            let f = noise.factor(p);
            assert!((0.0..=1.0).contains(&f));
            if f > 0.5 { filled += 1; }
        }
        let share = filled as f32 / samples as f32;
        assert!(share > 0.2 && share < 0.8, "{share}");
        let mut v = fog(unit_box(Vec3::new(0.0, 0.0, 5.0)), 3.0);
        v.noise = Some(noise);
        let (o, d) = (Vec3::new(0.1, 0.2, 0.0), Vec3::new(0.0, 0.0, 1.0));
        let total = optical_depth(&[v], o, d, 100.0);
        assert!(total > 0.0 && total < 6.0, "{total}");
        for target in [0.25 * total, 0.5 * total, 0.95 * total] {
            let (t, _) = distance_at_depth(&[v], o, d, 100.0, target, 0.5).unwrap();
            assert!((4.0..=6.0).contains(&t));
            assert!((optical_depth(&[v], o, d, t) - target).abs() < 0.02 * total + 1e-3, "target {target}");
        }
    }
}

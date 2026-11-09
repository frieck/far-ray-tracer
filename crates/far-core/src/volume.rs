//! Participating media: fog, smoke and haze that light scatters in, which turns beams of light
//! into visible shafts.
//!
//! Each volume is a homogeneous medium in a box or a sphere. `density` is the extinction
//! coefficient, the chance per scene unit that light hits a particle; `color` is the albedo,
//! the share of that light scattered rather than absorbed; `anisotropy` is the Henyey-Greenstein
//! g, from back scattering (negative) through even (0) to forward scattering (positive), as in
//! haze lit from behind. Where volumes overlap their densities add.

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
}

impl Volume {
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
    volumes.iter().filter_map(|v| v.interval(o, d, limit).map(|(a, b)| v.density * (b - a))).sum()
}

/// Where along the ray the optical depth reaches `target`, and in which volume the light
/// scatters there, picked by its share of the density with `pick` in [0, 1). None if the ray
/// leaves every volume before `limit` with less depth than that.
pub fn distance_at_depth(volumes: &[Volume], o: Vec3, d: Vec3, limit: f32, target: f32, pick: f32)
    -> Option<(f32, usize)> {
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
        Volume { shape, density, albedo: Color::new(1.0, 1.0, 1.0), anisotropy: 0.0 }
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
}

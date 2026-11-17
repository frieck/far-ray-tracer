//! Light portals, as V-Ray and Cycles have them: rectangles in a room's windows that tell the
//! renderer where the environment's light comes in. Indoors, most directions toward the sky
//! meet a wall, so drawing them by the environment's brightness alone wastes most shadow rays
//! and leaves the room noisy; a portal draws them through the window instead.
//!
//! A portal is not there for rays: it blocks nothing and is never seen. It only changes how
//! the environment is sampled, a mixture of a point on a portal and the environment's own
//! distribution, so light that comes in elsewhere, or the sun's disk, is still found. The same
//! mixture's density weighs reflections that reach the environment (MIS), so nothing is
//! counted twice.

use crate::environment::Environment;
use crate::vector::{Color, Vec3};

/// The share of environment samples drawn through portals, when a scene has any; the rest
/// follows the environment's brightness, which finds the sun.
const PORTAL_SHARE: f32 = 0.5;

/// A window's opening: centered on `center`, spanning `edge_u` and `edge_v`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Portal {
    pub center: Vec3,
    pub edge_u: Vec3,
    pub edge_v: Vec3,
    /// A unit normal; a portal works from either side.
    pub normal: Vec3,
    pub area: f32,
}

impl Portal {
    pub fn new(center: Vec3, edge_u: Vec3, edge_v: Vec3) -> Option<Self> {
        let mut normal = edge_u.cross(edge_v);
        let area = normal.length();
        if !(area > 0.0 && area.is_finite()) {
            return None;
        }
        normal *= 1.0 / area;
        Some(Self { center, edge_u, edge_v, normal, area })
    }

    /// A direction from `from` through a point of the portal drawn by `u` and `v`, and its
    /// density over directions; None when `from` lies in the portal's plane.
    fn sample(&self, from: Vec3, u: f32, v: f32) -> Option<(Vec3, f32)> {
        let target = self.center + self.edge_u * (u - 0.5) + self.edge_v * (v - 0.5);
        let delta = target - from;
        let distance = delta.length();
        if distance <= 0.0 {
            return None;
        }
        let direction = delta * (1.0 / distance);
        let cosine = direction.dot(self.normal).abs();
        (cosine > 1e-6).then(|| (direction, distance * distance / (self.area * cosine)))
    }

    /// The density `sample` gives the direction `d` from `from`: 0 when it misses the portal.
    fn pdf(&self, from: Vec3, d: Vec3) -> f32 {
        let facing = d.dot(self.normal);
        if facing.abs() < 1e-6 {
            return 0.0;
        }
        let t = (self.center - from).dot(self.normal) / facing;
        if t <= 0.0 {
            return 0.0;
        }
        let local = from + d * t - self.center;
        let (u, v) = (local.dot(self.edge_u) / self.edge_u.dot(self.edge_u), local.dot(self.edge_v) / self.edge_v.dot(self.edge_v));
        if u.abs() > 0.5 || v.abs() > 0.5 {
            return 0.0;
        }
        t * t / (self.area * facing.abs())
    }
}

/// A direction toward the environment from `from`, its radiance and the mixture's density:
/// through a portal half the time when the scene has portals, by the environment's own
/// brightness otherwise. `u`, `v` and `w` are in [0, 1).
pub fn sample(portals: &[Portal], environment: &Environment, from: Vec3, u: f32, v: f32, w: f32) -> (Vec3, Color, f32) {
    if portals.is_empty() {
        return environment.sample(u, v);
    }
    let direction = if w < PORTAL_SHARE {
        // The same w, stretched again, picks the portal.
        let pick = ((w / PORTAL_SHARE) * portals.len() as f32) as usize;
        match portals[pick.min(portals.len() - 1)].sample(from, u, v) {
            Some((direction, _)) => direction,
            None => return (Vec3::default(), Color::default(), 0.0),
        }
    } else {
        environment.sample(u, v).0
    };
    let pdf = pdf(portals, environment, from, direction);
    (direction, environment.radiance(direction), pdf)
}

/// The density `sample` gives the direction `d` from `from`.
pub fn pdf(portals: &[Portal], environment: &Environment, from: Vec3, d: Vec3) -> f32 {
    if portals.is_empty() {
        return environment.pdf(d);
    }
    let through: f32 = portals.iter().map(|p| p.pdf(from, d)).sum::<f32>() / portals.len() as f32;
    PORTAL_SHARE * through + (1.0 - PORTAL_SHARE) * environment.pdf(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_files_read_portals_as_lights_that_give_no_light() {
        let read = |portal: &str| crate::Scene::from_far(format!(r#"{{"lights": [
            {{"type": "rectangle", "position": [0, 3, 0], "edge_u": [1, 0, 0], "edge_v": [0, 0, 1]}},
            {{"type": "portal", "name": "Window", "position": [-3, 1.5, 0], "edge_u": {portal}, "edge_v": [0, 1.4, 0]}}],
            "objects": []}}"#).as_bytes(), std::path::Path::new("test.far"));
        let scene = read("[0, 0, 1.8]").unwrap();
        assert_eq!(scene.portals.len(), 1);
        assert!((scene.portals[0].area - 1.8 * 1.4).abs() < 1e-5);
        assert_eq!(scene.rect_lights.len(), 1, "a portal is not a light");
        let listed: Vec<(&str, &str, usize)> = scene.scene_lights.iter().map(|l| (l.name.as_str(), l.kind, l.index)).collect();
        assert_eq!(listed, [("Area light", "rectangle", 0), ("Window", "portal", 0)]);
        // Hidden, it guides nothing.
        assert!(scene.without(&[], &[1]).portals.is_empty());
        assert!(read("[0, 1.4, 0]").is_err(), "edges that span no area");
    }

    #[test]
    fn a_portal_draws_directions_through_it_with_the_density_it_gives_them() {
        let portal = Portal::new(Vec3::new(0.0, 2.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.5)).unwrap();
        let from = Vec3::new(0.3, 0.0, -0.4);
        for (u, v) in [(0.1, 0.2), (0.5, 0.5), (0.9, 0.7)] {
            let (d, density) = portal.sample(from, u, v).unwrap();
            assert!((portal.pdf(from, d) - density).abs() < 1e-3 * density, "{d:?}");
        }
        // Directions that miss it, or point away, have no density.
        assert_eq!(portal.pdf(from, Vec3::new(0.0, -1.0, 0.0)), 0.0);
        assert_eq!(portal.pdf(from, Vec3::new(1.0, 0.0, 0.0)), 0.0);
        // Over the whole sphere the density adds up to 1: the solid angle the portal fills,
        // estimated by uniform directions.
        let mut total = 0.0f64;
        let n = 400_000;
        let mut state = 12345u64;
        let mut random = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 40) as f32 / (1u64 << 24) as f32
        };
        for _ in 0..n {
            let z = 1.0 - 2.0 * random();
            let r = (1.0 - z * z).max(0.0).sqrt();
            let phi = 2.0 * std::f32::consts::PI * random();
            let d = Vec3::new(r * phi.cos(), z, r * phi.sin());
            total += portal.pdf(from, d) as f64 * 4.0 * std::f64::consts::PI / n as f64;
        }
        assert!((total - 1.0).abs() < 0.03, "{total}");
    }
}

//! Thin-lens camera. By default it sits at (0, 0, -5) and looks along +Z, as in 2008.
use crate::engine::Ray;
use crate::vector::{Vec3, PI};
use serde::{Deserialize, Serialize};

/// Distance from the eye to the image plane, which sets the base field of view.
const IMAGE_DISTANCE: f32 = 5.0;

/// Where the camera is and what it looks at. Enhanced mode only; 2008 and Fixed keep
/// the default, so that they stay comparable with the original.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pose {
    pub position: [f32; 3],
    pub target: [f32; 3],
    /// Lens magnification; 1 is the original field of view.
    pub zoom: f32,
}

impl Default for Pose {
    fn default() -> Self { Self { position: [0.0, 0.0, -5.0], target: [0.0, 0.0, 0.0], zoom: 1.0 } }
}

impl Pose {
    pub fn validate(&self) -> Result<(), String> {
        let finite = self.position.iter().chain(&self.target).all(|v| v.is_finite());
        if !finite || self.position == self.target {
            return Err("Camera position and target must be finite and different".into());
        }
        if !self.zoom.is_finite() || self.zoom <= 0.0 {
            return Err("Camera zoom must be finite and positive".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Camera {
    /// Lens diameter in scene units; zero preserves the pinhole camera.
    pub aperture: f32,
    /// Distance along the viewing axis, measured from the camera (not world Z).
    pub focus_distance: f32,
    /// Blades of the lens diaphragm, which give out-of-focus highlights their shape: 0 is a
    /// round opening, 3 or more a polygon with its corners on the round one.
    pub blades: u32,
    /// Turns the polygon, in degrees.
    pub blade_rotation: f32,
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        let pose = Pose::default();
        Self { aperture: 0.0, focus_distance: 10.0, blades: 0, blade_rotation: 0.0, position: pose.position, target: pose.target, zoom: pose.zoom }
    }
}

impl Camera {
    pub fn pose(&self) -> Pose {
        Pose { position: self.position, target: self.target, zoom: self.zoom }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.aperture.is_finite() || self.aperture < 0.0 {
            return Err("Camera aperture must be finite and nonnegative".into());
        }
        if !self.focus_distance.is_finite() || self.focus_distance <= 0.0 {
            return Err("Camera focus_distance must be finite and positive".into());
        }
        if self.blades != 0 && !(3..=16).contains(&self.blades) {
            return Err("Camera blades must be 0 (round) or from 3 to 16".into());
        }
        if !self.blade_rotation.is_finite() {
            return Err("Camera blade_rotation must be finite".into());
        }
        self.pose().validate()
    }

    pub(crate) fn ray(&self, view: &View, direction: Vec3, u: f32, v: f32) -> Ray {
        if self.aperture == 0.0 { return Ray { o: view.origin, d: direction }; }
        let focus = view.origin + direction * (self.focus_distance / direction.dot(view.forward));
        let (x, y) = self.lens_point(u, v);
        let o = view.origin + view.right * x + view.up * y;
        let mut d = focus - o;
        d.normalize();
        Ray { o, d }
    }
}

impl Camera {
    /// A point of the lens opening, spread evenly over it for (u, v) evenly spread in [0, 1)².
    pub(crate) fn lens_point(&self, u: f32, v: f32) -> (f32, f32) {
        let radius = 0.5 * self.aperture;
        if self.blades < 3 {
            let (r, angle) = (radius * u.sqrt(), 2.0 * PI * v);
            return (r * angle.cos(), r * angle.sin());
        }
        // One of the polygon's triangles around the center, chosen by u, then a uniform point in
        // it. Rotation 0 puts a flat edge at the bottom, like most lenses.
        let n = self.blades as f32;
        let side = ((u * n) as u32).min(self.blades - 1);
        let t = u * n - side as f32;
        let step = 2.0 * PI / n;
        let start = self.blade_rotation.to_radians() - 0.5 * PI - 0.5 * step + side as f32 * step;
        let corner = |angle: f32| (radius * angle.cos(), radius * angle.sin());
        let (a, b) = (corner(start), corner(start + step));
        let s = t.sqrt();
        (s * ((1.0 - v) * a.0 + v * b.0), s * ((1.0 - v) * a.1 + v * b.1))
    }
}

/// The image plane of a pose at a given image size.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub origin: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub forward: Vec3,
    pub half_width: f32,
    pub half_height: f32,
}

impl View {
    pub fn new(pose: &Pose, width: u32, height: u32) -> Self {
        let [x, y, z] = pose.position;
        let origin = Vec3::new(x, y, z);
        let [x, y, z] = pose.target;
        let mut forward = Vec3::new(x, y, z) - origin;
        forward.normalize();
        // Straight up or down, world Y gives no sideways direction.
        let world_up = if forward.y.abs() > 0.9999 { Vec3::new(0.0, 0.0, 1.0) } else { Vec3::new(0.0, 1.0, 0.0) };
        let mut right = world_up.cross(forward);
        right.normalize();
        let up = forward.cross(right);
        let (half_width, half_height) = crate::engine::view_half_size(width, height);
        Self { origin, right, up, forward, half_width: half_width / pose.zoom, half_height: half_height / pose.zoom }
    }

    /// Point of the image plane at normalized coordinates, with (-1, 1) at the top left.
    pub fn plane_point(&self, x: f32, y: f32) -> Vec3 {
        self.origin + self.forward * IMAGE_DISTANCE
            + self.right * (x * self.half_width) + self.up * (y * self.half_height)
    }

    /// Image coordinates in 0..1 of a world point, or None if it is behind the camera.
    pub fn project(&self, point: Vec3) -> Option<(f32, f32)> {
        let relative = point - self.origin;
        let depth = relative.dot(self.forward);
        if depth <= 0.0 { return None; }
        let scale = IMAGE_DISTANCE / depth;
        Some(((relative.dot(self.right) * scale / self.half_width + 1.0) * 0.5,
              (1.0 - relative.dot(self.up) * scale / self.half_height) * 0.5))
    }
}

/// Progressive, evenly distributed lens samples with an independent rotation per
/// pixel. f64 keeps distant passes precise; no dependence on thread scheduling.
pub(crate) fn lens_sample(pass: u32, pixel: u32) -> (f32, f32) {
    fn hash(mut x: u32) -> u32 {
        x ^= x >> 16;
        x = x.wrapping_mul(0x7feb352d);
        x ^= x >> 15;
        x = x.wrapping_mul(0x846ca68b);
        x ^ (x >> 16)
    }
    let u = hash(pixel) as f64 / 4294967296.0;
    let v = hash(pixel ^ 0xa511e9b3) as f64 / 4294967296.0;
    let index = pass as f64 + 0.5;
    (((u + index * 0.7548776662466927).fract() as f32).min(f32::from_bits(0x3f7fffff)),
     ((v + index * 0.5698402909980532).fract() as f32).min(f32::from_bits(0x3f7fffff)))
}

/// Focus on the first surface under normalized image coordinates. Use the central
/// pinhole ray even with an open lens, so picking is stable on a blurred image.
/// Return axial distance, not ray length (which would misfocus off-center picks).
pub fn pick_focus(scene: &crate::Scene, kd: Option<&crate::KdTree>, pose: &Pose, width: u32, height: u32, x: f32, y: f32) -> Option<f32> {
    if width == 0 || height == 0 || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) { return None; }
    let view = View::new(pose, width, height);
    let mut direction = view.plane_point(2.0 * x - 1.0, 1.0 - 2.0 * y) - view.origin;
    direction.normalize();
    let ray = Ray { o: view.origin, d: direction };
    let mut context = crate::engine::Context::with_fixes(scene, kd, 1);
    let mut distance = 10000.0;
    let mut found = false;
    if let Some(kd) = kd {
        found = context.closest(kd, &ray, &mut distance).0 != 0;
    } else {
        for i in 0..scene.prims.len() {
            found |= context.intersect(i, &ray, &mut distance) != 0;
        }
    }
    for light in &scene.rect_lights {
        if let Some(hit) = light.intersect(view.origin, direction, distance) {
            distance = hit;
            found = true;
        }
    }
    for light in &scene.bulbs {
        if let Some(hit) = light.intersect(view.origin, direction, distance) {
            distance = hit;
            found = true;
        }
    }
    found.then_some(distance * direction.dot(view.forward))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progressive_lens_samples_cover_the_aperture_without_clustering() {
        for pixel in [0, 123, 54321] {
            let mut cells = [0; 16];
            for pass in 0..256 {
                let (u, v) = lens_sample(pass, pixel);
                assert!((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v));
                cells[(v * 4.0) as usize * 4 + (u * 4.0) as usize] += 1;
            }
            assert!(cells.iter().all(|n| (10..=24).contains(n)), "{cells:?}");
        }
        assert_ne!(lens_sample(0, 0), lens_sample(0, 1));
    }

    #[test]
    fn picking_uses_axial_depth_and_returns_none_for_background() {
        let scene = crate::Scene::from_far(br#"{"lights":[],"objects":[{"type":"sphere","center":[0,0,5],"radius":1}]}"#, std::path::Path::new("pick.far")).unwrap();
        let kd = crate::KdTree::build(&scene);
        for tree in [None, Some(&kd)] {
            assert!((pick_focus(&scene, tree, &Pose::default(), 800, 600, 0.5, 0.5).unwrap() - 9.0).abs() < 1e-5);
            assert_eq!(pick_focus(&scene, tree, &Pose::default(), 800, 600, 0.0, 0.0), None);
        }
        let plane = crate::Scene::from_far(br#"{"lights":[],"objects":[{"type":"plane","points":[[-30,-30,7],[30,-30,7],[30,30,7],[-30,30,7]]}]}"#, std::path::Path::new("pick.far")).unwrap();
        for (width, height) in [(800,600), (1600,900), (600,800)] {
            let distance = pick_focus(&plane, None, &Pose::default(), width, height, 0.85, 0.2).unwrap();
            assert!((distance - 12.0).abs() < 1e-4, "off-axis pick must use plane depth");
        }
        assert_eq!(pick_focus(&plane, None, &Pose::default(), 800, 600, f32::NAN, 0.5), None);
        let panel = crate::Scene::from_far(br#"{"lights":[{"type":"rectangle","position":[0,0,2],"edge_u":[2,0,0],"edge_v":[0,2,0]}],"objects":[]}"#, std::path::Path::new("pick.far")).unwrap();
        assert_eq!(pick_focus(&panel, None, &Pose::default(), 800, 600, 0.5, 0.5), Some(7.0));
    }

    #[test]
    fn lens_samples_meet_at_the_focus_plane() {
        let camera = Camera { aperture: 1.0, focus_distance: 12.0, ..Camera::default() };
        let view = View::new(&camera.pose(), 800, 600);
        let mut direction = Vec3::new(0.3, -0.2, 1.0);
        direction.normalize();
        let expected = Vec3::new(0.0, 0.0, -5.0) + direction * (12.0 / direction.z);
        for (u, v) in [(0.0, 0.0), (0.25, 0.4), (0.99, 0.9)] {
            let ray = camera.ray(&view, direction, u, v);
            let at_focus = ray.o + ray.d * (12.0 / ray.d.z);
            assert!((at_focus - expected).length() < 1e-5);
            assert!(ray.o.x * ray.o.x + ray.o.y * ray.o.y <= 0.25);
        }
    }
    #[test]
    fn default_view_is_the_original_image_plane() {
        let view = View::new(&Pose::default(), 800, 600);
        assert_eq!(view.origin, Vec3::new(0.0, 0.0, -5.0));
        assert_eq!((view.right, view.up, view.forward),
            (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)));
        assert_eq!(view.plane_point(-1.0, 1.0), Vec3::new(-4.0, 3.0, 0.0));
    }

    #[test]
    fn views_have_an_orthonormal_basis_and_project_their_target_to_the_center() {
        for position in [[3.0, 2.0, -7.0], [0.0, 9.0, 0.0], [0.0, -9.0, 0.0], [-4.0, 0.5, 12.0]] {
            let pose = Pose { position, target: [0.0, 0.0, 0.0], zoom: 2.0 };
            let view = View::new(&pose, 1600, 900);
            for v in [view.right, view.up, view.forward] { assert!((v.length() - 1.0).abs() < 1e-5); }
            assert!(view.right.dot(view.up).abs() < 1e-5 && view.right.dot(view.forward).abs() < 1e-5);
            let (x, y) = view.project(Vec3::default()).unwrap();
            assert!((x - 0.5).abs() < 1e-5 && (y - 0.5).abs() < 1e-5);
            let corner = view.plane_point(1.0, 1.0);
            let (x, y) = view.project(corner).unwrap();
            assert!((x - 1.0).abs() < 1e-5 && y.abs() < 1e-5, "top right stays top right");
        }
        let view = View::new(&Pose::default(), 800, 600);
        assert!(view.project(Vec3::new(0.0, 0.0, -6.0)).is_none());
    }

    #[test]
    fn zoom_narrows_the_view_and_bad_poses_are_rejected() {
        let wide = View::new(&Pose::default(), 800, 600);
        let close = View::new(&Pose { zoom: 2.0, ..Pose::default() }, 800, 600);
        assert_eq!(close.half_width * 2.0, wide.half_width);
        assert!(Pose { position: [0.0; 3], target: [0.0; 3], zoom: 1.0 }.validate().is_err());
        assert!(Pose { zoom: 0.0, ..Pose::default() }.validate().is_err());
        assert!(Pose { position: [f32::NAN, 0.0, 0.0], ..Pose::default() }.validate().is_err());
    }

    #[test]
    fn picking_follows_the_camera() {
        let scene = crate::Scene::from_far(br#"{"lights":[],"objects":[{"type":"sphere","center":[0,0,5],"radius":1}]}"#, std::path::Path::new("pick.far")).unwrap();
        let pose = Pose { position: [10.0, 0.0, 5.0], target: [0.0, 0.0, 5.0], zoom: 1.0 };
        assert!((pick_focus(&scene, None, &pose, 800, 600, 0.5, 0.5).unwrap() - 9.0).abs() < 1e-4);
        assert_eq!(pick_focus(&scene, None, &Pose { target: [20.0, 0.0, 5.0], ..pose }, 800, 600, 0.5, 0.5), None);
    }

    #[test]
    fn polygon_lens_points_stay_inside_and_fill_the_blades() {
        for blades in [3u32, 5, 6, 9] {
            let camera = Camera { aperture: 2.0, blades, blade_rotation: 12.0, ..Camera::default() };
            let n = blades as f32;
            // Distance from the center to an edge of the polygon at this angle.
            let edge = |angle: f32| {
                let start = 12f32.to_radians() - 0.5 * PI - 0.5 * 2.0 * PI / n;
                let local = (angle - start).rem_euclid(2.0 * PI / n) - PI / n;
                (PI / n).cos() / local.cos()
            };
            let mut sectors = vec![0u32; blades as usize];
            for pass in 0..4000 {
                let (u, v) = lens_sample(pass, 7);
                let (x, y) = camera.lens_point(u, v);
                let (r, angle) = ((x * x + y * y).sqrt(), y.atan2(x));
                assert!(r <= edge(angle) + 1e-4, "{blades} blades: {x} {y}");
                sectors[(((angle.rem_euclid(2.0 * PI)) / (2.0 * PI) * n) as usize).min(blades as usize - 1)] += 1;
            }
            assert!(sectors.iter().all(|&c| c > 4000 / blades / 2), "{blades}: {sectors:?}");
        }
        let round = Camera { aperture: 2.0, ..Camera::default() };
        let (x, y) = round.lens_point(0.999, 0.3);
        assert!(((x * x + y * y).sqrt() - 1.0).abs() < 1e-3);
        assert!(Camera { blades: 2, ..Camera::default() }.validate().is_err());
        assert!(Camera { blades: 6, ..Camera::default() }.validate().is_ok());
    }

    #[test]
    fn zero_aperture_keeps_the_exact_pinhole_ray() {
        let direction = Vec3::new(0.0, 0.0, 1.0);
        let camera = Camera::default();
        let ray = camera.ray(&View::new(&camera.pose(), 800, 600), direction, 0.8, 0.2);
        assert_eq!(ray.o, Vec3::new(0.0, 0.0, -5.0));
        assert_eq!(ray.d, direction);
        assert!(Camera { aperture: -1.0, ..Camera::default() }.validate().is_err());
        assert!(Camera { focus_distance: 0.0, ..Camera::default() }.validate().is_err());
    }
}

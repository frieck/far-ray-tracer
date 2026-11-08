//! Analytic solids: box, cylinder, tube, cone and torus.
//!
//! Each shape is defined around the origin with Y as its axis and is intersected
//! exactly, so curves stay smooth at any resolution. A ray is moved into that local
//! frame; rotation keeps lengths, so distances along the ray stay the same.

use crate::vector::{Vec3, PI};

const MIN_DISTANCE: f32 = 1e-4;

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Box { half: Vec3 },
    Cylinder { radius: f32, half_height: f32 },
    Tube { radius: f32, inner_radius: f32, half_height: f32 },
    Cone { radius: f32, half_height: f32 },
    Torus { major_radius: f32, minor_radius: f32 },
}

#[derive(Clone)]
pub struct Solid {
    pub shape: Shape,
    pub center: Vec3,
    /// World directions of the local X, Y and Z axes.
    axes: [Vec3; 3],
    pub material: usize,
    /// World-space bounding box corners.
    pub low: Vec3,
    pub high: Vec3,
}

fn rotate(v: Vec3, degrees: [f32; 3]) -> Vec3 {
    let [x, y, z] = degrees.map(|d| d * PI / 180.0);
    let v = Vec3::new(v.x, v.y * x.cos() - v.z * x.sin(), v.y * x.sin() + v.z * x.cos());
    let v = Vec3::new(v.x * y.cos() + v.z * y.sin(), v.y, -v.x * y.sin() + v.z * y.cos());
    Vec3::new(v.x * z.cos() - v.y * z.sin(), v.x * z.sin() + v.y * z.cos(), v.z)
}

/// Keeps the smallest distance that is ahead of the ray and closer than `best`.
fn consider(t: f32, best: &mut f32) {
    if t > MIN_DISTANCE && t < *best {
        *best = t;
    }
}

/// Real roots of `a t^2 + b t + c`, using the form that avoids cancellation.
fn quadratic(a: f32, b: f32, c: f32) -> Option<(f32, f32)> {
    if a.abs() < 1e-9 {
        return (b.abs() > 1e-9).then(|| (-c / b, -c / b));
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let q = -0.5 * (b + disc.sqrt().copysign(b));
    let (t0, t1) = (q / a, if q != 0.0 { c / q } else { 0.0 });
    Some((t0.min(t1), t0.max(t1)))
}

/// Walls of an infinite cylinder, kept only between `-half_height` and `half_height`.
fn cylinder_wall(o: Vec3, d: Vec3, radius: f32, half_height: f32, best: &mut f32) {
    let a = d.x * d.x + d.z * d.z;
    let b = 2.0 * (o.x * d.x + o.z * d.z);
    let c = o.x * o.x + o.z * o.z - radius * radius;
    if let Some((t0, t1)) = quadratic(a, b, c) {
        for t in [t0, t1] {
            if (o.y + t * d.y).abs() <= half_height {
                consider(t, best);
            }
        }
    }
}

/// Flat ring at height `y`, between two radii.
fn ring(o: Vec3, d: Vec3, y: f32, inner: f32, outer: f32, best: &mut f32) {
    if d.y == 0.0 {
        return;
    }
    let t = (y - o.y) / d.y;
    let (x, z) = (o.x + t * d.x, o.z + t * d.z);
    let r2 = x * x + z * z;
    if r2 <= outer * outer && r2 >= inner * inner {
        consider(t, best);
    }
}

fn box_hit(o: Vec3, d: Vec3, half: Vec3, best: &mut f32) {
    let (mut near, mut far) = (f32::NEG_INFINITY, f32::INFINITY);
    for i in 0..3 {
        let inv = 1.0 / d.cell(i);
        let (a, b) = ((-half.cell(i) - o.cell(i)) * inv, (half.cell(i) - o.cell(i)) * inv);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    if near <= far {
        consider(near, best);
        consider(far, best);
    }
}

fn cone_hit(o: Vec3, d: Vec3, radius: f32, half_height: f32, best: &mut f32) {
    // The apex is at +half_height: x^2 + z^2 = (k (half_height - y))^2.
    let k = radius / (2.0 * half_height);
    let k2 = k * k;
    let (oy, dy) = (half_height - o.y, -d.y);
    let a = d.x * d.x + d.z * d.z - k2 * dy * dy;
    let b = 2.0 * (o.x * d.x + o.z * d.z - k2 * oy * dy);
    let c = o.x * o.x + o.z * o.z - k2 * oy * oy;
    if let Some((t0, t1)) = quadratic(a, b, c) {
        for t in [t0, t1] {
            if (o.y + t * d.y).abs() <= half_height {
                consider(t, best);
            }
        }
    }
    ring(o, d, -half_height, 0.0, radius, best);
}

/// Largest real root of `m^3 + a m^2 + b m + c`.
fn largest_cubic_root(a: f64, b: f64, c: f64) -> f64 {
    let p = b - a * a / 3.0;
    let q = 2.0 * a * a * a / 27.0 - a * b / 3.0 + c;
    let shift = -a / 3.0;
    let disc = q * q / 4.0 + p * p * p / 27.0;
    if disc >= 0.0 {
        let s = disc.sqrt();
        (-q / 2.0 + s).cbrt() + (-q / 2.0 - s).cbrt() + shift
    } else {
        let r = (-p / 3.0).sqrt();
        let angle = (-q / (2.0 * r * r * r)).clamp(-1.0, 1.0).acos();
        2.0 * r * (angle / 3.0).cos() + shift
    }
}

/// Real roots of `t^4 + p t^2 + q t + s`, by Ferrari's method. They are polished with
/// Newton steps, because the closed form loses precision near double roots.
fn quartic_roots(p: f64, q: f64, s: f64) -> ([f64; 4], usize) {
    let (mut roots, mut count) = ([0.0; 4], 0);
    let mut push = |t: f64| {
        roots[count] = t;
        count += 1;
    };
    let solve = |b: f64, c: f64, push: &mut dyn FnMut(f64)| {
        let disc = b * b - 4.0 * c;
        if disc >= 0.0 {
            let root = disc.sqrt();
            push((-b - root) / 2.0);
            push((-b + root) / 2.0);
        }
    };
    if q.abs() < 1e-12 {
        let disc = p * p - 4.0 * s;
        if disc >= 0.0 {
            for u in [(-p - disc.sqrt()) / 2.0, (-p + disc.sqrt()) / 2.0] {
                if u >= 0.0 {
                    push(u.sqrt());
                    push(-u.sqrt());
                }
            }
        }
    } else {
        let m = largest_cubic_root(p, p * p / 4.0 - s, -q * q / 8.0).max(1e-12);
        let w = (2.0 * m).sqrt();
        solve(w, p / 2.0 + m - q / (2.0 * w), &mut push);
        solve(-w, p / 2.0 + m + q / (2.0 * w), &mut push);
    }
    for t in roots.iter_mut().take(count) {
        for _ in 0..3 {
            let value = ((*t * *t + p) * *t + q) * *t + s;
            let slope = (4.0 * *t * *t + 2.0 * p) * *t + q;
            if slope.abs() > 1e-12 {
                *t -= value / slope;
            }
        }
    }
    (roots, count)
}

fn torus_hit(o: Vec3, d: Vec3, major: f32, minor: f32, best: &mut f32) {
    // Starting from the point of the ray closest to the origin removes the cubic term.
    let shift = -o.dot(d);
    let o = o + d * shift;
    let bound = major + minor;
    if o.dot(o) > bound * bound {
        return;
    }
    let (r2, m2) = ((major * major) as f64, (minor * minor) as f64);
    let (ox, oy, oz) = (o.x as f64, o.y as f64, o.z as f64);
    let (dx, dz) = (d.x as f64, d.z as f64);
    let b = ox * ox + oy * oy + oz * oz + r2 - m2;
    let p = 2.0 * b - 4.0 * r2 * (dx * dx + dz * dz);
    let q = -8.0 * r2 * (ox * dx + oz * dz);
    let s = b * b - 4.0 * r2 * (ox * ox + oz * oz);
    let (roots, count) = quartic_roots(p, q, s);
    for t in &roots[..count] {
        consider(*t as f32 + shift, best);
    }
}

impl Solid {
    pub fn axes(&self) -> [Vec3; 3] {
        self.axes
    }

    pub fn new(shape: Shape, center: Vec3, rotation: [f32; 3], material: usize) -> Self {
        let axes = [
            rotate(Vec3::new(1.0, 0.0, 0.0), rotation),
            rotate(Vec3::new(0.0, 1.0, 0.0), rotation),
            rotate(Vec3::new(0.0, 0.0, 1.0), rotation),
        ];
        let half = match shape {
            Shape::Box { half } => half,
            Shape::Cylinder { radius, half_height }
            | Shape::Tube { radius, half_height, .. }
            | Shape::Cone { radius, half_height } => Vec3::new(radius, half_height, radius),
            Shape::Torus { major_radius, minor_radius } => {
                let outer = major_radius + minor_radius;
                Vec3::new(outer, minor_radius, outer)
            }
        };
        let reach = |i: usize| {
            axes[0].cell(i).abs() * half.x + axes[1].cell(i).abs() * half.y + axes[2].cell(i).abs() * half.z
        };
        let extent = Vec3::new(reach(0), reach(1), reach(2));
        Self { shape, center, axes, material, low: center - extent, high: center + extent }
    }

    fn local_direction(&self, v: Vec3) -> Vec3 {
        Vec3::new(v.dot(self.axes[0]), v.dot(self.axes[1]), v.dot(self.axes[2]))
    }

    /// World position of a point given in the solid's own frame.
    pub fn to_world(&self, local: Vec3) -> Vec3 {
        self.center + self.direction_to_world(local)
    }

    /// World direction of a direction given in the solid's own frame.
    pub fn direction_to_world(&self, local: Vec3) -> Vec3 {
        self.axes[0] * local.x + self.axes[1] * local.y + self.axes[2] * local.z
    }

    fn local_point(&self, p: Vec3) -> Vec3 {
        self.local_direction(p - self.center)
    }

    /// Closest hit before `dist`: 1 from outside, -1 from inside, 0 for a miss.
    #[inline]
    pub fn intersect(&self, o: Vec3, d: Vec3, dist: &mut f32) -> i32 {
        let (lo, ld) = (self.local_point(o), self.local_direction(d));
        let mut best = *dist;
        match self.shape {
            Shape::Box { half } => box_hit(lo, ld, half, &mut best),
            Shape::Cylinder { radius, half_height } => {
                cylinder_wall(lo, ld, radius, half_height, &mut best);
                ring(lo, ld, half_height, 0.0, radius, &mut best);
                ring(lo, ld, -half_height, 0.0, radius, &mut best);
            }
            Shape::Tube { radius, inner_radius, half_height } => {
                cylinder_wall(lo, ld, radius, half_height, &mut best);
                cylinder_wall(lo, ld, inner_radius, half_height, &mut best);
                ring(lo, ld, half_height, inner_radius, radius, &mut best);
                ring(lo, ld, -half_height, inner_radius, radius, &mut best);
            }
            Shape::Cone { radius, half_height } => cone_hit(lo, ld, radius, half_height, &mut best),
            Shape::Torus { major_radius, minor_radius } => {
                torus_hit(lo, ld, major_radius, minor_radius, &mut best)
            }
        }
        if best >= *dist {
            return 0;
        }
        *dist = best;
        if d.dot(self.normal(o + d * best)) > 0.0 { -1 } else { 1 }
    }

    /// Outward unit normal at a point on the surface.
    pub fn normal(&self, pos: Vec3) -> Vec3 {
        let p = self.local_point(pos);
        let radial = {
            let r = (p.x * p.x + p.z * p.z).sqrt().max(1e-9);
            Vec3::new(p.x / r, 0.0, p.z / r)
        };
        let distance = (p.x * p.x + p.z * p.z).sqrt();
        let cap = Vec3::new(0.0, 1.0f32.copysign(p.y), 0.0);
        let local = match self.shape {
            Shape::Box { half } => {
                let ratio = [p.x / half.x, p.y / half.y, p.z / half.z];
                let axis = (0..3).fold(0, |best, i| if ratio[i].abs() > ratio[best].abs() { i } else { best });
                let mut n = Vec3::default();
                n.set_cell(axis, 1.0f32.copysign(ratio[axis]));
                n
            }
            Shape::Cylinder { radius, half_height } => {
                if (p.y.abs() - half_height).abs() < (distance - radius).abs() { cap } else { radial }
            }
            Shape::Tube { radius, inner_radius, half_height } => {
                let to_cap = (p.y.abs() - half_height).abs();
                let (to_outer, to_inner) = ((distance - radius).abs(), (distance - inner_radius).abs());
                if to_cap < to_outer && to_cap < to_inner {
                    cap
                } else if to_outer < to_inner {
                    radial
                } else {
                    -radial
                }
            }
            Shape::Cone { radius, half_height } => {
                let height = 2.0 * half_height;
                let side_radius = radius * (half_height - p.y) / height;
                if (p.y + half_height).abs() < (distance - side_radius).abs() {
                    Vec3::new(0.0, -1.0, 0.0)
                } else {
                    let mut n = radial * height + Vec3::new(0.0, radius, 0.0);
                    n.normalize();
                    n
                }
            }
            Shape::Torus { major_radius, .. } => {
                let mut n = p - radial * major_radius;
                n.normalize();
                n
            }
        };
        self.axes[0] * local.x + self.axes[1] * local.y + self.axes[2] * local.z
    }

    /// Texture coordinates at a point on the surface.
    pub fn uv(&self, pos: Vec3) -> (f32, f32) {
        let p = self.local_point(pos);
        let around = p.z.atan2(p.x) / (2.0 * PI) + 0.5;
        match self.shape {
            Shape::Box { half } => {
                let (ax, ay, az) = ((p.x / half.x).abs(), (p.y / half.y).abs(), (p.z / half.z).abs());
                let (u, v) = if ax >= ay && ax >= az {
                    (p.z / half.z, p.y / half.y)
                } else if ay >= az {
                    (p.x / half.x, p.z / half.z)
                } else {
                    (p.x / half.x, p.y / half.y)
                };
                (0.5 * u + 0.5, 0.5 * v + 0.5)
            }
            Shape::Cylinder { half_height, .. }
            | Shape::Tube { half_height, .. }
            | Shape::Cone { half_height, .. } => (around, 0.5 * p.y / half_height + 0.5),
            Shape::Torus { major_radius, .. } => {
                let ring_distance = (p.x * p.x + p.z * p.z).sqrt() - major_radius;
                (around, p.y.atan2(ring_distance) / (2.0 * PI) + 0.5)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::twister::Twister;

    /// Signed distance of each shape, written independently of the intersection code.
    fn distance(shape: Shape, p: Vec3) -> f32 {
        let around = (p.x * p.x + p.z * p.z).sqrt();
        match shape {
            Shape::Box { half } => {
                let q = Vec3::new(p.x.abs() - half.x, p.y.abs() - half.y, p.z.abs() - half.z);
                let outside = Vec3::new(q.x.max(0.0), q.y.max(0.0), q.z.max(0.0)).length();
                outside + q.x.max(q.y).max(q.z).min(0.0)
            }
            Shape::Cylinder { radius, half_height } => {
                let (a, b) = (around - radius, p.y.abs() - half_height);
                (a.max(0.0).powi(2) + b.max(0.0).powi(2)).sqrt() + a.max(b).min(0.0)
            }
            Shape::Tube { radius, inner_radius, half_height } => {
                let mid = 0.5 * (radius + inner_radius);
                let (a, b) = ((around - mid).abs() - 0.5 * (radius - inner_radius), p.y.abs() - half_height);
                (a.max(0.0).powi(2) + b.max(0.0).powi(2)).sqrt() + a.max(b).min(0.0)
            }
            Shape::Cone { radius, half_height } => {
                let slope = (2.0 * half_height).hypot(radius);
                let side = (around * 2.0 * half_height + (p.y - half_height) * radius) / slope;
                side.max(-p.y - half_height)
            }
            Shape::Torus { major_radius, minor_radius } => {
                (around - major_radius).hypot(p.y) - minor_radius
            }
        }
    }

    fn shapes() -> [Shape; 5] {
        [
            Shape::Box { half: Vec3::new(1.0, 0.7, 1.3) },
            Shape::Cylinder { radius: 1.0, half_height: 1.2 },
            Shape::Tube { radius: 1.2, inner_radius: 0.7, half_height: 1.0 },
            Shape::Cone { radius: 1.1, half_height: 1.3 },
            Shape::Torus { major_radius: 1.2, minor_radius: 0.4 },
        ]
    }

    fn random_unit(rng: &mut Twister) -> Vec3 {
        loop {
            let v = Vec3::new(rng.rand() - 0.5, rng.rand() - 0.5, rng.rand() - 0.5);
            if v.dot(v) > 0.01 && v.dot(v) < 0.25 {
                let mut v = v;
                v.normalize();
                return v;
            }
        }
    }

    /// Walks along the ray in small steps and returns where the sign first changes.
    fn marched_hit(shape: Shape, o: Vec3, d: Vec3) -> Option<f32> {
        let (mut t, step) = (0.0f32, 0.004f32);
        let start_outside = distance(shape, o) > 0.0;
        while t < 14.0 {
            t += step;
            if (distance(shape, o + d * t) > 0.0) != start_outside {
                return Some(t);
            }
        }
        None
    }

    #[test]
    fn hits_agree_with_an_independent_distance_function() {
        let mut rng = Twister::new(11);
        for shape in shapes() {
            let solid = Solid::new(shape, Vec3::default(), [0.0; 3], 0);
            let (mut hits, mut misses) = (0, 0);
            for _ in 0..4000 {
                let o = random_unit(&mut rng) * 6.0;
                let mut d = random_unit(&mut rng) * 1.5 - o;
                d.normalize();
                let mut dist = 1000.0;
                let result = solid.intersect(o, d, &mut dist);
                let expected = marched_hit(shape, o, d);
                match (result != 0, expected) {
                    (true, Some(t)) => {
                        hits += 1;
                        assert!((t - dist).abs() < 0.02, "{shape:?}: {dist} and marched {t}");
                        assert!(distance(shape, o + d * dist).abs() < 2e-3, "{shape:?} off the surface");
                    }
                    (false, None) => misses += 1,
                    // A ray that only grazes the surface can fall between the steps.
                    (true, None) => assert!(distance(shape, o + d * dist).abs() < 2e-3),
                    (false, Some(t)) => panic!("{shape:?} missed a hit at {t}"),
                }
            }
            assert!(hits > 500 && misses > 100, "{shape:?}: {hits} hits, {misses} misses");
        }
    }

    #[test]
    fn normals_are_unit_length_and_point_outward() {
        let mut rng = Twister::new(12);
        for shape in shapes() {
            let solid = Solid::new(shape, Vec3::default(), [0.0; 3], 0);
            for _ in 0..3000 {
                let o = random_unit(&mut rng) * 6.0;
                let mut d = random_unit(&mut rng) - o;
                d.normalize();
                let mut dist = 1000.0;
                if solid.intersect(o, d, &mut dist) == 0 {
                    continue;
                }
                let p = o + d * dist;
                let n = solid.normal(p);
                assert!((n.length() - 1.0).abs() < 1e-3, "{shape:?}");
                let moved = distance(shape, p + n * 0.01);
                assert!(moved > 0.0, "{shape:?}: normal points inward at {p:?}");
            }
        }
    }

    #[test]
    fn a_ray_through_the_hole_misses_the_torus_and_the_tube() {
        let d = Vec3::new(0.0, 1.0, 0.0);
        for shape in [shapes()[2], shapes()[4]] {
            let solid = Solid::new(shape, Vec3::default(), [0.0; 3], 0);
            let mut dist = 1000.0;
            assert_eq!(solid.intersect(Vec3::new(0.0, -5.0, 0.0), d, &mut dist), 0);
        }
    }

    #[test]
    fn a_ray_from_inside_reports_it() {
        let solid = Solid::new(shapes()[1], Vec3::default(), [0.0; 3], 0);
        let mut dist = 1000.0;
        assert_eq!(solid.intersect(Vec3::default(), Vec3::new(1.0, 0.0, 0.0), &mut dist), -1);
        assert!((dist - 1.0).abs() < 1e-5);
    }

    #[test]
    fn rotation_and_position_move_the_shape() {
        // A cylinder lying along X: its axis no longer points up.
        let shape = Shape::Cylinder { radius: 1.0, half_height: 3.0 };
        let solid = Solid::new(shape, Vec3::new(0.0, 0.0, 10.0), [0.0, 0.0, 90.0], 0);
        let mut dist = 1000.0;
        let o = Vec3::new(2.5, 0.0, 0.0);
        assert_eq!(solid.intersect(o, Vec3::new(0.0, 0.0, 1.0), &mut dist), 1);
        assert!((dist - 9.0).abs() < 1e-4);
        assert!((solid.high.x - 3.0).abs() < 1e-4 && (solid.high.y - 1.0).abs() < 1e-4);
    }
}

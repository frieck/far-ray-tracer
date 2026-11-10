//! KD-Tree construction with the SAH heuristic.
//!
//! The resulting tree is the same as the original's. What changes is how it is
//! computed and stored: indices into a vector instead of pointers tagged in their
//! low bits, and counts computed in parallel. The original build can take longer
//! than the render itself.

use crate::scene::{Scene, Primitive, Triangle};
use crate::vector::{Aabb, Vec3};
use rayon::prelude::*;

/// A split whose cost is above this share of the leaf's is poor enough to try the other axes.
const POOR_SPLIT: f32 = 0.6;
/// The original's depth limit, which Classic mode keeps to match it.
const MAX_DEPTH: i32 = 20;
/// The corrected tree grows deeper with the scene, so large meshes end in small leaves: the
/// usual 8 + 1.3 log2(n) of KD-Tree builders, never below the original's limit.
fn corrected_depth(primitives: usize) -> i32 {
    ((8.0 + 1.3 * (primitives.max(1) as f32).log2()) as i32).clamp(MAX_DEPTH, 40)
}
const LEAF_AXIS: u32 = 3;
/// Below this, splitting the work between threads does not pay off.
const MIN_PARALLEL: usize = 256;
/// Scenes beyond this size, far larger than any the original could load, such as displaced
/// meshes, choose splits by a sweep over primitive extents. The original's exact counting
/// tests every primitive at every candidate, which grows with the square of the primitives.
const LARGE_SCENE: usize = 50_000;
/// Fixed and Enhanced modes use the sweep from this size: exact counting took seconds for a
/// single model of 30000 triangles. Classic keeps the original's build up to `LARGE_SCENE`.
const LARGE_SCENE_CORRECTED: usize = 10_000;
/// In a large scene, nodes with at least this many primitives use the sweep.
const SWEEP_FROM: usize = 512;

#[derive(Clone, Copy)]
pub struct Node {
    a: u32,
    b: u32,
}

impl Node {
    #[inline(always)]
    pub fn is_leaf(self) -> bool {
        self.b & 3 == LEAF_AXIS
    }
    #[inline(always)]
    pub fn axis(self) -> usize {
        (self.b & 3) as usize
    }
    #[inline(always)]
    pub fn split_position(self) -> f32 {
        f32::from_bits(self.a)
    }
    /// The two children are adjacent: right is always `left + 1`.
    #[inline(always)]
    pub fn left(self) -> u32 {
        self.b >> 2
    }
    #[inline(always)]
    pub fn span(self) -> std::ops::Range<usize> {
        let start = (self.b >> 2) as usize;
        start..start + self.a as usize
    }
}

pub struct KdTree {
    pub nodes: Vec<Node>,
    pub list: Vec<u32>,
}

enum BuildNode {
    Leaf(Vec<u32>),
    Interior { axis: usize, position: f32, children: Box<(BuildNode, BuildNode)> },
}

impl KdTree {
    /// The tree of the original, bugs included. Classic mode needs it to match the
    /// C++ program pixel for pixel.
    pub fn build(scene: &Scene) -> Self {
        Self::build_with(scene, false)
    }

    /// The tree for Fixed and Enhanced modes. A triangle lying exactly on a split
    /// plane, such as a wall, can drop out of both children in the original tree,
    /// and rays then pass through it.
    pub fn build_corrected(scene: &Scene) -> Self {
        Self::build_with(scene, true)
    }

    pub fn for_mode(scene: &Scene, mode: crate::Mode) -> Self {
        Self::build_with(scene, mode != crate::Mode::Classic)
    }

    fn build_with(scene: &Scene, corrected: bool) -> Self {
        // The original inserts at the head of a list, hence the reversed order.
        // Infinite planes have no bounds; the traversal tests them on its own.
        let root: Vec<u32> = (0..scene.prims.len() as u32).rev().filter(|&i| !scene.grounds.contains(&(i as usize))).collect();
        let large = if corrected { LARGE_SCENE_CORRECTED } else { LARGE_SCENE };
        let sweep_from = if scene.prims.len() > large { SWEEP_FROM } else { usize::MAX };
        let max_depth = if corrected { corrected_depth(root.len()) } else { MAX_DEPTH };
        let tree = subdivide(scene, corrected, sweep_from, max_depth, root, scene.extends, 0);
        let mut kd = KdTree { nodes: vec![Node { a: 0, b: LEAF_AXIS }], list: Vec::new() };
        kd.flatten(tree, 0);
        kd
    }

    fn flatten(&mut self, c: BuildNode, index: usize) {
        match c {
            BuildNode::Leaf(list) => {
                let start = self.list.len() as u32;
                self.nodes[index] = Node { a: list.len() as u32, b: (start << 2) | LEAF_AXIS };
                self.list.extend(list);
            }
            BuildNode::Interior { axis, position, children } => {
                let left = self.nodes.len();
                self.nodes.push(Node { a: 0, b: LEAF_AXIS });
                self.nodes.push(Node { a: 0, b: LEAF_AXIS });
                self.nodes[index] = Node { a: position.to_bits(), b: ((left as u32) << 2) | axis as u32 };
                let (e, d) = *children;
                self.flatten(e, left);
                self.flatten(d, left + 1);
            }
        }
    }
}

fn span(scene: &Scene, p: &Primitive, axis: usize) -> (f32, f32) {
    match p {
        Primitive::Ground(_) => unreachable!("grounds stay out of the KD-Tree"),
        Primitive::Sphere(e) => (e.center.cell(axis) - e.radius, e.center.cell(axis) + e.radius),
        Primitive::Solid(s) => (s.low.cell(axis), s.high.cell(axis)),
        Primitive::Triangle(t) => {
            let mut a = scene.vertices[t.vertices[0] as usize].pos.cell(axis);
            let mut b = a;
            for &v in &t.vertices[1..] {
                let c = scene.vertices[v as usize].pos.cell(axis);
                if c < a { a = c; }
                if c > b { b = c; }
            }
            (a, b)
        }
    }
}

fn intersects_box(scene: &Scene, corrected: bool, p: &Primitive, bounds: &Aabb) -> bool {
    match p {
        Primitive::Ground(_) => unreachable!("grounds stay out of the KD-Tree"),
        // Testing the solid's own box may keep it in a cell it does not touch. That only
        // costs an extra test, and it never loses a hit.
        Primitive::Solid(s) => (0..3).all(|i| {
            let low = bounds.pos.cell(i);
            s.low.cell(i) <= low + bounds.size.cell(i) && s.high.cell(i) >= low
        }),
        Primitive::Sphere(e) => {
            let mut dmin = 0.0f32;
            let (s, b, t) = (e.center.array(), bounds.pos.array(), bounds.size.array());
            for i in 0..3 {
                if s[i] < b[i] {
                    dmin += (s[i] - b[i]) * (s[i] - b[i]);
                } else if s[i] > b[i] + t[i] {
                    dmin += (s[i] - (b[i] + t[i])) * (s[i] - (b[i] + t[i]));
                }
            }
            dmin <= e.sq_radius
        }
        Primitive::Triangle(t) => {
            let half = bounds.size * 0.5;
            // Computing the center rounds it, so a triangle lying exactly on a face of the
            // box, such as a wall on a split plane, can test as just outside and be left
            // out of both children. A margin far below any scene detail keeps it in.
            // The original has no margin, and Classic mode keeps it that way.
            let margin = if corrected { half.x.max(half.y).max(half.z) * 1e-5 } else { 0.0 };
            tri_box(scene, t, bounds.pos + half, half + Vec3::new(margin, margin, margin))
        }
    }
}

fn plane_overlaps_box(normal: Vec3, vert: Vec3, max_box: Vec3) -> bool {
    let (mut vmin, mut vmax) = (Vec3::default(), Vec3::default());
    for q in 0..3 {
        let v = vert.cell(q);
        if normal.cell(q) > 0.0 {
            vmin.set_cell(q, -max_box.cell(q) - v);
            vmax.set_cell(q, max_box.cell(q) - v);
        } else {
            vmin.set_cell(q, max_box.cell(q) - v);
            vmax.set_cell(q, -max_box.cell(q) - v);
        }
    }
    if normal.dot(vmin) > 0.0 {
        return false;
    }
    normal.dot(vmax) >= 0.0
}

/// Separating axis test between a triangle and a box (Akenine-Moller).
fn tri_box(scene: &Scene, t: &Triangle, center: Vec3, half: Vec3) -> bool {
    let v0 = scene.vertices[t.vertices[0] as usize].pos - center;
    let v1 = scene.vertices[t.vertices[1] as usize].pos - center;
    let v2 = scene.vertices[t.vertices[2] as usize].pos - center;
    let (e0, e1, e2) = (v1 - v0, v2 - v1, v0 - v2);
    let h = half.array();
    let (a0, a1, a2) = (v0.array(), v1.array(), v2.array());

    let separated = |p: f32, q: f32, rad: f32| {
        let (min, max) = if p < q { (p, q) } else { (q, p) };
        min > rad || max < -rad
    };
    let x = |v: [f32; 3], a: f32, b: f32| a * v[1] - b * v[2];
    let y = |v: [f32; 3], a: f32, b: f32| -a * v[0] + b * v[2];
    let z = |v: [f32; 3], a: f32, b: f32| a * v[0] - b * v[1];

    let (fex, fey, fez) = (e0.x.abs(), e0.y.abs(), e0.z.abs());
    if separated(x(a0, e0.z, e0.y), x(a2, e0.z, e0.y), fez * h[1] + fey * h[2]) { return false; }
    if separated(y(a0, e0.z, e0.x), y(a2, e0.z, e0.x), fez * h[0] + fex * h[2]) { return false; }
    if separated(z(a1, e0.y, e0.x), z(a2, e0.y, e0.x), fey * h[0] + fex * h[1]) { return false; }

    let (fex, fey, fez) = (e1.x.abs(), e1.y.abs(), e1.z.abs());
    if separated(x(a0, e1.z, e1.y), x(a2, e1.z, e1.y), fez * h[1] + fey * h[2]) { return false; }
    if separated(y(a0, e1.z, e1.x), y(a2, e1.z, e1.x), fez * h[0] + fex * h[2]) { return false; }
    if separated(z(a0, e1.y, e1.x), z(a1, e1.y, e1.x), fey * h[0] + fex * h[1]) { return false; }

    let (fex, fey, fez) = (e2.x.abs(), e2.y.abs(), e2.z.abs());
    if separated(x(a0, e2.z, e2.y), x(a1, e2.z, e2.y), fez * h[1] + fey * h[2]) { return false; }
    if separated(y(a0, e2.z, e2.x), y(a1, e2.z, e2.x), fez * h[0] + fex * h[2]) { return false; }
    if separated(z(a1, e2.y, e2.x), z(a2, e2.y, e2.x), fey * h[0] + fex * h[1]) { return false; }

    for i in 0..3 {
        let (p, q, r) = (a0[i], a1[i], a2[i]);
        let (mut min, mut max) = (p, p);
        if q < min { min = q; }
        if q > max { max = q; }
        if r < min { min = r; }
        if r > max { max = r; }
        if min > h[i] || max < -h[i] {
            return false;
        }
    }
    plane_overlaps_box(e0.cross(e1), v0, half)
}

fn split_bounds(bounds: &Aabb, axis: usize, position: f32, pos1: f32, pos2: f32) -> (Aabb, Aabb) {
    let (mut left, mut right) = (*bounds, *bounds);
    left.size.set_cell(axis, position - pos1);
    right.pos.set_cell(axis, position);
    right.size.set_cell(axis, pos2 - position);
    (left, right)
}

struct SplitCounts {
    n1: Vec<u32>,
    n2: Vec<u32>,
    /// `rest[s]`: primitives that count on the left from split `s` onward.
    rest: Vec<u32>,
}

impl SplitCounts {
    fn new(n: usize) -> Self {
        Self { n1: vec![0; n], n2: vec![0; n], rest: vec![0; n + 1] }
    }

    fn sum(mut self, o: Self) -> Self {
        for (a, b) in self.n1.iter_mut().zip(&o.n1) { *a += b; }
        for (a, b) in self.n2.iter_mut().zip(&o.n2) { *a += b; }
        for (a, b) in self.rest.iter_mut().zip(&o.rest) { *a += b; }
        self
    }
}

struct SplitPlan<'a> {
    scene: &'a Scene,
    corrected: bool,
    bounds: Aabb,
    axis: usize,
    pos1: f32,
    pos2: f32,
    candidates: &'a [f32],
}

impl SplitPlan<'_> {
    /// Counts one primitive at every candidate. The original loops over candidates
    /// and then primitives; since each primitive evolves independently, swapping the
    /// loops gives the same numbers and allows running in parallel.
    fn count(&self, mut c: SplitCounts, p: &Primitive, lo: f32, hi: f32) -> SplitCounts {
        let (b3p1, b4p2) = (self.pos1, self.pos2);
        for (s, &position) in self.candidates.iter().enumerate() {
            let (b3, b4) = split_bounds(&self.bounds, self.axis, position, self.pos1, self.pos2);
            let b3p2 = b3.pos.cell(self.axis) + b3.size.cell(self.axis);
            if lo <= b3p2 && hi >= b3p1 && intersects_box(self.scene, self.corrected, p, &b3) {
                c.n1[s] += 1;
            }
            if lo <= b4p2 && hi >= position {
                if intersects_box(self.scene, self.corrected, p, &b4) {
                    c.n2[s] += 1;
                } else {
                    // Left the right box: it counts on the left from here on.
                    c.rest[s + 1] += 1;
                    break;
                }
            }
        }
        c
    }
}

/// Primitives on the left and on the right of each candidate, from their extents along the axis:
/// a primitive counts on a side when its extent reaches into it.
fn sweep_counts(spans: &[(f32, f32)], candidates: &[f32]) -> (Vec<u32>, Vec<u32>) {
    let mut lows: Vec<f32> = spans.iter().map(|s| s.0).collect();
    let mut highs: Vec<f32> = spans.iter().map(|s| s.1).collect();
    lows.sort_by(f32::total_cmp);
    highs.sort_by(f32::total_cmp);
    let n = spans.len() as u32;
    candidates.iter().map(|&x| {
        let left = lows.partition_point(|&v| v <= x) as u32;
        let right = n - highs.partition_point(|&v| v < x) as u32;
        (left, right)
    }).unzip()
}

fn subdivide(scene: &Scene, corrected: bool, sweep_from: usize, max_depth: i32, list: Vec<u32>, bounds: Aabb, depth: i32) -> BuildNode {
    let n = list.len();
    let s = bounds.size;
    let longest = if s.x >= s.y && s.x >= s.z {
        0
    } else if s.y >= s.x && s.y >= s.z {
        1
    } else {
        2
    };
    // The original splits only across the longest side. The corrected tree also tries the
    // other two and keeps the cheapest, so a bundle of long thin triangles (the side of a vase,
    // say) still splits.
    let mut split = best_split(scene, corrected, sweep_from, &list, bounds, longest);
    // Most of the time the longest side splits well; the others are tried only when it does
    // not, which keeps building about as fast as the original's.
    if corrected && split.lowest > POOR_SPLIT * n as f32 {
        let (a, b) = ((longest + 1) % 3, (longest + 2) % 3);
        // Counted by extents, which is fast; exact counts here would make building slower.
        let try_axis = |axis: usize| (s.cell(axis) > 0.0).then(|| best_split(scene, corrected, 0, &list, bounds, axis));
        let (first, second) = if n >= MIN_PARALLEL { rayon::join(|| try_axis(a), || try_axis(b)) } else { (try_axis(a), try_axis(b)) };
        for other in [first, second].into_iter().flatten() {
            if other.lowest < split.lowest {
                split = other;
            }
        }
    }
    let Split { axis, best, b1, b2, lowest, spans } = split;
    finish(scene, corrected, sweep_from, max_depth, list, spans, axis, best, b1, b2, lowest, n as f32, depth)
}

/// The cheapest split of `list` across one axis, by the original's cost.
struct Split {
    axis: usize,
    best: f32,
    b1: Aabb,
    b2: Aabb,
    lowest: f32,
    spans: Vec<(f32, f32)>,
}

fn best_split(scene: &Scene, corrected: bool, sweep_from: usize, list: &[u32], bounds: Aabb, axis: usize) -> Split {
    let n = list.len();
    let pos1 = bounds.pos.cell(axis);
    let pos2 = bounds.pos.cell(axis) + bounds.size.cell(axis);

    let mut candidates = Vec::with_capacity(n * 2);
    let mut spans = Vec::with_capacity(n);
    for &i in list {
        let p = &scene.prims[i as usize];
        spans.push(span(scene, p, axis));
        let mut add = |v: f32| {
            if v >= pos1 && v <= pos2 {
                candidates.push(v);
            }
        };
        match p {
            Primitive::Sphere(e) => {
                add(e.center.cell(axis) - e.radius);
                add(e.center.cell(axis) + e.radius);
            }
            Primitive::Solid(s) => {
                add(s.low.cell(axis));
                add(s.high.cell(axis));
            }
            Primitive::Triangle(t) => {
                for &v in &t.vertices {
                    add(scene.vertices[v as usize].pos.cell(axis));
                }
            }
            Primitive::Ground(_) => unreachable!("grounds stay out of the KD-Tree"),
        }
    }
    candidates.sort_by(|a, b| a.partial_cmp(b).expect("candidates are never NaN"));
    candidates.dedup();

    let (left_counts, right_counts) = if n >= sweep_from {
        sweep_counts(&spans, &candidates)
    } else {
        exact_counts(scene, corrected, list, &spans, &candidates, bounds, axis, pos1, pos2)
    };

    let sav = 0.5 / bounds.semi_area();
    // The original starts from a cost of 10000. Past about 10000 primitives no split costs less,
    // and it splits into two empty boxes, losing everything, so a model of 30000 triangles
    // vanished. Only Classic mode keeps that, to match the original; it never loaded so much.
    let mut lowest = if sweep_from == usize::MAX && !corrected { 10000.0f32 } else { f32::INFINITY };
    let mut best = 0.0f32;
    let (mut b1, mut b2) = (Aabb::default(), Aabb::default());
    for (k, &position) in candidates.iter().enumerate() {
        let (b3, b4) = split_bounds(&bounds, axis, position, pos1, pos2);
        let n1 = left_counts[k] as f32;
        let n2 = right_counts[k] as f32;
        let sa1 = 2.0 * b3.semi_area();
        let sa2 = 2.0 * b4.semi_area();
        let cost = 0.3 + 1.0 * (sa1 * sav * n1 + sa2 * sav * n2);
        if cost < lowest {
            lowest = cost;
            best = position;
            b1 = b3;
            b2 = b4;
        }
    }
    Split { axis, best, b1, b2, lowest, spans }
}

/// The original's counts: each primitive tested exactly against the boxes of every candidate.
#[allow(clippy::too_many_arguments)]
fn exact_counts(
    scene: &Scene, corrected: bool, list: &[u32], spans: &[(f32, f32)], candidates: &[f32],
    bounds: Aabb, axis: usize, pos1: f32, pos2: f32,
) -> (Vec<u32>, Vec<u32>) {
    let n = list.len();
    let plan = SplitPlan { scene, corrected, bounds, axis, pos1, pos2, candidates };
    let m = candidates.len();
    let counts = if n >= MIN_PARALLEL {
        (0..n)
            .into_par_iter()
            .with_min_len(16)
            .fold(
                || SplitCounts::new(m),
                |c, k| {
                    let (lo, hi) = spans[k];
                    plan.count(c, &scene.prims[list[k] as usize], lo, hi)
                },
            )
            .reduce(|| SplitCounts::new(m), SplitCounts::sum)
    } else {
        (0..n).fold(SplitCounts::new(m), |c, k| {
            let (lo, hi) = spans[k];
            plan.count(c, &scene.prims[list[k] as usize], lo, hi)
        })
    };
    let mut running = 0;
    let left = (0..m).map(|k| {
        running += counts.rest[k];
        counts.n1[k] + running
    }).collect();
    (left, counts.n2)
}

/// Makes a leaf, or splits `list` at `best` into the two boxes and builds both sides.
#[allow(clippy::too_many_arguments)]
fn finish(
    scene: &Scene, corrected: bool, sweep_from: usize, max_depth: i32, list: Vec<u32>, spans: Vec<(f32, f32)>,
    axis: usize, best: f32, b1: Aabb, b2: Aabb, lowest: f32, leaf_cost: f32, depth: i32,
) -> BuildNode {
    let n = list.len();
    if lowest > leaf_cost {
        return BuildNode::Leaf(list);
    }

    let (b1p1, b1p2) = (b1.pos.cell(axis), b1.pos.cell(axis) + b1.size.cell(axis));
    let (b2p1, b2p2) = (b2.pos.cell(axis), b2.pos.cell(axis) + b2.size.cell(axis));
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for (k, &i) in list.iter().enumerate() {
        let p = &scene.prims[i as usize];
        let (lo, hi) = spans[k];
        if lo <= b1p2 && hi >= b1p1 && intersects_box(scene, corrected, p, &b1) {
            left.push(i);
        }
        if lo <= b2p2 && hi >= b2p1 && intersects_box(scene, corrected, p, &b2) {
            right.push(i);
        }
    }
    // Same order as inserting at the head of the original linked list.
    left.reverse();
    right.reverse();
    drop(list);

    let may_split = depth < max_depth;
    let branch = |l: Vec<u32>, c: Aabb| {
        if may_split && l.len() > 2 {
            subdivide(scene, corrected, sweep_from, max_depth, l, c, depth + 1)
        } else {
            BuildNode::Leaf(l)
        }
    };
    let children = if n >= MIN_PARALLEL {
        rayon::join(|| branch(left, b1), || branch(right, b2))
    } else {
        (branch(left, b1), branch(right, b2))
    };
    BuildNode::Interior { axis, position: best, children: Box::new(children) }
}

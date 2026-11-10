//! A bounding volume hierarchy over the scene's bounded primitives: the acceleration structure
//! of Fixed and Enhanced modes, and the one a GPU can walk.
//!
//! Each node is a box around its primitives; an inner node has two children, stored side by
//! side, and a leaf lists a few primitives. The nodes sit in one flat array with no pointers,
//! and a ray walks them with a small stack, nearer child first, so the same data and the same
//! walk work in a compute shader. Built top down by the surface area heuristic over 16 bins.
//! Infinite grounds have no box and stay outside, as with the KD-Tree.

use crate::scene::{Primitive, Scene};
use crate::vector::Vec3;

/// Most primitives in a leaf; fewer where a split would cost more than testing them all.
const LEAF_SIZE: usize = 4;
const BINS: usize = 16;
/// Nodes with at least this many primitives build their two sides in parallel.
const PARALLEL_FROM: usize = 4096;
/// Deep enough for any tree: the build splits by count when bins fail, so depth stays near
/// log2 of the primitives.
const STACK: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub min: [f32; 3],
    /// A leaf: the first of its primitives in `Bvh::prims`. An inner node: its left child; the
    /// right one follows it.
    pub first: u32,
    pub max: [f32; 3],
    /// Primitives in a leaf; 0 marks an inner node.
    pub count: u32,
}

pub struct Bvh {
    pub nodes: Vec<Node>,
    /// Primitive indices, grouped by leaf.
    pub prims: Vec<u32>,
}

#[derive(Clone, Copy)]
struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}

impl Bounds {
    const EMPTY: Self = Self { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3] };

    fn grow(&mut self, other: &Bounds) {
        for k in 0..3 {
            self.min[k] = self.min[k].min(other.min[k]);
            self.max[k] = self.max[k].max(other.max[k]);
        }
    }

    fn half_area(&self) -> f32 {
        let d = [self.max[0] - self.min[0], self.max[1] - self.min[1], self.max[2] - self.min[2]];
        if d[0] < 0.0 { 0.0 } else { d[0] * d[1] + d[1] * d[2] + d[2] * d[0] }
    }
}

fn bounds_of(scene: &Scene, p: &Primitive) -> Option<Bounds> {
    let mut b = Bounds::EMPTY;
    match p {
        Primitive::Ground(_) => return None,
        Primitive::Sphere(s) => {
            let c = s.center.array();
            for k in 0..3 {
                b.min[k] = c[k] - s.radius;
                b.max[k] = c[k] + s.radius;
            }
        }
        Primitive::Solid(s) => {
            b.min = s.low.array();
            b.max = s.high.array();
        }
        Primitive::Triangle(t) => {
            for &v in &t.vertices {
                let pos = scene.vertices[v as usize].pos.array();
                for k in 0..3 {
                    b.min[k] = b.min[k].min(pos[k]);
                    b.max[k] = b.max[k].max(pos[k]);
                }
            }
        }
    }
    // A primitive with no real position cannot be hit; leaving it out keeps the boxes finite.
    b.min.iter().chain(&b.max).all(|v| v.is_finite()).then_some(b)
}

struct Item {
    prim: u32,
    bounds: Bounds,
    center: [f32; 3],
}

enum Built {
    Leaf(Bounds, Vec<u32>),
    Inner(Bounds, Box<(Built, Built)>),
}

fn build(mut items: Vec<Item>) -> Built {
    let mut bounds = Bounds::EMPTY;
    let mut centers = Bounds::EMPTY;
    for item in &items {
        bounds.grow(&item.bounds);
        centers.grow(&Bounds { min: item.center, max: item.center });
    }
    let n = items.len();
    if n <= 1 {
        return Built::Leaf(bounds, items.iter().map(|i| i.prim).collect());
    }
    // The cheapest of the splits between bins, across the three axes: the cost of a side is its
    // box's area times its primitives, against testing all of them here.
    let mut best: Option<(f32, usize, f32)> = None;
    for axis in 0..3 {
        let (low, extent) = (centers.min[axis], centers.max[axis] - centers.min[axis]);
        if !(extent > 0.0) {
            continue;
        }
        let scale = BINS as f32 / extent;
        let mut bins = [(Bounds::EMPTY, 0usize); BINS];
        for item in &items {
            let k = (((item.center[axis] - low) * scale) as usize).min(BINS - 1);
            bins[k].0.grow(&item.bounds);
            bins[k].1 += 1;
        }
        let mut right = [(0.0f32, 0usize); BINS];
        let (mut acc, mut count) = (Bounds::EMPTY, 0);
        for k in (1..BINS).rev() {
            acc.grow(&bins[k].0);
            count += bins[k].1;
            right[k] = (acc.half_area(), count);
        }
        let (mut acc, mut count) = (Bounds::EMPTY, 0);
        for k in 0..BINS - 1 {
            acc.grow(&bins[k].0);
            count += bins[k].1;
            let (area, others) = right[k + 1];
            if count == 0 || others == 0 {
                continue;
            }
            let cost = acc.half_area() * count as f32 + area * others as f32;
            if best.is_none_or(|(c, _, _)| cost < c) {
                best = Some((cost, axis, low + (k + 1) as f32 / scale));
            }
        }
    }
    let leaf_cost = bounds.half_area() * n as f32;
    let (left, right): (Vec<Item>, Vec<Item>) = match best {
        Some((cost, _, _)) if n <= LEAF_SIZE && cost >= leaf_cost => {
            return Built::Leaf(bounds, items.iter().map(|i| i.prim).collect());
        }
        Some((_, axis, at)) => {
            let (mut left, mut right): (Vec<Item>, Vec<Item>) = items.into_iter().partition(|i| i.center[axis] < at);
            // Rounding can put a bin's edge on the other side of every center; halve instead,
            // so the build always ends.
            if left.is_empty() || right.is_empty() {
                left.append(&mut right);
                right = left.split_off(n / 2);
            }
            (left, right)
        }
        // Every center in one place: no split separates them.
        None if n <= LEAF_SIZE => return Built::Leaf(bounds, items.iter().map(|i| i.prim).collect()),
        None => {
            let right = items.split_off(n / 2);
            (items, right)
        }
    };
    let children = if n >= PARALLEL_FROM {
        rayon::join(|| build(left), || build(right))
    } else {
        (build(left), build(right))
    };
    Built::Inner(bounds, Box::new(children))
}

impl Bvh {
    pub fn build(scene: &Scene) -> Self {
        let items: Vec<Item> = scene.prims.iter().enumerate()
            .filter_map(|(i, p)| bounds_of(scene, p).map(|bounds| Item {
                prim: i as u32,
                center: [0, 1, 2].map(|k| 0.5 * (bounds.min[k] + bounds.max[k])),
                bounds,
            }))
            .collect();
        let mut bvh = Bvh { nodes: Vec::with_capacity(items.len() / 2 + 1), prims: Vec::with_capacity(items.len()) };
        bvh.nodes.push(Node { min: [0.0; 3], max: [0.0; 3], first: 0, count: 0 });
        if items.is_empty() {
            // One empty leaf that no ray enters.
            bvh.nodes[0] = Node { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3], first: 0, count: 0 };
            return bvh;
        }
        let tree = build(items);
        bvh.flatten(tree, 0);
        bvh
    }

    fn flatten(&mut self, built: Built, index: usize) {
        match built {
            Built::Leaf(b, prims) => {
                self.nodes[index] = Node { min: b.min, max: b.max, first: self.prims.len() as u32, count: prims.len() as u32 };
                self.prims.extend(prims);
            }
            Built::Inner(b, children) => {
                let left = self.nodes.len();
                self.nodes.push(Node { min: [0.0; 3], max: [0.0; 3], first: 0, count: 0 });
                self.nodes.push(Node { min: [0.0; 3], max: [0.0; 3], first: 0, count: 0 });
                self.nodes[index] = Node { min: b.min, max: b.max, first: left as u32, count: 0 };
                let (l, r) = *children;
                self.flatten(l, left);
                self.flatten(r, left + 1);
            }
        }
    }

    /// Walks the leaves the ray may cross, nearer first, calling `visit` with each primitive
    /// and the farthest distance still of interest; `visit` returns the new such distance, as
    /// when it finds a hit, or a negative one to stop the walk.
    #[inline]
    pub fn walk(&self, o: Vec3, d: Vec3, mut limit: f32, mut visit: impl FnMut(usize, f32) -> f32) {
        let (o, d) = (o.array(), d.array());
        let inv = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
        // Where the ray enters a node's box, if it does before `limit`.
        let entry = |node: &Node, limit: f32| -> Option<f32> {
            let (mut near, mut far) = (0.0f32, limit);
            for k in 0..3 {
                let (a, b) = ((node.min[k] - o[k]) * inv[k], (node.max[k] - o[k]) * inv[k]);
                // `min` and `max` drop the NaN of a ray that runs along a face of the box.
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
            (near <= far).then_some(near)
        };
        let root = &self.nodes[0];
        if root.count == 0 && self.nodes.len() == 1 || entry(root, limit).is_none() {
            return;
        }
        let mut stack = [(0u32, 0.0f32); STACK];
        let mut top = 0;
        let mut index = 0u32;
        loop {
            let node = &self.nodes[index as usize];
            if node.count > 0 {
                for k in node.first..node.first + node.count {
                    limit = visit(self.prims[k as usize] as usize, limit);
                    if limit < 0.0 {
                        return;
                    }
                }
            } else {
                let (l, r) = (node.first, node.first + 1);
                match (entry(&self.nodes[l as usize], limit), entry(&self.nodes[r as usize], limit)) {
                    (Some(a), Some(b)) => {
                        let (near, far, far_at) = if a <= b { (l, r, b) } else { (r, l, a) };
                        if top < STACK {
                            stack[top] = (far, far_at);
                            top += 1;
                        }
                        index = near;
                        continue;
                    }
                    (Some(_), None) => { index = l; continue; }
                    (None, Some(_)) => { index = r; continue; }
                    (None, None) => {}
                }
            }
            // The next node still ahead of the nearest hit.
            loop {
                if top == 0 {
                    return;
                }
                top -= 1;
                if stack[top].1 <= limit {
                    index = stack[top].0;
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn example(name: &str) -> Scene {
        Scene::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes").join(name)).unwrap()
    }

    #[test]
    fn every_bounded_primitive_is_in_exactly_one_leaf_inside_its_box() {
        for name in ["showcase.far", "techniques/models.far", "cornell-box.far", "techniques/primitives.far"] {
            let scene = example(name);
            let bvh = Bvh::build(&scene);
            let mut seen = vec![0u32; scene.prims.len()];
            for node in bvh.nodes.iter().filter(|n| n.count > 0) {
                for k in node.first..node.first + node.count {
                    let prim = bvh.prims[k as usize] as usize;
                    seen[prim] += 1;
                    let b = bounds_of(&scene, &scene.prims[prim]).unwrap();
                    assert!((0..3).all(|a| b.min[a] >= node.min[a] && b.max[a] <= node.max[a]), "{name}: primitive {prim}");
                }
            }
            for (i, p) in scene.prims.iter().enumerate() {
                let expected = if bounds_of(&scene, p).is_some() { 1 } else { 0 };
                assert_eq!(seen[i], expected, "{name}: primitive {i}");
            }
            // Children sit inside their parent.
            for node in bvh.nodes.iter().filter(|n| n.count == 0 && bvh.nodes.len() > 1) {
                for child in [node.first, node.first + 1] {
                    let c = &bvh.nodes[child as usize];
                    assert!((0..3).all(|a| c.min[a] >= node.min[a] && c.max[a] <= node.max[a]), "{name}");
                }
            }
        }
    }

    #[test]
    fn an_empty_scene_has_a_tree_no_ray_enters() {
        let scene = Scene::from_far(br#"{"lights":[],"objects":[{"type":"ground"}]}"#, Path::new("t.far")).unwrap();
        let bvh = Bvh::build(&scene);
        let mut visited = 0;
        bvh.walk(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 100.0, |_, limit| { visited += 1; limit });
        assert_eq!(visited, 0);
    }
}

//! Displacement: a height map that moves the surface itself, so silhouettes, shadows and
//! contact show the relief, unlike a bump map, which only changes the shading.
//!
//! Objects are tessellated once, when the scene loads: each shape becomes a dense mesh whose
//! vertices move along the surface normal by the map's height, and the mesh joins the
//! KD-Tree like any other. Shading does not use the mesh's facets: a displaced triangle
//! remembers where it lies on the original shape, so its color and its normal come from the
//! shape and the map at the exact point, as fine as the map itself.
//!
//! The infinite ground cannot become a mesh. Rays march over it instead: they enter the slab
//! the relief can fill, step across it a texel at a time and refine the crossing.

use crate::bump::{Bump, BumpKind};
use crate::engine::Ray;
use crate::scene::{Ground, Primitive, Scene, Triangle, Vertex};
use crate::solid::Shape;
use crate::vector::{Vec3, PI};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Displacement {
    pub texture: usize,
    /// Distance white moves the surface out, in scene units; black stays in place.
    pub height: f32,
    /// Divisions across the object's width. Higher shows smaller relief in the outline.
    pub detail: u32,
}

impl Displacement {
    /// The same map as a bump, which gives the normal of the displaced surface.
    fn as_bump(&self) -> Bump {
        Bump { texture: self.texture, kind: BumpKind::Height, strength: self.height }
    }
}

pub const DEFAULT_DETAIL: u32 = 128;
/// Triangles all displaced objects may produce together; detail is lowered to fit.
const MAX_TRIANGLES: usize = 4_000_000;

/// Where a displaced triangle lies on the shape it came from.
#[derive(Clone, Copy, Debug)]
pub struct DisplacedFace {
    /// Index into `Scene::displaced_sources`.
    pub source: u32,
    /// Corners before displacement, in the triangle's corner order.
    pub base: [Vec3; 3],
    /// The shape's own normals at those corners.
    pub normals: [Vec3; 3],
}

impl DisplacedFace {
    /// Point and normal on the original shape under barycentric `uv` of the triangle.
    pub fn base_at(&self, uv: [f32; 2]) -> (Vec3, Vec3) {
        let mix = |v: [Vec3; 3]| v[0] + uv[0] * (v[1] - v[0]) + uv[1] * (v[2] - v[0]);
        let mut normal = mix(self.normals);
        normal.normalize();
        (mix(self.base), normal)
    }
}

/// Height of the relief at a point of a shape, in scene units.
fn height_at(scene: &Scene, p: &Primitive, d: &Displacement, pos: Vec3) -> f32 {
    scene.texture_uv_on(p, pos).map_or(0.0, |(u, v)| {
        let c = scene.textures[d.texture].texel_fixed(u, v);
        d.height * (c.x + c.y + c.z) / 3.0
    })
}

/// The shading normal of a displaced surface at a point of the original shape.
pub fn displaced_normal(scene: &Scene, p: &Primitive, base: Vec3, normal: Vec3) -> Vec3 {
    match scene.materials[p.material()].displacement {
        Some(d) => crate::bump::tilt(scene, p, base, normal, d.as_bump()),
        None => normal,
    }
}

/// Points of a surface with their normals, and the triangles between them.
#[derive(Default)]
struct Patches {
    points: Vec<(Vec3, Vec3)>,
    triangles: Vec<[u32; 3]>,
}

impl Patches {
    /// A grid over `f(s, t)`, s and t from 0 to 1.
    fn grid(&mut self, columns: u32, rows: u32, f: impl Fn(f32, f32) -> (Vec3, Vec3)) {
        let first = self.points.len() as u32;
        for i in 0..=rows {
            for j in 0..=columns {
                self.points.push(f(j as f32 / columns as f32, i as f32 / rows as f32));
            }
        }
        let at = |i: u32, j: u32| first + i * (columns + 1) + j;
        for i in 0..rows {
            for j in 0..columns {
                let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j + 1), at(i + 1, j));
                self.triangles.extend([[a, b, c], [a, c, d]]);
            }
        }
    }

    /// A triangle split into `k`² smaller ones, its corners given with their normals.
    fn triangle(&mut self, k: u32, corners: [(Vec3, Vec3); 3]) {
        let first = self.points.len() as u32;
        let [(a, na), (b, nb), (c, nc)] = corners;
        let mut rows = Vec::new();
        for i in 0..=k {
            rows.push(self.points.len() as u32 - first);
            for j in 0..=(k - i) {
                let (s, t) = (j as f32 / k as f32, i as f32 / k as f32);
                let mut n = na + s * (nb - na) + t * (nc - na);
                n.normalize();
                self.points.push((a + s * (b - a) + t * (c - a), n));
            }
        }
        let at = |i: u32, j: u32| first + rows[i as usize] + j;
        for i in 0..k {
            for j in 0..(k - i) {
                self.triangles.push([at(i, j), at(i, j + 1), at(i + 1, j)]);
                if j + 1 < k - i {
                    self.triangles.push([at(i, j + 1), at(i + 1, j + 1), at(i + 1, j)]);
                }
            }
        }
    }
}

fn divisions(length: f32, step: f32) -> u32 {
    ((length / step).ceil() as u32).clamp(1, 8192)
}

/// The surface of a shape as patches, with edges about `step` long.
fn tessellate(scene: &Scene, p: &Primitive, corner_normals: [Vec3; 3], step: f32, mesh_k: u32) -> Patches {
    let mut out = Patches::default();
    let around = |s: f32| (2.0 * PI * s).sin_cos();
    match p {
        Primitive::Sphere(e) => {
            let r = e.radius;
            out.grid(divisions(2.0 * PI * r, step), divisions(PI * r, step), |s, t| {
                let (sin_a, cos_a) = around(s);
                let (sin_p, cos_p) = (PI * t).sin_cos();
                let n = Vec3::new(sin_p * cos_a, -cos_p, sin_p * sin_a);
                (e.center + n * r, n)
            });
        }
        Primitive::Solid(solid) => {
            let world = |(pos, n): (Vec3, Vec3)| (solid.to_world(pos), solid.direction_to_world(n));
            let side = |out: &mut Patches, columns: u32, rows: u32, radius: f32, bottom: f32, top: f32, inward: bool| {
                out.grid(columns, rows, |s, t| {
                    let (sin_a, cos_a) = around(s);
                    let n = Vec3::new(cos_a, 0.0, sin_a);
                    let pos = Vec3::new(radius * cos_a, bottom + (top - bottom) * t, radius * sin_a);
                    world((pos, if inward { -n } else { n }))
                });
            };
            let cap = |out: &mut Patches, columns: u32, rows: u32, inner: f32, outer: f32, y: f32, up: bool| {
                out.grid(columns, rows, |s, t| {
                    let (sin_a, cos_a) = around(s);
                    let radius = inner + (outer - inner) * t;
                    world((Vec3::new(radius * cos_a, y, radius * sin_a), Vec3::new(0.0, if up { 1.0 } else { -1.0 }, 0.0)))
                });
            };
            match solid.shape {
                Shape::Box { half } => {
                    let h = [half.x, half.y, half.z];
                    for axis in 0..3 {
                        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
                        for sign in [-1.0f32, 1.0] {
                            out.grid(divisions(2.0 * h[a], step), divisions(2.0 * h[b], step), |s, t| {
                                let mut pos = [0.0; 3];
                                let mut n = [0.0; 3];
                                pos[axis] = sign * h[axis];
                                pos[a] = (2.0 * s - 1.0) * h[a];
                                pos[b] = (2.0 * t - 1.0) * h[b];
                                n[axis] = sign;
                                world((Vec3::new(pos[0], pos[1], pos[2]), Vec3::new(n[0], n[1], n[2])))
                            });
                        }
                    }
                }
                Shape::Cylinder { radius, half_height } => {
                    let columns = divisions(2.0 * PI * radius, step);
                    side(&mut out, columns, divisions(2.0 * half_height, step), radius, -half_height, half_height, false);
                    for (y, up) in [(-half_height, false), (half_height, true)] {
                        cap(&mut out, columns, divisions(radius, step), 0.0, radius, y, up);
                    }
                }
                Shape::Tube { radius, inner_radius, half_height } => {
                    let columns = divisions(2.0 * PI * radius, step);
                    let rows = divisions(2.0 * half_height, step);
                    side(&mut out, columns, rows, radius, -half_height, half_height, false);
                    side(&mut out, columns, rows, inner_radius, -half_height, half_height, true);
                    for (y, up) in [(-half_height, false), (half_height, true)] {
                        cap(&mut out, columns, divisions(radius - inner_radius, step), inner_radius, radius, y, up);
                    }
                }
                Shape::Cone { radius, half_height } => {
                    let height = 2.0 * half_height;
                    let columns = divisions(2.0 * PI * radius, step);
                    out.grid(columns, divisions((radius * radius + height * height).sqrt(), step), |s, t| {
                        let (sin_a, cos_a) = around(s);
                        let ring = radius * (1.0 - t);
                        let mut n = Vec3::new(cos_a * height, radius, sin_a * height);
                        n.normalize();
                        world((Vec3::new(ring * cos_a, -half_height + height * t, ring * sin_a), n))
                    });
                    cap(&mut out, columns, divisions(radius, step), 0.0, radius, -half_height, false);
                }
                Shape::Torus { major_radius, minor_radius } => {
                    let columns = divisions(2.0 * PI * (major_radius + minor_radius), step);
                    out.grid(columns, divisions(2.0 * PI * minor_radius, step), |s, t| {
                        let (sin_a, cos_a) = around(s);
                        let (sin_b, cos_b) = (2.0 * PI * t).sin_cos();
                        let ring = major_radius + minor_radius * cos_b;
                        let n = Vec3::new(cos_b * cos_a, sin_b, cos_b * sin_a);
                        world((Vec3::new(ring * cos_a, minor_radius * sin_b, ring * sin_a), n))
                    });
                }
            }
        }
        Primitive::Triangle(t) => {
            let normals = corner_normals;
            let flat = t.n;
            let corners = [0, 1, 2].map(|c| {
                let n = normals[c];
                let n = if n.x.is_finite() && n.length() > 0.5 { n } else { flat };
                (scene.vertices[t.vertices[c] as usize].pos, n)
            });
            out.triangle(mesh_k, corners);
        }
        Primitive::Ground(_) => {}
    }
    out
}

/// Size of a shape: the longest half extent, which sets the edge length for its detail.
fn half_size(scene: &Scene, p: &Primitive) -> f32 {
    match p {
        Primitive::Sphere(e) => e.radius,
        Primitive::Solid(s) => ((s.high - s.low) * 0.5).array().into_iter().fold(0.0, f32::max),
        Primitive::Triangle(t) => {
            let [a, b, c] = t.vertices.map(|i| scene.vertices[i as usize].pos);
            [(b - a).length(), (c - b).length(), (a - c).length()].into_iter().fold(0.0, f32::max) * 0.5
        }
        Primitive::Ground(_) => 0.0,
    }
}

impl Scene {
    /// Turns every displaced object into its displaced mesh. The ground stays a plane; rays march
    /// over its relief. Does nothing when no material displaces.
    pub fn apply_displacement(&mut self) {
        let displaced = |p: &Primitive, scene: &Scene| {
            !matches!(p, Primitive::Ground(_)) && scene.materials[p.material()].displacement.is_some()
        };
        if !self.prims.iter().any(|p| displaced(p, self)) {
            return;
        }
        // A mesh splits every face the same number of times, so neighbors share their edge
        // points. That count comes from its largest face; its edge length, from its whole size.
        #[derive(Default)]
        struct Mesh { largest: f32, low: Option<(Vec3, Vec3)> }
        let mut meshes: HashMap<usize, Mesh> = HashMap::new();
        for p in self.prims.iter().filter(|p| displaced(p, self)) {
            let Primitive::Triangle(t) = p else { continue };
            let mesh = meshes.entry(p.material()).or_default();
            mesh.largest = mesh.largest.max(half_size(self, p));
            for v in t.vertices {
                let pos = self.vertices[v as usize].pos;
                let (low, high) = mesh.low.unwrap_or((pos, pos));
                mesh.low = Some((
                    Vec3::new(low.x.min(pos.x), low.y.min(pos.y), low.z.min(pos.z)),
                    Vec3::new(high.x.max(pos.x), high.y.max(pos.y), high.z.max(pos.z)),
                ));
            }
        }
        let step_for = |scene: &Scene, p: &Primitive, scale: f32| {
            let d = scene.materials[p.material()].displacement.expect("displaced");
            let size = match p {
                Primitive::Triangle(_) => meshes[&p.material()].low.map_or(1.0, |(low, high)| (high - low).length() * 0.5),
                _ => half_size(scene, p),
            };
            2.0 * size / (d.detail as f32 * scale)
        };
        let mesh_k = |p: &Primitive, step: f32| match p {
            Primitive::Triangle(_) => (2.0 * meshes[&p.material()].largest / step).ceil().clamp(1.0, 512.0) as u32,
            _ => 1,
        };

        // Count at full detail, and lower it evenly when the triangles would not fit.
        let total: usize = self.prims.iter().enumerate().filter(|(_, p)| displaced(p, self)).map(|(i, p)| {
            let step = step_for(self, p, 1.0);
            tessellate(self, p, self.corner_normals[i], step, mesh_k(p, step)).triangles.len()
        }).sum();
        let scale = if total > MAX_TRIANGLES {
            self.warnings.push(format!("Displacement detail lowered to fit {MAX_TRIANGLES} triangles"));
            (MAX_TRIANGLES as f32 / total as f32).sqrt() * 0.95
        } else {
            1.0
        };

        let prims = std::mem::take(&mut self.prims);
        let corner_normals = std::mem::take(&mut self.corner_normals);
        let mut faces = Vec::with_capacity(prims.len());
        for (index, p) in prims.iter().enumerate() {
            if !displaced(p, self) {
                self.prims.push(p.clone());
                self.corner_normals.push(corner_normals[index]);
                faces.push(None);
                continue;
            }
            let step = step_for(self, p, scale);
            let patches = tessellate(self, p, corner_normals[index], step, mesh_k(p, step));
            let source = self.displaced_sources.len() as u32;
            self.add_displaced(p, source, &patches, &mut faces);
            self.displaced_sources.push(p.clone());
        }
        self.displaced = faces;
        self.grounds = self.prims.iter().enumerate()
            .filter_map(|(i, p)| matches!(p, Primitive::Ground(_)).then_some(i))
            .collect();
    }

    /// Adds the displaced mesh of `p`. Points at the same place, along seams and the edges
    /// between patches, become one vertex that moves along the average of their distinct normals,
    /// so the surface does not tear there.
    fn add_displaced(&mut self, p: &Primitive, source: u32, patches: &Patches, faces: &mut Vec<Option<DisplacedFace>>) {
        let d = self.materials[p.material()].displacement.expect("displaced");
        let material = p.material();
        let reach = patches.points.iter().fold(0.0f32, |m, (pos, _)| m.max(pos.x.abs()).max(pos.y.abs()).max(pos.z.abs()));
        let quantum = reach.max(1.0) * 1e-5;
        let key = |v: Vec3| ((v.x / quantum).round() as i64, (v.y / quantum).round() as i64, (v.z / quantum).round() as i64);
        let mut slots: HashMap<(i64, i64, i64), usize> = HashMap::new();
        let mut welded: Vec<(Vec3, Vec<Vec3>)> = Vec::new();
        let index: Vec<usize> = patches.points.iter().map(|&(pos, n)| {
            let slot = *slots.entry(key(pos)).or_insert_with(|| {
                welded.push((pos, Vec::new()));
                welded.len() - 1
            });
            let normals = &mut welded[slot].1;
            if normals.iter().all(|m| m.dot(n) < 0.999) {
                normals.push(n);
            }
            slot
        }).collect();

        let first = self.vertices.len();
        for (pos, normals) in &welded {
            let mut n = normals.iter().fold(Vec3::default(), |sum, &m| sum + m);
            n.normalize();
            let moved = *pos + n * height_at(self, p, &d, *pos);
            let (u, v) = self.texture_uv_on(p, *pos).unwrap_or((0.0, 0.0));
            self.vertices.push(Vertex { pos: moved, normal: n, u, v });
        }
        let mut smooth = vec![Vec3::default(); welded.len()];
        let mut kept = Vec::new();
        for &tri in &patches.triangles {
            let base = tri.map(|i| patches.points[i as usize].0);
            let normals = tri.map(|i| patches.points[i as usize].1);
            // Wind each face like the shape faces, so its normal points out.
            let outward = normals[0] + normals[1] + normals[2];
            let tri = if (base[1] - base[0]).cross(base[2] - base[0]).dot(outward) < 0.0 { [tri[0], tri[2], tri[1]] } else { tri };
            let ids = tri.map(|i| index[i as usize]);
            if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
                continue;
            }
            let corners = ids.map(|i| self.vertices[first + i].pos);
            let face = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
            if !(face.length() > 1e-12) {
                continue;
            }
            for i in ids {
                smooth[i] += face;
            }
            let face = DisplacedFace {
                source,
                base: tri.map(|i| patches.points[i as usize].0),
                normals: tri.map(|i| patches.points[i as usize].1),
            };
            kept.push((ids, face));
        }
        for (ids, face) in kept {
            let t = Triangle::new(&mut self.vertices, ids.map(|i| (first + i) as u32), material);
            let corner = ids.map(|i| {
                let mut n = smooth[i];
                n.normalize();
                if n.x.is_finite() { n } else { t.n }
            });
            self.prims.push(Primitive::Triangle(t));
            self.corner_normals.push(corner);
            faces.push(Some(face));
        }
    }

    /// The displaced-shape record of a primitive, if it is a displaced triangle.
    pub fn displaced_face(&self, prim: usize) -> Option<&DisplacedFace> {
        self.displaced.get(prim).and_then(Option::as_ref)
    }
}

/// Most steps a ray takes across a displaced ground; grazing rays far away step longer.
const MAX_GROUND_STEPS: f32 = 1500.0;

/// Intersection with a displaced ground, from above. Rays from below its base plane see the
/// plain plane, as an undisplaced ground shows them.
pub(crate) fn intersect_ground(scene: &Scene, prim: usize, g: &Ground, d: &Displacement, r: &Ray, dist: &mut f32) -> i32 {
    let p = &scene.prims[prim];
    let altitude0 = (r.o - g.point).dot(g.normal);
    if altitude0 < 0.0 || d.height <= 0.0 {
        return g.intersect(r.o, r.d, dist);
    }
    let climb = r.d.dot(g.normal);
    // The part of the ray inside the slab between the base plane and the highest relief.
    let enter = if altitude0 > d.height {
        if climb >= -1e-7 {
            return 0;
        }
        (d.height - altitude0) / climb
    } else {
        0.0
    };
    let leave = if climb < -1e-7 { -altitude0 / climb } else if climb > 1e-7 { (d.height - altitude0) / climb } else { f32::INFINITY };
    let (start, end) = (enter.max(crate::vector::EPSILON), leave.min(*dist));
    if start >= end {
        return 0;
    }
    // About half a texel along the ground per step.
    let m = &scene.materials[g.material];
    let texel = crate::scene::GROUND_TILE / m.ru_scale.abs().max(m.rv_scale.abs()).max(1e-6)
        / scene.textures[d.texture].width.max(1) as f32;
    let across = (1.0 - climb * climb).max(0.0).sqrt().max(1e-3);
    let step = (0.5 * texel / across).max((end - start) / MAX_GROUND_STEPS);
    let above = |t: f32| {
        let pos = r.o + r.d * t;
        (pos - g.point).dot(g.normal) - height_at(scene, p, d, pos)
    };
    // A ray that starts inside the relief, as one leaving the surface does, must first get out.
    let mut t = start;
    let mut was_above = above(t) > 0.0;
    while t < end {
        let next = (t + step).min(end);
        let now_above = above(next) > 0.0;
        if was_above && !now_above {
            let (mut low, mut high) = (t, next);
            for _ in 0..12 {
                let mid = 0.5 * (low + high);
                if above(mid) > 0.0 { low = mid } else { high = mid }
            }
            if high >= *dist {
                return 0;
            }
            *dist = high;
            return 1;
        }
        was_above = now_above;
        t = next;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> Scene {
        Scene::open(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/displacement.far")))
            .expect("the displacement example opens")
    }

    #[test]
    fn displaced_meshes_are_finite_and_inside_the_scene_bounds() {
        let scene = example();
        let bad = scene.vertices.iter().filter(|v| !v.pos.x.is_finite() || !v.pos.y.is_finite() || !v.pos.z.is_finite()).count();
        assert_eq!(bad, 0, "non-finite vertices");
        let (low, high) = (scene.extends.pos, scene.extends.pos + scene.extends.size);
        assert!(low.x.is_finite() && high.x.is_finite(), "bounds {:?} {:?}", low.array(), high.array());
        assert_eq!(scene.displaced.len(), scene.prims.len());
        assert_eq!(scene.corner_normals.len(), scene.prims.len());
    }

    #[test]
    fn a_ray_down_meets_the_ground_at_the_map_height() {
        let text = br#"{"lights":[],"objects":[{"type":"ground","material":{"color":[0.5,0.5,0.5]}}]}"#;
        let mut scene = Scene::from_far(text, std::path::Path::new("ground.far")).unwrap();
        // A ramp: black on the left of the image, white on the right.
        let pixels = (0..64 * 64).map(|i| { let v = (i % 64) as f32 / 64.0; crate::vector::Color::new(v, v, v) }).collect();
        scene.textures.push(crate::texture::Texture { width: 64, height: 64, image: pixels });
        let d = Displacement { texture: 0, height: 0.5, detail: DEFAULT_DETAIL };
        scene.materials[0].displacement = Some(d);
        let Primitive::Ground(g) = scene.prims[0].clone() else { unreachable!() };
        for column in [8.0f32, 32.0, 56.0] {
            // A point over the given image column, then straight down onto it.
            let spot = g.point + g.u_axis * (crate::scene::GROUND_TILE * (column + 0.5) / 64.0);
            let expected = height_at(&scene, &scene.prims[0], &d, spot);
            let ray = Ray { o: spot + g.normal * 3.0, d: -g.normal };
            let mut dist = 100.0;
            assert_eq!(intersect_ground(&scene, 0, &g, &d, &ray, &mut dist), 1);
            assert!((3.0 - dist - expected).abs() < 0.01, "column {column}: hit at {} for a height of {expected}", 3.0 - dist);
        }
        // From below the base plane, the ground is the plain plane.
        let ray = Ray { o: g.point - g.normal, d: g.normal };
        let mut dist = 100.0;
        assert_ne!(intersect_ground(&scene, 0, &g, &d, &ray, &mut dist), 0);
        assert!((dist - 1.0).abs() < 1e-4);
    }
}

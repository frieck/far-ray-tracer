//! Describes a scene for the real-time view shown while the camera moves. It carries
//! shapes and base colors only: lighting there is an approximation, not a render.

use crate::scene::{Primitive, Scene};
use crate::solid::Shape;
use crate::vector::Vec3;
use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
pub struct Look {
    pub color: [f32; 3],
    pub metalness: f32,
    pub roughness: f32,
    pub opacity: f32,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreviewShape {
    Sphere { radius: f32 },
    Box { size: [f32; 3] },
    Cylinder { radius: f32, height: f32 },
    Tube { radius: f32, inner_radius: f32, height: f32 },
    Cone { radius: f32, height: f32 },
    Torus { major_radius: f32, minor_radius: f32 },
}

#[derive(Serialize)]
pub struct PreviewObject {
    pub shape: PreviewShape,
    pub center: [f32; 3],
    /// World directions of the local X, Y and Z axes.
    pub axes: [[f32; 3]; 3],
    pub look: Look,
}

/// All the triangles of one material, three vertices each.
#[derive(Serialize)]
pub struct PreviewMesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub look: Look,
}

#[derive(Serialize)]
pub struct PreviewLight {
    pub position: [f32; 3],
    pub color: [f32; 3],
    /// Four corners for a rectangular panel, empty for a point or sphere.
    pub corners: Vec<[f32; 3]>,
}

#[derive(Serialize)]
pub struct PreviewScene {
    pub objects: Vec<PreviewObject>,
    pub meshes: Vec<PreviewMesh>,
    pub lights: Vec<PreviewLight>,
}

fn look(scene: &Scene, material: usize) -> Look {
    let m = &scene.materials[material];
    let (metalness, roughness) = match m.ggx {
        Some(ggx) => (ggx.metallic, ggx.roughness),
        None => (m.refl.clamp(0.0, 1.0), (1.0 - m.refl).clamp(0.15, 0.9)),
    };
    let opacity = if m.refr > 0.0 { 0.35 } else { 1.0 };
    Look { color: m.color.array(), metalness, roughness, opacity }
}

fn shape(shape: Shape) -> PreviewShape {
    match shape {
        Shape::Box { half } => PreviewShape::Box { size: (half * 2.0).array() },
        Shape::Cylinder { radius, half_height } => PreviewShape::Cylinder { radius, height: 2.0 * half_height },
        Shape::Tube { radius, inner_radius, half_height } => {
            PreviewShape::Tube { radius, inner_radius, height: 2.0 * half_height }
        }
        Shape::Cone { radius, half_height } => PreviewShape::Cone { radius, height: 2.0 * half_height },
        Shape::Torus { major_radius, minor_radius } => PreviewShape::Torus { major_radius, minor_radius },
    }
}

/// Half the side of the square that stands for an infinite plane in the 3D view.
const GROUND_HALF_SIZE: f32 = 1000.0;

/// Adds triangles to the mesh of `material`, creating it the first time.
fn mesh_of<'a>(scene: &Scene, meshes: &'a mut Vec<(usize, PreviewMesh)>, material: usize) -> &'a mut PreviewMesh {
    let at = match meshes.iter().position(|(m, _)| *m == material) {
        Some(at) => at,
        None => {
            meshes.push((material, PreviewMesh { positions: Vec::new(), normals: Vec::new(), look: look(scene, material) }));
            meshes.len() - 1
        }
    };
    &mut meshes[at].1
}

pub fn describe(scene: &Scene) -> PreviewScene {
    const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut objects = Vec::new();
    let mut meshes: Vec<(usize, PreviewMesh)> = Vec::new();
    for (index, prim) in scene.prims.iter().enumerate() {
        match prim {
            Primitive::Sphere(s) => objects.push(PreviewObject {
                shape: PreviewShape::Sphere { radius: s.radius },
                center: s.center.array(),
                axes: IDENTITY,
                look: look(scene, s.material),
            }),
            Primitive::Solid(s) => objects.push(PreviewObject {
                shape: shape(s.shape),
                center: s.center.array(),
                axes: s.axes().map(Vec3::array),
                look: look(scene, s.material),
            }),
            Primitive::Ground(g) => {
                // A large square centered below the objects stands for the infinite plane.
                let center = scene.center().map_or(g.point, |[x, y, z]| Vec3::new(x, y, z));
                let center = center - g.normal * (center - g.point).dot(g.normal);
                let (u, v) = (g.u_axis * GROUND_HALF_SIZE, g.v_axis * GROUND_HALF_SIZE);
                let [a, b, c, d] = [center - u - v, center - u + v, center + u + v, center + u - v];
                let mesh = mesh_of(scene, &mut meshes, g.material);
                for p in [a, b, c, a, c, d] {
                    mesh.positions.extend(p.array());
                    mesh.normals.extend(g.normal.array());
                }
            }
            Primitive::Triangle(t) => {
                let mesh = mesh_of(scene, &mut meshes, t.material);
                for (corner, vertex) in t.vertices.iter().enumerate() {
                    mesh.positions.extend(scene.vertices[*vertex as usize].pos.array());
                    mesh.normals.extend(scene.corner_normals[index][corner].array());
                }
            }
        }
    }
    let mut lights: Vec<PreviewLight> = scene.lights.iter().map(|light| PreviewLight {
        position: light.center.array(),
        color: scene.materials[light.material].color.array(),
        corners: Vec::new(),
    }).collect();
    lights.extend(scene.rect_lights.iter().map(|light| PreviewLight {
        position: light.proxy.center.array(),
        color: scene.materials[light.proxy.material].color.array(),
        corners: [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(u, v)| light.point(u, v).array()).to_vec(),
    }));
    lights.extend(scene.bulbs.iter().map(|light| PreviewLight {
        position: light.center.array(),
        color: scene.materials[light.material].color.array(),
        corners: Vec::new(),
    }));
    PreviewScene { objects, meshes: meshes.into_iter().map(|(_, mesh)| mesh).collect(), lights }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn assets(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes").join(name)
    }

    #[test]
    fn every_primitive_appears_once() {
        let scene = Scene::open(&assets("primitives.far")).unwrap();
        let preview = describe(&scene);
        let triangles: usize = preview.meshes.iter().map(|m| m.positions.len() / 9).sum();
        assert_eq!(preview.objects.len() + triangles, scene.prims.len());
        assert_eq!(preview.lights.len(), scene.lights.len());
    }

    #[test]
    fn mesh_data_is_complete_and_finite() {
        let scene = Scene::open(&assets("rhino.far")).unwrap();
        let preview = describe(&scene);
        assert!(!preview.meshes.is_empty());
        for mesh in &preview.meshes {
            assert_eq!(mesh.positions.len(), mesh.normals.len());
            assert_eq!(mesh.positions.len() % 9, 0);
            assert!(mesh.positions.iter().chain(&mesh.normals).all(|v| v.is_finite()));
        }
    }

    #[test]
    fn panels_keep_their_corners() {
        let scene = Scene::open(&assets("rectangular-lights.far")).unwrap();
        let preview = describe(&scene);
        assert_eq!(preview.lights.iter().filter(|l| l.corners.len() == 4).count(), 2);
    }

    #[test]
    fn glass_is_see_through_and_ggx_keeps_its_values() {
        let text = br#"{"lights":[],"objects":[
            {"type":"sphere","center":[0,0,5],"radius":1,"material":{"refraction":1}},
            {"type":"sphere","center":[3,0,5],"radius":1,"material":{"surface":{"preset":"metal","roughness":0.3}}}]}"#;
        let preview = describe(&Scene::from_far(text, Path::new("test.far")).unwrap());
        assert!(preview.objects[0].look.opacity < 1.0);
        assert_eq!((preview.objects[1].look.metalness, preview.objects[1].look.roughness), (1.0, 0.3));
    }
}

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

    /// A scene by its file name, wherever it sits under `assets/scenes`: the examples are sorted
    /// into folders, and a test should not break when they move.
    fn assets(name: &str) -> std::path::PathBuf {
        fn find(folder: &std::path::Path, file: &str) -> Option<std::path::PathBuf> {
            let mut entries: Vec<_> = std::fs::read_dir(folder).ok()?.flatten().map(|e| e.path()).collect();
            entries.sort();
            entries.iter().find(|p| p.file_name().is_some_and(|n| n == file)).cloned()
                .or_else(|| entries.iter().filter(|p| p.is_dir()).find_map(|p| find(p, file)))
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes");
        let file = name.rsplit('/').next().unwrap_or(name);
        find(&root, file).unwrap_or_else(|| root.join(name))
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

/// Where the material ball is seen from unless a pose is given.
pub const MATERIAL_BALL_POSE: crate::camera::Pose =
    crate::camera::Pose { position: [0.9, 2.35, -4.6], target: [0.0, 1.25, 0.0], zoom: 2.1 };

/// Most displacement detail on the material ball.
const MAX_PREVIEW_DETAIL: u32 = 48;

/// A material ball with this many primitives, a displaced one, needs a KD-Tree to render quickly.
pub const BALL_TREE_FROM: usize = 64;

/// Marks the parts of the material ball that take the chosen material.
const CHOSEN: [f32; 3] = [1.0, 0.0, 1.0];

/// A shader ball: the chosen material, with an optional edit, on a ball with a front medallion,
/// standing on a pedestal over a measuring grid, under a soft key light and `environment`.
/// Framed for a square image. `data` is a scene folder whose data holds `preview-grid.png`;
/// without the grid the floor is plain.
pub fn material_ball(
    source: &Scene,
    material: usize,
    edit: Option<&crate::surface::MaterialEdit>,
    environment: Option<crate::environment::Environment>,
    data: &std::path::Path,
) -> Result<Scene, String> {
    use serde_json::json;
    let chosen_material = source.materials.get(material).ok_or("Unknown object material")?;
    let chosen = json!({ "color": CHOSEN });
    let core = json!({ "color": [0.5, 0.5, 0.49], "surface": { "preset": "plastic", "roughness": 0.35 } });
    let band = json!({ "color": [0.05, 0.05, 0.055], "surface": { "preset": "plastic", "roughness": 0.25 } });
    let block = json!({ "color": [0.62, 0.62, 0.6], "surface": { "preset": "plastic", "roughness": 0.45 } });
    let grid = data.join("../scene-data/images/preview-grid.png");
    let floor = if crate::scene::find_data_folder(&data.join("ball.far")).is_some() && grid.is_file() {
        json!({ "color": [0.9, 0.9, 0.9], "texture": "preview-grid.png", "scale_u": 0.3, "scale_v": 0.3,
                "surface": { "preset": "plastic", "roughness": 0.6 } })
    } else {
        json!({ "color": [0.6, 0.6, 0.6], "surface": { "preset": "plastic", "roughness": 0.6 } })
    };
    // The medallion faces the camera, turned up and to the right. A solid's Y axis turns by X,
    // then by Y, so these two angles carry it onto `facing`.
    let mut facing = crate::vector::Vec3::new(0.42, 0.42, -0.8);
    facing.normalize();
    let (center, turn_x): ([f32; 3], f32) = ([0.0, 1.62, 0.0], -facing.y.acos());
    let turn_y = (facing.x / turn_x.sin()).atan2(facing.z / turn_x.sin());
    let medallion = [turn_x.to_degrees(), turn_y.to_degrees(), 0.0];
    let along = |distance: f32| [center[0] + facing.x * distance, center[1] + facing.y * distance, center[2] + facing.z * distance];
    let text = json!({
        "camera": {
            "position": MATERIAL_BALL_POSE.position, "target": MATERIAL_BALL_POSE.target, "zoom": MATERIAL_BALL_POSE.zoom
        },
        "lights": [{
            // Above and behind the camera, so it lights the ball without showing in the frame.
            "type": "rectangle", "position": [-3.6, 5.0, -5.0],
            "edge_u": [1.8, 0, 0], "edge_v": [0, 0, 1.8], "intensity": 30
        }],
        "objects": [
            { "type": "ground", "material": floor },
            { "type": "box", "center": [0, 0.08, 0], "size": [1.9, 0.16, 1.9], "rotation": [0, 18, 0], "material": block },
            { "type": "cylinder", "center": [0, 0.22, 0], "radius": 0.72, "height": 0.12, "material": chosen },
            { "type": "torus", "center": [0, 0.22, 0], "major_radius": 0.72, "minor_radius": 0.07, "material": band },
            { "type": "cone", "center": [0, 0.45, 0], "radius": 0.62, "height": 0.36, "material": chosen },
            { "type": "cylinder", "center": [0, 0.75, 0], "radius": 0.13, "height": 0.5, "material": chosen },
            { "type": "sphere", "center": center, "radius": 1.0, "material": chosen },
            { "type": "cylinder", "center": along(0.97), "radius": 0.5, "height": 0.12,
              "rotation": medallion, "material": core },
            { "type": "torus", "center": along(0.95), "major_radius": 0.54, "minor_radius": 0.06,
              "rotation": medallion, "material": chosen }
        ]
    });
    let mut ball = Scene::from_far(text.to_string().as_bytes(), &data.join("material-ball.far"))?;
    let mut edited = chosen_material.clone();
    if let Some(edit) = edit {
        edit.apply(&mut edited)?;
    }
    // The chosen material's texture indices point into the source's textures, appended here.
    let offset = ball.textures.len();
    if let Some(texture) = edited.texture.as_mut() {
        *texture += offset;
    }
    if let Some(bump) = edited.bump.as_mut() {
        bump.texture += offset;
    }
    // A preview tessellates on every edit, so displacement keeps a modest detail here.
    if let Some(d) = edited.displacement.as_mut() {
        d.texture += offset;
        d.detail = d.detail.min(MAX_PREVIEW_DETAIL);
    }
    ball.textures.extend(source.textures.iter().cloned());
    for m in ball.materials.iter_mut().filter(|m| m.color.array() == CHOSEN) {
        *m = edited.clone();
    }
    ball.apply_displacement();
    ball.fit_bounds();
    ball.environment = environment;
    Ok(ball)
}

#[cfg(test)]
mod ball_tests {
    use super::*;
    use crate::engine::{render, Image, Mode, Options, Progress};

    #[test]
    fn the_material_ball_shows_the_chosen_material() {
        let source = Scene::open(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/cornell-box.far")))
            .expect("cornell box");
        let red = source.materials.iter().position(|m| m.color.x > 0.5 && m.color.y < 0.2).expect("a red wall");
        let data = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes"));
        let ball = material_ball(&source, red, None, None, data).unwrap();
        let op = Options { mode: Mode::Enhanced, width: 48, height: 48, samples: 8, ..Options::default() };
        let image = Image::new(op.width, op.height);
        render(&ball, None, &op, &image, &Progress::default());
        let rgba = image.rgba();
        let red_pixels = rgba.chunks_exact(4).filter(|p| p[0] > 40 && p[0] > 2 * p[1] && p[0] > 2 * p[2]).count();
        assert!(red_pixels > 48 * 48 / 10, "the ball is lit and red: {red_pixels} red pixels");
        assert!(material_ball(&source, source.materials.len(), None, None, data).is_err());
    }
}

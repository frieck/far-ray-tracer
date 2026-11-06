//! Loads `.far` scenes, the JSON format that can hold every primitive.
//!
//! Meshes go first and planes second, as in `.frt` scenes, because corner normals are
//! stored in the same order as the primitives.

use crate::scene::{find_data_folder, MaterialData, Primitive, Scene, Sphere, Triangle, Vertex};
use crate::solid::{Shape, Solid};
use crate::vector::{Aabb, Vec3};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

type Point = [f32; 3];

fn vector(p: Point) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn white() -> Point {
    [1.0; 3]
}

fn default_light_radius() -> f32 {
    0.2
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneFile {
    lights: Vec<LightFile>,
    objects: Vec<ObjectFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LightFile {
    position: Point,
    #[serde(default = "default_light_radius")]
    radius: f32,
    #[serde(default = "white")]
    color: Point,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MaterialFile {
    color: Point,
    diffuse: f32,
    specular: f32,
    reflection: f32,
    refraction: f32,
    ior: f32,
    glossy: f32,
    texture: String,
    scale_u: f32,
    scale_v: f32,
}

impl Default for MaterialFile {
    fn default() -> Self {
        Self {
            color: white(),
            diffuse: 1.0,
            specular: 1.0,
            reflection: 0.0,
            refraction: 0.0,
            ior: 1.5,
            glossy: 0.0,
            texture: String::new(),
            scale_u: 1.0,
            scale_v: 1.0,
        }
    }
}

impl From<MaterialFile> for MaterialData {
    fn from(m: MaterialFile) -> Self {
        Self {
            color: vector(m.color),
            refl: m.reflection,
            diff: m.diffuse,
            spec: m.specular,
            refr: m.refraction,
            ior: m.ior,
            drefl: m.glossy,
            u_scale: m.scale_u,
            v_scale: m.scale_v,
            texture: m.texture,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ObjectFile {
    Sphere {
        center: Point,
        radius: f32,
        #[serde(default)]
        material: MaterialFile,
    },
    Box {
        center: Point,
        size: Point,
        #[serde(default)]
        rotation: Point,
        #[serde(default)]
        material: MaterialFile,
    },
    Cylinder {
        center: Point,
        radius: f32,
        height: f32,
        #[serde(default)]
        rotation: Point,
        #[serde(default)]
        material: MaterialFile,
    },
    Tube {
        center: Point,
        radius: f32,
        inner_radius: f32,
        height: f32,
        #[serde(default)]
        rotation: Point,
        #[serde(default)]
        material: MaterialFile,
    },
    Cone {
        center: Point,
        radius: f32,
        height: f32,
        #[serde(default)]
        rotation: Point,
        #[serde(default)]
        material: MaterialFile,
    },
    Torus {
        center: Point,
        major_radius: f32,
        minor_radius: f32,
        #[serde(default)]
        rotation: Point,
        #[serde(default)]
        material: MaterialFile,
    },
    Plane {
        points: [Point; 4],
        #[serde(default)]
        material: MaterialFile,
    },
    Mesh {
        file: String,
        position: Point,
        size: f32,
        #[serde(default)]
        material: MaterialFile,
    },
}

fn require_positive(values: &[f32], what: &str) -> Result<(), String> {
    if values.iter().all(|v| *v > 0.0) {
        Ok(())
    } else {
        Err(format!("{what} must have positive sizes"))
    }
}

impl Scene {
    pub(crate) fn from_far(data: &[u8], path: &Path) -> Result<Self, String> {
        let file: SceneFile =
            serde_json::from_slice(data).map_err(|e| format!("{}: {e}", path.display()))?;
        let dir = find_data_folder(path);
        let mut scene = Scene::empty();
        let mut cache = HashMap::new();

        let (mut meshes, mut planes, mut others) = (Vec::new(), Vec::new(), Vec::new());
        for object in file.objects {
            match object {
                ObjectFile::Mesh { .. } => meshes.push(object),
                ObjectFile::Plane { .. } => planes.push(object),
                _ => others.push(object),
            }
        }

        for object in meshes {
            let ObjectFile::Mesh { file, position, size, material } = object else { continue };
            let material = scene.add_material(material.into(), dir.as_deref(), &mut cache);
            scene.add_mesh(&file, dir.as_deref(), material, vector(position), size);
        }
        for object in planes {
            let ObjectFile::Plane { points, material } = object else { continue };
            let material = scene.add_material(material.into(), dir.as_deref(), &mut cache);
            let base = scene.vertices.len() as u32;
            scene.vertices.extend(points.map(|p| Vertex::new(vector(p))));
            for ids in [[base, base + 1, base + 3], [base + 1, base + 2, base + 3]] {
                let t = Triangle::new(&mut scene.vertices, ids, material);
                scene.corner_normals.push([t.n; 3]);
                scene.prims.push(Primitive::Triangle(t));
            }
        }
        for object in others {
            scene.add_analytic(object, dir.as_deref(), &mut cache)?;
        }
        for light in file.lights {
            let material = MaterialFile { color: light.color, ..MaterialFile::default() };
            let material = scene.add_material(material.into(), dir.as_deref(), &mut cache);
            scene.lights.push(Sphere::new(vector(light.position), light.radius, material));
        }
        scene.fit_bounds();
        Ok(scene)
    }

    fn add_analytic(
        &mut self,
        object: ObjectFile,
        dir: Option<&Path>,
        cache: &mut HashMap<String, Option<usize>>,
    ) -> Result<(), String> {
        let (center, rotation, shape, material) = match object {
            ObjectFile::Sphere { center, radius, material } => {
                require_positive(&[radius], "A sphere")?;
                let material = self.add_material(material.into(), dir, cache);
                self.corner_normals.push([Vec3::default(); 3]);
                self.prims.push(Primitive::Sphere(Sphere::new(vector(center), radius, material)));
                return Ok(());
            }
            ObjectFile::Box { center, size, rotation, material } => {
                require_positive(&size, "A box")?;
                (center, rotation, Shape::Box { half: vector(size) * 0.5 }, material)
            }
            ObjectFile::Cylinder { center, radius, height, rotation, material } => {
                require_positive(&[radius, height], "A cylinder")?;
                (center, rotation, Shape::Cylinder { radius, half_height: 0.5 * height }, material)
            }
            ObjectFile::Tube { center, radius, inner_radius, height, rotation, material } => {
                require_positive(&[radius, inner_radius, height], "A tube")?;
                if inner_radius >= radius {
                    return Err("A tube needs inner_radius below radius".into());
                }
                let half_height = 0.5 * height;
                (center, rotation, Shape::Tube { radius, inner_radius, half_height }, material)
            }
            ObjectFile::Cone { center, radius, height, rotation, material } => {
                require_positive(&[radius, height], "A cone")?;
                (center, rotation, Shape::Cone { radius, half_height: 0.5 * height }, material)
            }
            ObjectFile::Torus { center, major_radius, minor_radius, rotation, material } => {
                require_positive(&[major_radius, minor_radius], "A torus")?;
                (center, rotation, Shape::Torus { major_radius, minor_radius }, material)
            }
            ObjectFile::Plane { .. } | ObjectFile::Mesh { .. } => return Ok(()),
        };
        let material = self.add_material(material.into(), dir, cache);
        self.corner_normals.push([Vec3::default(); 3]);
        self.prims.push(Primitive::Solid(Solid::new(shape, vector(center), rotation, material)));
        Ok(())
    }

    /// Grows the KD-Tree bounds to hold the whole scene. `.frt` scenes keep the fixed
    /// bounds of the original, where geometry outside them disappears.
    fn fit_bounds(&mut self) {
        let (mut low, mut high) = (self.extends.pos, self.extends.pos + self.extends.size);
        let mut grow = |a: Vec3, b: Vec3| {
            low = Vec3::new(low.x.min(a.x), low.y.min(a.y), low.z.min(a.z));
            high = Vec3::new(high.x.max(b.x), high.y.max(b.y), high.z.max(b.z));
        };
        for prim in &self.prims {
            match prim {
                Primitive::Sphere(s) => {
                    let r = Vec3::new(s.radius, s.radius, s.radius);
                    grow(s.center - r, s.center + r);
                }
                Primitive::Solid(s) => grow(s.low, s.high),
                Primitive::Triangle(t) => {
                    for v in t.vertices {
                        let p = self.vertices[v as usize].pos;
                        grow(p, p);
                    }
                }
            }
        }
        let margin = Vec3::new(0.01, 0.01, 0.01);
        self.extends = Aabb::new(low - margin, high - low + margin * 2.0);
    }
}

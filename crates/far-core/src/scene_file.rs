//! Loads `.far` scenes, the JSON format that can hold every primitive.
//!
//! Meshes go first and planes second, as in the original engine, because corner normals are
//! stored in the same order as the primitives.

use crate::scene::{find_data_folder, Ground, MaterialData, Primitive, RectLight, Scene, Sphere, Triangle, Vertex};
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
    #[serde(default)]
    environment: Option<crate::environment::EnvironmentConfig>,
    #[serde(default)]
    camera: crate::camera::Camera,
    #[serde(default)]
    bounds: Option<[Point; 2]>,
    lights: Vec<LightFile>,
    objects: Vec<ObjectFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphereLightFile {
    #[serde(default)]
    material: Option<MaterialFile>,
    position: Point,
    #[serde(default = "default_light_radius")]
    radius: f32,
    #[serde(default = "white")]
    color: Point,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LightFile {
    Sphere(SphereLightFile),
    Area(AreaLightFile),
}

fn default_intensity() -> f32 { 1.0 }

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum AreaLightFile {
    Rectangle {
        position: Point,
        edge_u: Point,
        edge_v: Point,
        #[serde(default = "white")]
        color: Point,
        #[serde(default = "default_intensity")]
        intensity: f32,
    },
    Sphere {
        position: Point,
        radius: f32,
        #[serde(default = "white")]
        color: Point,
        #[serde(default = "default_intensity")]
        intensity: f32,
    },
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MaterialFile {
    surface: Option<crate::surface::Surface>,
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
            surface: None,
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
            surface: m.surface,
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
    /// An infinite plane through `position`, facing `normal`.
    Ground {
        #[serde(default)]
        position: Point,
        #[serde(default = "up")]
        normal: Point,
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

fn up() -> Point {
    [0.0, 1.0, 0.0]
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
        file.camera.validate()?;
        let mut scene = Scene::empty();
        scene.camera = file.camera;
        if let Some(mut config) = file.environment {
            let map_path = path.parent().unwrap_or(Path::new(".")).join(&config.path);
            config.path = map_path.to_string_lossy().into_owned();
            scene.environment = Some(crate::environment::Environment::load(config)?);
        }
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
            match light {
                LightFile::Sphere(light) => {
                    if light.radius < 0.0 { return Err("A light radius must be nonnegative".into()); }
                    let material = light.material.unwrap_or(MaterialFile { color: light.color, ..MaterialFile::default() });
                    let material = scene.add_material(material.into(), dir.as_deref(), &mut cache);
                    scene.lights.push(Sphere::new(vector(light.position), light.radius, material));
                }
                LightFile::Area(AreaLightFile::Rectangle { position, edge_u, edge_v, color, intensity }) => {
                    let (u, v) = (vector(edge_u), vector(edge_v));
                    let (width, height) = (u.length(), v.length());
                    require_positive(&[width, height], "A rectangular light")?;
                    if !width.is_finite() || !height.is_finite() || u.dot(v).abs() > 1e-5 * width * height {
                        return Err("A rectangular light needs finite, perpendicular edge_u and edge_v vectors".into());
                    }
                    if !intensity.is_finite() || intensity < 0.0 || color.iter().any(|c| *c < 0.0) {
                        return Err("Light intensity and color must be nonnegative".into());
                    }
                    let material = MaterialFile { color: (vector(color) * intensity).array(), ..MaterialFile::default() };
                    let material = scene.add_material(material.into(), dir.as_deref(), &mut cache);
                    let mut normal = u.cross(v);
                    normal.normalize();
                    scene.rect_lights.push(RectLight {
                        proxy: Sphere::new(vector(position), 0.5 * (u + v).length(), material),
                        edge_u: u, edge_v: v, normal, area: width * height,
                    });
                }
                LightFile::Area(AreaLightFile::Sphere { position, radius, color, intensity }) => {
                    require_positive(&[radius], "A spherical light")?;
                    if !radius.is_finite() || !intensity.is_finite() || intensity < 0.0 || color.iter().any(|c| *c < 0.0) {
                        return Err("Light intensity and color must be nonnegative".into());
                    }
                    let material = MaterialFile { color: (vector(color) * intensity).array(), ..MaterialFile::default() };
                    let material = scene.add_material(material.into(), dir.as_deref(), &mut cache);
                    scene.bulbs.push(Sphere::new(vector(position), radius, material));
                }
            }
        }
        if let Some([low, high]) = file.bounds {
            let (low, high) = (vector(low), vector(high));
            require_positive(&(high - low).array(), "Scene bounds")?;
            scene.extends = Aabb::new(low, high - low);
        } else {
            scene.fit_bounds();
        }
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
            ObjectFile::Ground { position, normal, material } => {
                let normal = vector(normal);
                let length = normal.length();
                if !(length.is_finite() && length > 0.0) || position.iter().any(|v| !v.is_finite()) {
                    return Err("A ground needs a finite position and a nonzero normal".into());
                }
                let material = self.add_material(material.into(), dir, cache);
                self.corner_normals.push([Vec3::default(); 3]);
                self.grounds.push(self.prims.len());
                self.prims.push(Primitive::Ground(Ground::new(vector(position), normal, material)));
                return Ok(());
            }
            ObjectFile::Plane { .. } | ObjectFile::Mesh { .. } => return Ok(()),
        };
        let material = self.add_material(material.into(), dir, cache);
        self.corner_normals.push([Vec3::default(); 3]);
        self.prims.push(Primitive::Solid(Solid::new(shape, vector(center), rotation, material)));
        Ok(())
    }

    /// Grows the KD-Tree bounds to hold the whole scene. Explicit bounds preserve the
    /// historical volume when requested by a reference scene.
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
                // Tested apart from the KD-Tree, so it does not need to fit in it.
                Primitive::Ground(_) => {}
            }
        }
        let margin = Vec3::new(0.01, 0.01, 0.01);
        self.extends = Aabb::new(low - margin, high - low + margin * 2.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_bounds_and_point_light_material_survive_json_loading() {
        let text = br#"{"bounds":[[-14,-6,-6],[14,8,30]],"lights":[{"position":[0,6,2],"radius":0,"material":{"color":[0.25,0.5,1],"scale_u":2,"scale_v":3}}],"objects":[]}"#;
        let scene = Scene::from_far(text, Path::new("test.far")).unwrap();
        assert_eq!(scene.extends.pos, Vec3::new(-14.0, -6.0, -6.0));
        assert_eq!(scene.extends.size, Vec3::new(28.0, 14.0, 36.0));
        assert_eq!(scene.lights[0].radius, 0.0);
        assert_eq!(scene.materials[scene.lights[0].material].color, Vec3::new(0.25, 0.5, 1.0));
        let invalid = br#"{"bounds":[[1,0,0],[0,1,1]],"lights":[],"objects":[]}"#;
        assert!(Scene::from_far(invalid, Path::new("test.far")).is_err());
    }

    #[test]
    fn scene_files_can_place_the_camera() {
        let text = br#"{"camera":{"position":[1,2,3],"target":[0,0,9],"zoom":2},"lights":[],"objects":[]}"#;
        let scene = Scene::from_far(text, Path::new("test.far")).unwrap();
        assert_eq!(scene.camera.pose(), crate::Pose { position: [1., 2., 3.], target: [0., 0., 9.], zoom: 2. });
        let same = br#"{"camera":{"position":[1,2,3],"target":[1,2,3]},"lights":[],"objects":[]}"#;
        assert!(Scene::from_far(same, Path::new("test.far")).is_err());
    }

    #[test]
    fn reads_spherical_lights_and_rejects_invalid_ones() {
        let text = br#"{"lights":[{"type":"sphere","position":[0,2,3],"radius":0.1,"color":[1,0.5,0.25],"intensity":4}],"objects":[]}"#;
        let scene = Scene::from_far(text, Path::new("test.far")).unwrap();
        assert_eq!(scene.bulbs.len(), 1);
        assert!(scene.lights.is_empty());
        assert_eq!(scene.materials[scene.bulbs[0].material].color, Vec3::new(4.0, 2.0, 1.0));
        for (radius, intensity) in [(0.0, 1.0), (-1.0, 1.0), (0.1, -1.0)] {
            let text = serde_json::json!({"lights":[{"type":"sphere","position":[0,2,3],"radius":radius,"intensity":intensity}],"objects":[]});
            assert!(Scene::from_far(text.to_string().as_bytes(), Path::new("test.far")).is_err());
        }
    }

    #[test]
    fn reads_the_ground_and_rejects_a_zero_normal() {
        let text = br#"{"lights":[],"objects":[{"type":"ground","position":[0,-2,0],"normal":[0,0,-3]}]}"#;
        let scene = Scene::from_far(text, Path::new("test.far")).unwrap();
        let Primitive::Ground(g) = &scene.prims[0] else { panic!("expected a ground") };
        assert_eq!(g.normal, Vec3::new(0.0, 0.0, -1.0));
        assert!(g.u_axis.dot(g.normal).abs() < 1e-6 && g.v_axis.dot(g.normal).abs() < 1e-6);
        assert!((g.u_axis.cross(g.v_axis) + g.normal).length() < 1e-6);
        let text = br#"{"lights":[],"objects":[{"type":"ground"}]}"#;
        let scene = Scene::from_far(text, Path::new("test.far")).unwrap();
        let Primitive::Ground(g) = &scene.prims[0] else { panic!("expected a ground") };
        assert_eq!(g.normal, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(scene.center(), None);
        let text = br#"{"lights":[],"objects":[{"type":"ground","normal":[0,0,0]}]}"#;
        assert!(Scene::from_far(text, Path::new("test.far")).is_err());
    }

    #[test]
    fn rejects_degenerate_or_skewed_rectangular_lights() {
        for (u, v, intensity) in [([0.,0.,0.], [0.,1.,0.], 1.), ([1.,0.,0.], [1.,1.,0.], 1.), ([1.,0.,0.], [0.,1.,0.], -1.)] {
            let text = serde_json::json!({"lights":[{"type":"rectangle","position":[0,2,3],"edge_u":u,"edge_v":v,"intensity":intensity}],"objects":[]});
            assert!(Scene::from_far(text.to_string().as_bytes(), Path::new("test.far")).is_err());
        }
    }
}

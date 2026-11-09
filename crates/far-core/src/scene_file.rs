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
    /// A medium that fills the scene's bounds.
    #[serde(default)]
    fog: Option<MediumFile>,
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
    bump: Option<BumpFile>,
    displacement: Option<DisplacementFile>,
    roughness_map: String,
    metallic_map: String,
    emission: Option<EmissionFile>,
    alpha_map: String,
}

/// What a volume is made of; see `volume::Volume`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MediumFile {
    /// Only for `fog`: the box it fills, which defaults to the box around the objects.
    #[serde(default)]
    bounds: Option<[Point; 2]>,
    density: f32,
    #[serde(default = "white")]
    color: Point,
    #[serde(default)]
    anisotropy: f32,
}

impl MediumFile {
    fn volume(&self, shape: crate::volume::Shape) -> Result<crate::volume::Volume, String> {
        if !(self.density.is_finite() && self.density >= 0.0) || self.color.iter().any(|c| !(0.0..=1.0).contains(c))
            || !(-0.95..=0.95).contains(&self.anisotropy) {
            return Err("A volume needs a density of zero or more, a color in [0, 1] and an anisotropy from -0.95 to 0.95".into());
        }
        Ok(crate::volume::Volume { shape, density: self.density, albedo: vector(self.color), anisotropy: self.anisotropy })
    }
}

/// Light a surface gives off: `{"color": [1, 0.6, 0.2], "strength": 4, "map": "neon.png"}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmissionFile {
    #[serde(default = "white")]
    color: Point,
    #[serde(default = "one")]
    strength: f32,
    #[serde(default)]
    map: String,
}

/// A displacement map: `{"map": "rocks.png", "height": 0.1, "detail": 128}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplacementFile {
    map: String,
    height: f32,
    #[serde(default = "default_detail")]
    detail: u32,
}

fn default_detail() -> u32 {
    crate::displace::DEFAULT_DETAIL
}

/// A bump or normal map: `{"map": "bricks.png", "kind": "height", "strength": 0.02}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BumpFile {
    map: String,
    #[serde(default)]
    kind: crate::bump::BumpKind,
    #[serde(default = "one")]
    strength: f32,
}

fn one() -> f32 {
    1.0
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
            bump: None,
            displacement: None,
            roughness_map: String::new(),
            metallic_map: String::new(),
            emission: None,
            alpha_map: String::new(),
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
            bump: m.bump.map(|b| (b.map, b.kind, b.strength)),
            displacement: m.displacement.map(|d| (d.map, d.height, d.detail)),
            roughness_map: m.roughness_map,
            metallic_map: m.metallic_map,
            emission: m.emission.map(|e| (vector(e.color) * e.strength, e.map)),
            alpha_map: m.alpha_map,
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
    /// Fog or smoke in a box (`size`) or a sphere (`radius`); Enhanced mode only.
    Volume {
        center: Point,
        #[serde(default)]
        size: Option<Point>,
        #[serde(default)]
        radius: Option<f32>,
        #[serde(default)]
        rotation: Point,
        density: f32,
        #[serde(default = "white")]
        color: Point,
        #[serde(default)]
        anisotropy: f32,
    },
    /// An OBJ or glTF model; see `model::Placement`. Without `material` it keeps its own.
    Model {
        file: String,
        #[serde(default)]
        position: Point,
        #[serde(default)]
        rotation: Point,
        #[serde(default = "one")]
        scale: f32,
        #[serde(default)]
        size: Option<f32>,
        #[serde(default)]
        material: Option<MaterialFile>,
    },
}

/// A model file next to the scene, or in the data folder's meshes folder.
fn model_path(scene: &Path, data: Option<&Path>, file: &str) -> Option<std::path::PathBuf> {
    let file = file.replace('\\', "/");
    let beside = scene.parent().unwrap_or(Path::new(".")).join(&file);
    if beside.is_file() {
        return Some(beside);
    }
    let name = Path::new(&file).file_name()?;
    data.map(|d| d.join("meshes").join(name)).filter(|p| p.is_file())
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
        Self::from_file(file, path, None)
    }

    /// A model opened on its own, in a small studio: resting on a gray floor, lit by a key, a
    /// fill and a rim light, and framed by the camera, all scaled to the model.
    pub(crate) fn from_model(path: &Path) -> Result<Self, String> {
        let model = crate::model::load(path)?;
        let points: Vec<Vec3> = model.vertices.iter().map(|v| v.pos).collect();
        let (min, max) = crate::model::bounds(&points);
        let extent = max - min;
        let side = extent.x.max(extent.y).max(extent.z);
        if !(side > 0.0 && side.is_finite()) {
            return Err(format!("{}: the model has no size", path.display()));
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let target = Vec3::new(0.0, 0.45 * extent.y, 0.0);
        let mut view = Vec3::new(0.55, 0.38, -1.0);
        view.normalize();
        let camera = target + view * (1.9 * side);
        // A light at `offset` from the target, `size` wide, facing it.
        let light = |offset: Vec3, size: f32, intensity: f32| {
            let center = target + offset * side;
            let mut facing = target - center;
            facing.normalize();
            let mut u = Vec3::new(0.0, 1.0, 0.0).cross(facing);
            u.normalize();
            let v = facing.cross(u);
            serde_json::json!({"type": "rectangle", "position": center.array(), "edge_u": (u * size * side).array(),
                "edge_v": (v * size * side).array(), "color": [1.0, 0.95, 0.88], "intensity": intensity})
        };
        let json = serde_json::json!({
            "camera": {"position": camera.array(), "target": target.array(), "zoom": 1.0},
            "lights": [
                light(Vec3::new(-1.4, 1.6, -1.2), 0.8, 10.0),
                light(Vec3::new(1.7, 0.7, -1.0), 1.0, 2.5),
                light(Vec3::new(0.6, 3.2, 1.6), 0.9, 7.0),
            ],
            "objects": [
                {"type": "ground", "position": [0.0, 0.0, 0.0],
                 "material": {"color": [0.6, 0.6, 0.6], "surface": {"preset": "plastic", "roughness": 0.55}}},
                {"type": "model", "file": name, "size": side},
            ],
        });
        let file: SceneFile = serde_json::from_value(json).map_err(|e| e.to_string())?;
        Self::from_file(file, path, Some(model))
    }

    fn from_file(file: SceneFile, path: &Path, mut preloaded: Option<crate::model::Model>) -> Result<Self, String> {
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
                ObjectFile::Mesh { .. } | ObjectFile::Model { .. } => meshes.push(object),
                ObjectFile::Plane { .. } => planes.push(object),
                _ => others.push(object),
            }
        }

        for object in meshes {
            match object {
                ObjectFile::Mesh { file, position, size, material } => {
                    let material = scene.add_material(material.into(), dir.as_deref(), &mut cache);
                    scene.add_mesh(&file, dir.as_deref(), material, vector(position), size);
                }
                ObjectFile::Model { file, position, rotation, scale, size, material } => {
                    let valid = scale.is_finite() && scale > 0.0 && size.is_none_or(|s| s.is_finite() && s > 0.0)
                        && rotation.iter().chain(&position).all(|v| v.is_finite());
                    if !valid {
                        return Err(format!("Model {file}: scale and size must be positive, and position and rotation finite"));
                    }
                    let found = model_path(path, dir.as_deref(), &file);
                    let model = match preloaded.take().map(Ok).or_else(|| found.as_deref().map(crate::model::load)) {
                        Some(Ok(model)) => model,
                        Some(Err(e)) => { scene.warnings.push(format!("Skipped model {e}")); continue; }
                        None => { scene.warnings.push(format!("Skipped model {file}: file not found")); continue; }
                    };
                    let material = material.map(|m| scene.add_material(m.into(), dir.as_deref(), &mut cache));
                    let place = crate::model::Placement { position: vector(position), rotation, scale, size };
                    scene.add_model(model, place, material, dir.as_deref(), &mut cache, &file);
                }
                _ => {}
            }
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
        scene.apply_displacement();
        if let Some([low, high]) = file.bounds {
            let (low, high) = (vector(low), vector(high));
            require_positive(&(high - low).array(), "Scene bounds")?;
            scene.extends = Aabb::new(low, high - low);
        } else {
            scene.fit_bounds();
        }
        if let Some(fog) = &file.fog {
            let (low, high) = match fog.bounds {
                Some([low, high]) => (vector(low), vector(high)),
                None => scene.object_bounds().ok_or("Fog needs objects to fill, or bounds")?,
            };
            require_positive(&(high - low).array(), "Fog bounds")?;
            let half = (high - low) * 0.5;
            let axes = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
            scene.volumes.push(fog.volume(crate::volume::Shape::Box { center: low + half, axes, half })?);
        }
        Ok(scene)
    }

    fn add_analytic(
        &mut self,
        object: ObjectFile,
        dir: Option<&Path>,
        cache: &mut HashMap<String, Option<usize>>,
    ) -> Result<(), String> {
        if let ObjectFile::Volume { center, size, radius, rotation, density, color, anisotropy } = object {
            let medium = MediumFile { bounds: None, density, color, anisotropy };
            let center = vector(center);
            let shape = match (size, radius) {
                (Some(size), None) => {
                    require_positive(&size, "A volume box")?;
                    let axes = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)]
                        .map(|a| crate::solid::rotate(a, rotation));
                    crate::volume::Shape::Box { center, axes, half: vector(size) * 0.5 }
                }
                (None, Some(radius)) => {
                    require_positive(&[radius], "A volume sphere")?;
                    crate::volume::Shape::Sphere { center, radius }
                }
                _ => return Err("A volume needs either a size (a box) or a radius (a sphere)".into()),
            };
            self.volumes.push(medium.volume(shape)?);
            return Ok(());
        }
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
            ObjectFile::Plane { .. } | ObjectFile::Mesh { .. } | ObjectFile::Model { .. } | ObjectFile::Volume { .. } => return Ok(()),
        };
        let material = self.add_material(material.into(), dir, cache);
        self.corner_normals.push([Vec3::default(); 3]);
        self.prims.push(Primitive::Solid(Solid::new(shape, vector(center), rotation, material)));
        Ok(())
    }

    /// Grows the KD-Tree bounds to hold the whole scene. Explicit bounds preserve the
    /// historical volume when requested by a reference scene.
    pub(crate) fn fit_bounds(&mut self) {
        let (mut low, mut high) = (self.extends.pos, self.extends.pos + self.extends.size);
        if let Some((a, b)) = self.object_bounds() {
            low = Vec3::new(low.x.min(a.x), low.y.min(a.y), low.z.min(a.z));
            high = Vec3::new(high.x.max(b.x), high.y.max(b.y), high.z.max(b.z));
        }
        let margin = Vec3::new(0.01, 0.01, 0.01);
        self.extends = Aabb::new(low - margin, high - low + margin * 2.0);
    }

    /// The box around the objects, the infinite ground left out; None with no objects.
    pub(crate) fn object_bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut low = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut high = -low;
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
        (low.x <= high.x).then_some((low, high))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_fills_the_objects_and_volumes_check_their_values() {
        let open = |objects: &str, fog: &str| {
            let text = format!(r#"{{{fog}"lights":[],"objects":[{objects}]}}"#);
            Scene::from_far(text.as_bytes(), Path::new("test.far"))
        };
        let ground = r#"{"type":"ground"},{"type":"sphere","center":[1,2,3],"radius":1}"#;
        let scene = open(ground, r#""fog":{"density":0.1,"anisotropy":0.4},"#).unwrap();
        let fog = scene.volumes[0];
        // The infinite ground is left out of the box the fog fills.
        assert_eq!(fog.interval(Vec3::new(1.0, 2.0, -10.0), Vec3::new(0.0, 0.0, 1.0), 100.0), Some((12.0, 14.0)));
        assert_eq!(fog.anisotropy, 0.4);
        let boxed = open(ground, r#""fog":{"density":0.1,"bounds":[[0,0,0],[1,1,1]]},"#).unwrap();
        assert!(boxed.volumes[0].interval(Vec3::new(0.5, 0.5, -1.0), Vec3::new(0.0, 0.0, 1.0), 9.0).is_some());
        let sphere = open(r#"{"type":"volume","center":[0,0,0],"radius":2,"density":1,"color":[0.5,0.5,0.5]}"#, "").unwrap();
        assert_eq!(sphere.volumes.len(), 1);
        assert!(sphere.prims.is_empty());
        for bad in [
            r#"{"type":"volume","center":[0,0,0],"density":1}"#,
            r#"{"type":"volume","center":[0,0,0],"radius":1,"size":[1,1,1],"density":1}"#,
            r#"{"type":"volume","center":[0,0,0],"radius":1,"density":-1}"#,
            r#"{"type":"volume","center":[0,0,0],"radius":1,"density":1,"anisotropy":1}"#,
            r#"{"type":"volume","center":[0,0,0],"radius":1,"density":1,"color":[2,0,0]}"#,
        ] {
            assert!(open(bad, "").is_err(), "{bad}");
        }
        assert!(open("", r#""fog":{"density":0.1},"#).is_err());
    }

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

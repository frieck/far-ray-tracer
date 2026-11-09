//! Models from other 3D programs: OBJ (with its MTL materials) and glTF 2.0 (`.gltf` or `.glb`).
//!
//! Both load into a [`Model`]: shared vertices with optional normals and texture coordinates,
//! triangles, and materials in the program's terms (color, roughness, metallic, glass,
//! emission and maps). [`Scene::add_model`] then places it in a scene.
//!
//! Texture coordinates follow the image convention, v = 0 at the top, as in glTF; OBJ counts
//! v from the bottom and is flipped on reading.

use crate::bump::BumpKind;
use crate::scene::{MaterialData, Primitive, Scene, Triangle, Vertex};
use crate::texture::Texture;
use crate::vector::{Color, Vec3};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ModelVertex {
    pub pos: Vec3,
    pub normal: Option<Vec3>,
    pub uv: Option<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ModelTriangle {
    pub vertices: [u32; 3],
    /// Index into `Model::materials`; None uses a plain gray.
    pub material: Option<usize>,
}

/// An image a material uses: a file, or pixels already decoded from a glTF file.
#[derive(Clone)]
pub(crate) enum Image {
    File(PathBuf),
    Decoded { key: String, texture: Texture },
}

#[derive(Clone)]
pub(crate) struct ModelMaterial {
    pub name: String,
    pub color: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    pub ior: f32,
    pub glass: bool,
    /// Emitted light, strength included.
    pub emission: [f32; 3],
    pub color_map: Option<Image>,
    pub roughness_map: Option<Image>,
    pub metallic_map: Option<Image>,
    pub emission_map: Option<Image>,
    /// Dark parts are cut away.
    pub alpha_map: Option<Image>,
    pub bump: Option<(Image, BumpKind, f32)>,
}

impl ModelMaterial {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_string(),
            color: [0.8; 3],
            roughness: 0.5,
            metallic: 0.0,
            ior: 1.5,
            glass: false,
            emission: [0.0; 3],
            color_map: None,
            roughness_map: None,
            metallic_map: None,
            emission_map: None,
            alpha_map: None,
            bump: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct Model {
    pub vertices: Vec<ModelVertex>,
    pub triangles: Vec<ModelTriangle>,
    pub materials: Vec<ModelMaterial>,
    pub warnings: Vec<String>,
}

/// File types a model can come from.
pub fn is_model(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["obj", "gltf", "glb"].iter().any(|known| e.eq_ignore_ascii_case(known))
    })
}

pub(crate) fn load(path: &Path) -> Result<Model, String> {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let model = match extension.as_str() {
        "obj" => {
            let text = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            parse_obj(&String::from_utf8_lossy(&text), path.parent().unwrap_or(Path::new(".")))
        }
        "gltf" | "glb" => load_gltf(path)?,
        _ => return Err(format!("{}: models load from .obj, .gltf or .glb files", path.display())),
    };
    if model.triangles.is_empty() {
        return Err(format!("{}: no triangles found", path.display()));
    }
    Ok(model)
}

// ---------------------------------------------------------------------------------------------
// OBJ and MTL

fn numbers<const N: usize>(words: &[&str]) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    for (slot, word) in out.iter_mut().zip(words) {
        *slot = word.parse().ok().filter(|v: &f32| v.is_finite())?;
    }
    (words.len() >= N).then_some(out)
}

/// Resolves a 1-based OBJ index, or a negative one counted back from the end.
fn obj_index(word: &str, count: usize) -> Option<usize> {
    let i: i64 = word.parse().ok()?;
    let index = if i > 0 { i - 1 } else { count as i64 + i };
    (0..count as i64).contains(&index).then_some(index as usize)
}

pub(crate) fn parse_obj(text: &str, folder: &Path) -> Model {
    let mut model = Model::default();
    let (mut positions, mut normals, mut uvs) = (Vec::new(), Vec::new(), Vec::new());
    let mut library: HashMap<String, ModelMaterial> = HashMap::new();
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut current: Option<String> = None;
    let mut corners: HashMap<(usize, Option<usize>, Option<usize>), u32> = HashMap::new();
    let mut bad_faces = 0;
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some((&keyword, rest)) = words.split_first() else { continue };
        match keyword {
            "v" => match numbers::<3>(rest) {
                Some([x, y, z]) => positions.push(Vec3::new(x, y, z)),
                None => positions.push(Vec3::default()),
            },
            "vn" => normals.push(numbers::<3>(rest).map(|[x, y, z]| Vec3::new(x, y, z)).unwrap_or_default()),
            "vt" => {
                let u = rest.first().and_then(|w| w.parse().ok()).unwrap_or(0.0);
                let v = rest.get(1).and_then(|w| w.parse().ok()).unwrap_or(0.0);
                uvs.push([u, 1.0 - v]);
            }
            "mtllib" => {
                // File names may hold spaces; several files may follow, so try the whole rest first.
                let whole = rest.join(" ");
                let names: Vec<String> = if folder.join(&whole).is_file() { vec![whole] } else { rest.iter().map(|s| s.to_string()).collect() };
                for name in names {
                    let path = folder.join(name.replace('\\', "/"));
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            for material in parse_mtl(&String::from_utf8_lossy(&bytes), path.parent().unwrap_or(folder)) {
                                library.insert(material.name.clone(), material);
                            }
                        }
                        Err(e) => model.warnings.push(format!("Skipped material library {}: {e}", path.display())),
                    }
                }
            }
            "usemtl" => current = Some(rest.join(" ")),
            "f" => {
                let mut face = Vec::with_capacity(rest.len());
                for word in rest {
                    let mut parts = word.split('/');
                    let p = parts.next().and_then(|w| obj_index(w, positions.len()));
                    let t = parts.next().filter(|w| !w.is_empty()).and_then(|w| obj_index(w, uvs.len()));
                    let n = parts.next().filter(|w| !w.is_empty()).and_then(|w| obj_index(w, normals.len()));
                    let Some(p) = p else { face.clear(); break };
                    let next = model.vertices.len() as u32;
                    let id = *corners.entry((p, t, n)).or_insert_with(|| {
                        model.vertices.push(ModelVertex { pos: positions[p], normal: n.map(|n| normals[n]), uv: t.map(|t| uvs[t]) });
                        next
                    });
                    face.push(id);
                }
                if face.len() < 3 {
                    bad_faces += 1;
                    continue;
                }
                let material = current.as_ref().map(|name| {
                    let count = used.len();
                    *used.entry(name.clone()).or_insert(count)
                });
                for i in 1..face.len() - 1 {
                    model.triangles.push(ModelTriangle { vertices: [face[0], face[i], face[i + 1]], material });
                }
            }
            _ => {}
        }
    }
    if bad_faces > 0 {
        model.warnings.push(format!("Skipped {bad_faces} faces with missing vertices"));
    }
    let mut order: Vec<(&String, &usize)> = used.iter().collect();
    order.sort_by_key(|(_, i)| **i);
    model.materials = order.into_iter().map(|(name, _)| match library.get(name) {
        Some(m) => m.clone(),
        None => {
            model.warnings.push(format!("Material {name} is not in the material library; it renders gray"));
            ModelMaterial::named(name)
        }
    }).collect();
    model
}

/// Splits a map statement into its options and file name: `-bm 0.5 bricks.png` gives the
/// bump multiplier and `bricks.png`.
fn map_statement(words: &[&str], folder: &Path) -> Option<(PathBuf, HashMap<String, f32>)> {
    let mut options = HashMap::new();
    let mut i = 0;
    while i < words.len() && words[i].starts_with('-') && words[i].len() > 1 {
        let name = words[i].trim_start_matches('-').to_string();
        i += 1;
        // -o, -s and -t take up to three numbers; the rest take one value.
        let most = if ["o", "s", "t"].contains(&name.as_str()) { 3 } else if name == "mm" { 2 } else { 1 };
        let mut taken = 0;
        while taken < most && i < words.len() {
            match words[i].parse::<f32>() {
                Ok(value) => {
                    if taken == 0 { options.insert(name.clone(), value); }
                    i += 1;
                    taken += 1;
                }
                // Values such as `-clamp on` are words.
                Err(_) if taken == 0 && most == 1 && i + 1 < words.len() => { i += 1; taken += 1; }
                Err(_) => break,
            }
        }
    }
    let name = words[i..].join(" ").replace('\\', "/");
    (!name.is_empty()).then(|| (folder.join(name), options))
}

pub(crate) fn parse_mtl(text: &str, folder: &Path) -> Vec<ModelMaterial> {
    struct Reading {
        m: ModelMaterial,
        shininess: Option<f32>,
        roughness: Option<f32>,
        dissolve: f32,
        illum: u32,
        filter: Option<[f32; 3]>,
    }
    let finish = |r: Reading| {
        let mut m = r.m;
        // Phong shininess to GGX roughness: alpha = sqrt(2 / (Ns + 2)), roughness = sqrt(alpha).
        m.roughness = r.roughness.unwrap_or_else(|| r.shininess.map_or(0.5, |ns| (2.0 / (ns.max(0.0) + 2.0)).powf(0.25)))
            .clamp(0.0, 1.0);
        m.glass = r.dissolve <= 0.5 || ([4, 6, 7, 9].contains(&r.illum) && r.dissolve < 1.0);
        if m.glass {
            m.color = r.filter.unwrap_or([1.0; 3]);
            m.roughness = r.roughness.unwrap_or(0.0);
        }
        m
    };
    let mut done = Vec::new();
    let mut reading: Option<Reading> = None;
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some((&keyword, rest)) = words.split_first() else { continue };
        if keyword == "newmtl" {
            done.extend(reading.take().map(finish));
            reading = Some(Reading { m: ModelMaterial::named(&rest.join(" ")), shininess: None, roughness: None,
                dissolve: 1.0, illum: 2, filter: None });
            continue;
        }
        let Some(r) = reading.as_mut() else { continue };
        let color = || numbers::<3>(rest).or_else(|| numbers::<1>(rest).map(|[v]| [v; 3])).map(|c| c.map(|v| v.clamp(0.0, 1.0)));
        let value = || numbers::<1>(rest).map(|[v]| v);
        match keyword.to_ascii_lowercase().as_str() {
            "kd" => if let Some(c) = color() { r.m.color = c },
            "ke" => if let Some(c) = numbers::<3>(rest) { r.m.emission = c.map(|v| v.max(0.0)) },
            "ns" => r.shininess = value(),
            "pr" => r.roughness = value(),
            "pm" => if let Some(v) = value() { r.m.metallic = v.clamp(0.0, 1.0) },
            "ni" => if let Some(v) = value() { r.m.ior = v.clamp(1.0, 3.0) },
            "d" => if let Some(v) = value() { r.dissolve = v },
            "tr" => if let Some(v) = value() { r.dissolve = 1.0 - v },
            "tf" => r.filter = color(),
            "illum" => r.illum = value().map_or(2, |v| v as u32),
            "map_kd" => r.m.color_map = map_statement(rest, folder).map(|(p, _)| Image::File(p)),
            "map_ke" => r.m.emission_map = map_statement(rest, folder).map(|(p, _)| Image::File(p)),
            "map_pr" => r.m.roughness_map = map_statement(rest, folder).map(|(p, _)| Image::File(p)),
            "map_pm" => r.m.metallic_map = map_statement(rest, folder).map(|(p, _)| Image::File(p)),
            "map_d" => r.m.alpha_map = map_statement(rest, folder).map(|(p, _)| Image::File(p)),
            "map_bump" | "bump" => {
                r.m.bump = map_statement(rest, folder).map(|(p, options)| {
                    // Height maps are a few hundredths of a unit deep at -bm 1.
                    (Image::File(p), BumpKind::Height, 0.02 * options.get("bm").copied().unwrap_or(1.0).max(0.0))
                });
            }
            "norm" | "map_kn" => {
                r.m.bump = map_statement(rest, folder).map(|(p, options)| {
                    (Image::File(p), BumpKind::Normal, options.get("bm").copied().unwrap_or(1.0).max(0.0))
                });
            }
            _ => {}
        }
    }
    done.extend(reading.map(finish));
    done
}

// ---------------------------------------------------------------------------------------------
// glTF

/// One channel or the color of a decoded glTF image, as a texture.
fn decoded(data: &gltf::image::Data, key: String, pick: fn([f32; 4]) -> Color) -> Option<Image> {
    use gltf::image::Format;
    let channels = match data.format {
        Format::R8 => 1,
        Format::R8G8 => 2,
        Format::R8G8B8 => 3,
        Format::R8G8B8A8 => 4,
        _ => return None,
    };
    let inv = 1.0 / 256.0;
    let image = data.pixels.chunks_exact(channels).map(|p| {
        let get = |i: usize| p.get(i).map_or(if i == 3 { 1.0 } else { p[0] as f32 * inv }, |&v| v as f32 * inv);
        pick([get(0), get(1), get(2), get(3)])
    }).collect();
    Some(Image::Decoded { key, texture: Texture { width: data.width as i32, height: data.height as i32, image } })
}

fn load_gltf(path: &Path) -> Result<Model, String> {
    let (document, buffers, images) = gltf::import(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut model = Model::default();
    let file_key = path.display().to_string().replace(['/', '\\'], ":");
    let mut warned_uv_set = false;
    for (index, material) in document.materials().enumerate() {
        let pbr = material.pbr_metallic_roughness();
        let base = pbr.base_color_factor();
        let mut m = ModelMaterial::named(material.name().unwrap_or("glTF material"));
        m.color = [base[0], base[1], base[2]].map(|v| v.clamp(0.0, 1.0));
        m.roughness = pbr.roughness_factor().clamp(0.0, 1.0);
        m.metallic = pbr.metallic_factor().clamp(0.0, 1.0);
        m.ior = material.ior().unwrap_or(1.5).clamp(1.0, 3.0);
        m.glass = material.transmission().is_some_and(|t| t.transmission_factor() > 0.5);
        let strength = material.emissive_strength().unwrap_or(1.0);
        m.emission = material.emissive_factor().map(|v| (v * strength).max(0.0));
        let key = |what: &str| format!("<{file_key} material {index} {what}>");
        let mut texture_uses = Vec::new();
        let mut image = |info: Option<(gltf::texture::Texture, u32)>, what: &str, pick: fn([f32; 4]) -> Color| {
            let (texture, set) = info?;
            if set != 0 {
                texture_uses.push(set);
            }
            images.get(texture.source().index()).and_then(|data| decoded(data, key(what), pick))
        };
        let color_info = pbr.base_color_texture().map(|t| (t.texture(), t.tex_coord()));
        m.color_map = image(color_info.clone(), "color", |c| Color::new(c[0], c[1], c[2]));
        if !matches!(material.alpha_mode(), gltf::material::AlphaMode::Opaque) {
            m.alpha_map = image(color_info, "alpha", |c| Color::new(c[3], c[3], c[3]));
        }
        let packed = pbr.metallic_roughness_texture().map(|t| (t.texture(), t.tex_coord()));
        // glTF packs roughness in green and metalness in blue.
        m.roughness_map = image(packed.clone(), "roughness", |c| Color::new(c[1], c[1], c[1]));
        m.metallic_map = image(packed, "metallic", |c| Color::new(c[2], c[2], c[2]));
        m.emission_map = image(material.emissive_texture().map(|t| (t.texture(), t.tex_coord())), "emission",
            |c| Color::new(c[0], c[1], c[2]));
        if let Some(normal) = material.normal_texture() {
            let scale = normal.scale().max(0.0);
            m.bump = image(Some((normal.texture(), normal.tex_coord())), "normal", |c| Color::new(c[0], c[1], c[2]))
                .map(|i| (i, BumpKind::Normal, scale));
        }
        if m.glass {
            m.roughness_map = None;
            m.metallic_map = None;
        }
        if !texture_uses.is_empty() && !warned_uv_set {
            warned_uv_set = true;
            model.warnings.push("Some textures use a second set of texture coordinates; the first is used".into());
        }
        model.materials.push(m);
    }

    let scene = document.default_scene().or_else(|| document.scenes().next()).ok_or("The glTF file has no scene")?;
    let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let mut stack: Vec<(gltf::Node, [[f32; 4]; 4])> = scene.nodes().map(|n| (n, identity)).collect();
    let mut skipped_modes = 0;
    while let Some((node, parent)) = stack.pop() {
        let matrix = multiply(&parent, &node.transform().matrix());
        stack.extend(node.children().map(|child| (child, matrix)));
        let Some(mesh) = node.mesh() else { continue };
        let normal_matrix = normal_matrix(&matrix);
        let flip = determinant3(&matrix) < 0.0;
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                skipped_modes += 1;
                continue;
            }
            let reader = primitive.reader(|b| buffers.get(b.index()).map(|data| &data.0[..]));
            let Some(positions) = reader.read_positions() else { continue };
            let first = model.vertices.len() as u32;
            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
            let uvs: Option<Vec<[f32; 2]>> = reader.read_tex_coords(0).map(|t| t.into_f32().collect());
            for (i, p) in positions.enumerate() {
                let pos = transform_point(&matrix, p);
                let normal = normals.as_ref().and_then(|n| n.get(i)).map(|&n| {
                    let mut n = multiply3(&normal_matrix, n);
                    n.normalize();
                    n
                }).filter(|n| n.x.is_finite());
                let uv = uvs.as_ref().and_then(|t| t.get(i)).copied();
                model.vertices.push(ModelVertex { pos, normal, uv });
            }
            let count = model.vertices.len() as u32 - first;
            let indices: Vec<u32> = match reader.read_indices() {
                Some(indices) => indices.into_u32().collect(),
                None => (0..count).collect(),
            };
            let material = primitive.material().index();
            for t in indices.chunks_exact(3) {
                if t.iter().any(|&i| i >= count) {
                    continue;
                }
                let mut ids = [first + t[0], first + t[1], first + t[2]];
                if flip {
                    ids.swap(1, 2);
                }
                model.triangles.push(ModelTriangle { vertices: ids, material });
            }
        }
    }
    if skipped_modes > 0 {
        model.warnings.push(format!("Skipped {skipped_modes} parts drawn as points or lines"));
    }
    Ok(model)
}

type Matrix = [[f32; 4]; 4];

/// Column-major product a · b, as glTF stores matrices.
fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0; 4]; 4];
    for (column, out_column) in out.iter_mut().enumerate() {
        for (row, cell) in out_column.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][row] * b[column][k]).sum();
        }
    }
    out
}

fn transform_point(m: &Matrix, p: [f32; 3]) -> Vec3 {
    let at = |row: usize| m[0][row] * p[0] + m[1][row] * p[1] + m[2][row] * p[2] + m[3][row];
    Vec3::new(at(0), at(1), at(2))
}

fn determinant3(m: &Matrix) -> f32 {
    let c = |col: usize, row: usize| m[col][row];
    c(0, 0) * (c(1, 1) * c(2, 2) - c(2, 1) * c(1, 2)) - c(1, 0) * (c(0, 1) * c(2, 2) - c(2, 1) * c(0, 2))
        + c(2, 0) * (c(0, 1) * c(1, 2) - c(1, 1) * c(0, 2))
}

/// Rows of the cofactor matrix of the upper 3×3, which turns normals as the inverse
/// transpose does, up to a scale that normalizing removes.
fn normal_matrix(m: &Matrix) -> [[f32; 3]; 3] {
    let a = |row: usize, col: usize| m[col][row];
    let mut out = [[0.0; 3]; 3];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (col, cell) in out_row.iter_mut().enumerate() {
            let (r1, r2) = ((row + 1) % 3, (row + 2) % 3);
            let (c1, c2) = ((col + 1) % 3, (col + 2) % 3);
            *cell = a(r1, c1) * a(r2, c2) - a(r1, c2) * a(r2, c1);
        }
    }
    out
}

fn multiply3(m: &[[f32; 3]; 3], v: [f32; 3]) -> Vec3 {
    let at = |row: usize| m[row][0] * v[0] + m[row][1] * v[1] + m[row][2] * v[2];
    Vec3::new(at(0), at(1), at(2))
}

// ---------------------------------------------------------------------------------------------
// Placing a model in a scene

/// Where a model goes: turned by `rotation` (degrees about X, Y and Z, as solids turn), scaled,
/// then moved by `position`. With `size`, the model is scaled so its largest side measures
/// `size` instead, and its lowest point, centered, rests at `position`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub position: Vec3,
    pub rotation: [f32; 3],
    pub scale: f32,
    pub size: Option<f32>,
}

impl Placement {
    /// The model's points in the scene, and the factor its sizes were scaled by.
    fn apply(&self, model: &Model) -> (Vec<Vec3>, f32) {
        let turned: Vec<Vec3> = model.vertices.iter().map(|v| crate::solid::rotate(v.pos, self.rotation)).collect();
        let Some(size) = self.size else {
            return (turned.iter().map(|&p| p * self.scale + self.position).collect(), self.scale);
        };
        let (min, max) = bounds(&turned);
        let extent = max - min;
        let side = extent.x.max(extent.y).max(extent.z);
        let factor = if side > 0.0 { size / side } else { 1.0 };
        let anchor = Vec3::new(0.5 * (min.x + max.x), min.y, 0.5 * (min.z + max.z));
        (turned.iter().map(|&p| (p - anchor) * factor + self.position).collect(), factor)
    }
}

pub(crate) fn bounds(points: &[Vec3]) -> (Vec3, Vec3) {
    let mut min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = -min;
    for p in points {
        min = Vec3::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
        max = Vec3::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
    }
    (min, max)
}

impl Scene {
    /// Adds a model's triangles. `material` replaces all of its materials; otherwise each
    /// model material becomes a scene material, with its textures.
    pub(crate) fn add_model(
        &mut self,
        model: Model,
        place: Placement,
        material: Option<usize>,
        dir: Option<&Path>,
        cache: &mut HashMap<String, Option<usize>>,
        name: &str,
    ) {
        self.warnings.extend(model.warnings.iter().map(|w| format!("Model {name}: {w}")));
        let (points, factor) = place.apply(&model);
        let materials: Vec<usize> = match material {
            Some(_) => Vec::new(),
            None => model.materials.iter().map(|m| self.add_model_material(m, factor, dir, cache)).collect(),
        };
        let gray = match material {
            Some(m) => m,
            None => self.add_model_material(&ModelMaterial::named("default"), factor, dir, cache),
        };

        let base = self.vertices.len() as u32;
        for (v, &pos) in model.vertices.iter().zip(&points) {
            let mut vertex = Vertex::new(pos);
            if let Some([u, w]) = v.uv {
                // `Texture::texel_fixed` reads half a texture to the right, as in 2008.
                vertex.u = u - 0.5;
                vertex.v = w;
            }
            self.vertices.push(vertex);
        }
        let first_face = self.prims.len();
        let mut file_normals = Vec::new();
        let mut degenerate = 0;
        for t in &model.triangles {
            let [a, b, c] = t.vertices.map(|i| points[i as usize]);
            let area = (b - a).cross(c - a).length();
            if !(area > 1e-12) {
                degenerate += 1;
                continue;
            }
            let id = match (material, t.material) {
                (Some(m), _) => m,
                (None, Some(i)) => materials.get(i).copied().unwrap_or(gray),
                (None, None) => gray,
            };
            let ids = t.vertices.map(|i| base + i);
            let triangle = Triangle::new(&mut self.vertices, ids, id);
            let normals = t.vertices.map(|i| {
                model.vertices[i as usize].normal.map(|n| {
                    let mut n = crate::solid::rotate(n, place.rotation);
                    n.normalize();
                    n
                })
            });
            file_normals.push(normals);
            self.prims.push(Primitive::Triangle(triangle));
        }
        if degenerate > 0 {
            self.warnings.push(format!("Model {name}: skipped {degenerate} triangles with no area"));
        }
        self.compute_corner_normals(first_face);
        let start = self.corner_normals.len() - file_normals.len();
        for (corners, normals) in self.corner_normals[start..].iter_mut().zip(file_normals) {
            for (corner, normal) in corners.iter_mut().zip(normals) {
                if let Some(n) = normal {
                    *corner = n;
                }
            }
        }
        // Classic mode reads vertex normals.
        for (prim, corners) in self.prims[first_face..].iter().zip(&self.corner_normals[start..]) {
            if let Primitive::Triangle(t) = prim {
                for (id, normal) in t.vertices.iter().zip(corners) {
                    self.vertices[*id as usize].normal = *normal;
                }
            }
        }
    }

    fn add_model_material(&mut self, m: &ModelMaterial, factor: f32, dir: Option<&Path>,
        cache: &mut HashMap<String, Option<usize>>) -> usize {
        let mut key = |image: &Option<Image>| -> String {
            match image {
                None => String::new(),
                Some(Image::File(path)) => {
                    let key = format!("<{}>", path.display().to_string().replace(['/', '\\'], ":"));
                    if !cache.contains_key(&key) {
                        let loaded = match Texture::load(path) {
                            Ok(t) => {
                                self.textures.push(t);
                                Some(self.textures.len() - 1)
                            }
                            Err(e) => {
                                self.warnings.push(format!("Skipped texture {e}"));
                                None
                            }
                        };
                        cache.insert(key.clone(), loaded);
                    }
                    key
                }
                Some(Image::Decoded { key, texture }) => {
                    if !cache.contains_key(key) {
                        self.textures.push(texture.clone());
                        cache.insert(key.clone(), Some(self.textures.len() - 1));
                    }
                    key.clone()
                }
            }
        };
        let surface = if m.glass {
            serde_json::json!({"preset": "glass", "roughness": m.roughness.clamp(0.0, 1.0), "ior": m.ior})
        } else {
            serde_json::json!({"preset": "custom", "roughness": m.roughness.clamp(0.0, 1.0),
                "metallic": m.metallic.clamp(0.0, 1.0), "ior": m.ior})
        };
        let surface = serde_json::from_value(surface).ok();
        let texture = key(&m.color_map);
        let roughness_map = key(&m.roughness_map);
        let metallic_map = key(&m.metallic_map);
        let alpha_map = key(&m.alpha_map);
        let emission_map = key(&m.emission_map);
        let bump = m.bump.as_ref().map(|(image, kind, strength)| {
            let strength = if *kind == BumpKind::Height { strength * factor } else { *strength };
            (key(&Some(image.clone())), *kind, strength)
        });
        let emits = m.emission.iter().any(|v| *v > 0.0);
        let data = MaterialData {
            surface,
            color: Vec3::new(m.color[0], m.color[1], m.color[2]),
            refl: 0.0,
            diff: 1.0,
            spec: 1.0,
            refr: 0.0,
            ior: m.ior,
            drefl: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
            texture,
            bump,
            displacement: None,
            roughness_map,
            metallic_map,
            emission: (emits || !emission_map.is_empty())
                .then(|| (Vec3::new(m.emission[0], m.emission[1], m.emission[2]), emission_map)),
            alpha_map,
        };
        self.add_material(data, dir, cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obj_faces_share_corners_and_fan_into_triangles() {
        let model = parse_obj(
            "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 1\nvn 0 0 1\n\
             f 1/1/1 2/1/1 3/2/1 4/2/1\nf -4 -3 -2\nusemtl red\nf 1 3 4\n",
            Path::new("."),
        );
        assert_eq!(model.triangles.len(), 4);
        assert_eq!(model.triangles[0].vertices, [0, 1, 2]);
        assert_eq!(model.triangles[1].vertices, [0, 2, 3]);
        // Plain indices make corners of their own, apart from the ones with normals.
        assert_eq!(model.triangles[2].vertices, [4, 5, 6]);
        assert_eq!(model.triangles[3].material, Some(0));
        assert_eq!(model.triangles[0].material, None);
        // v is flipped: OBJ counts it from the bottom.
        assert_eq!(model.vertices[2].uv, Some([1.0, 0.0]));
        assert_eq!(model.vertices[0].normal, Some(Vec3::new(0.0, 0.0, 1.0)));
        assert_eq!(model.materials.len(), 1);
        assert!(model.warnings.iter().any(|w| w.contains("red")));
    }

    #[test]
    fn obj_skips_faces_with_missing_vertices() {
        let model = parse_obj("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 9\nf 1 2 3\n", Path::new("."));
        assert_eq!(model.triangles.len(), 1);
        assert!(!model.warnings.is_empty());
    }

    #[test]
    fn mtl_reads_pbr_glass_emission_and_maps() {
        let materials = parse_mtl(
            "newmtl paint\nKd 0.8 0.1 0.1\nNs 900\nmap_Kd -s 2 2 1 tex/paint.png\nbump -bm 0.5 bump.png\n\
             newmtl steel\nKd 0.9 0.9 0.9\nPr 0.3\nPm 1\n\
             newmtl glass\nKd 0.2 0.2 0.2\nd 0.1\nNi 1.45\nTf 0.9 1 0.9\n\
             newmtl lamp\nKe 4 3 2\n",
            Path::new("models"),
        );
        let [paint, steel, glass, lamp] = &materials[..] else { panic!("{}", materials.len()) };
        assert_eq!(paint.color, [0.8, 0.1, 0.1]);
        assert!(paint.roughness < 0.25 && !paint.glass);
        assert!(matches!(&paint.color_map, Some(Image::File(p)) if p == Path::new("models/tex/paint.png")));
        assert!(matches!(&paint.bump, Some((Image::File(_), BumpKind::Height, s)) if (*s - 0.01).abs() < 1e-6));
        assert_eq!((steel.roughness, steel.metallic), (0.3, 1.0));
        assert!(glass.glass && glass.ior == 1.45 && glass.color == [0.9, 1.0, 0.9]);
        assert_eq!(lamp.emission, [4.0, 3.0, 2.0]);
        assert_eq!(lamp.roughness, 0.5);
    }

    #[test]
    fn matrices_compose_and_turn_normals() {
        // Scale x by 2, then move by (1, 2, 3).
        let scale = [[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let shift = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 2.0, 3.0, 1.0]];
        let m = multiply(&shift, &scale);
        assert_eq!(transform_point(&m, [1.0, 1.0, 1.0]), Vec3::new(3.0, 3.0, 4.0));
        // A slanted normal leans toward the squashed axis, as the inverse transpose does.
        let mut n = multiply3(&normal_matrix(&m), [1.0, 1.0, 0.0]);
        n.normalize();
        assert!(n.y > n.x && n.x > 0.0);
        assert!(determinant3(&m) > 0.0);
        let mirror = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert!(determinant3(&mirror) < 0.0);
    }

    #[test]
    fn size_rests_the_model_on_its_position() {
        let model = parse_obj("v 0 0 0\nv 4 0 0\nv 4 2 0\nf 1 2 3\n", Path::new("."));
        let place = Placement { position: Vec3::new(1.0, -1.0, 5.0), rotation: [0.0; 3], scale: 1.0, size: Some(2.0) };
        let (points, factor) = place.apply(&model);
        assert_eq!(factor, 0.5);
        let (min, max) = bounds(&points);
        assert!((min - Vec3::new(0.0, -1.0, 5.0)).length() < 1e-6 && (max - Vec3::new(2.0, 0.0, 5.0)).length() < 1e-6);
        let turned = Placement { position: Vec3::default(), rotation: [0.0, 90.0, 0.0], scale: 2.0, size: None };
        let (points, _) = turned.apply(&model);
        assert!((points[1] - Vec3::new(0.0, 0.0, -8.0)).length() < 1e-5, "{:?}", points[1]);
    }
}

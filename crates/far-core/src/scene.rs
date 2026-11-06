//! Scene: primitives, materials and `.frt` file loading.

use crate::mesh3ds;
use crate::solid::Solid;
use crate::texture::Texture;
use crate::vector::{Aabb, Color, Vec3, PI};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Data folders, followed by the names used by the 2008 distribution, so scenes
/// in an untouched copy of the original program still open.
const DATA_FOLDERS: [&str; 2] = ["scene-data", "DadosCena"];
const IMAGE_FOLDERS: [&str; 2] = ["images", "Imagens"];
const MESH_FOLDERS: [&str; 2] = ["meshes", "Malhas"];

const HEADER_SIZE: usize = 12;
const SPHERE_SIZE: usize = 220;
const PLANE_SIZE: usize = 252;
const MESH_SIZE: usize = 1220;
const MATERIAL_SIZE: usize = 1052;

pub struct Material {
    pub color: Color,
    pub refl: f32,
    pub diff: f32,
    pub spec: f32,
    pub refr: f32,
    pub ior: f32,
    pub drefl: f32,
    pub texture: Option<usize>,
    pub ru_scale: f32,
    pub rv_scale: f32,
}

#[derive(Clone, Copy, Default)]
pub struct Vertex {
    pub pos: Vec3,
    pub normal: Vec3,
    pub u: f32,
    pub v: f32,
}

impl Vertex {
    pub(crate) fn new(pos: Vec3) -> Self {
        Self { pos, normal: Vec3::default(), u: pos.x / 18.0, v: pos.y / 18.0 }
    }
}

pub struct Sphere {
    pub center: Vec3,
    pub sq_radius: f32,
    pub radius: f32,
    pub inv_radius: f32,
    pub material: usize,
    ve: Vec3,
    vn: Vec3,
    vc: Vec3,
}

impl Sphere {
    pub(crate) fn new(center: Vec3, radius: f32, material: usize) -> Self {
        let vn = Vec3::new(0.0, 1.0, 0.0);
        let ve = Vec3::new(1.0, 0.0, 0.0);
        Self {
            center,
            sq_radius: radius * radius,
            radius,
            inv_radius: 1.0 / radius,
            material,
            ve,
            vn,
            vc: vn.cross(ve),
        }
    }
}

pub struct Triangle {
    pub a: Vec3,
    pub n: Vec3,
    pub k: usize,
    pub nu: f32,
    pub nv: f32,
    pub nd: f32,
    pub bnu: f32,
    pub bnv: f32,
    pub cnu: f32,
    pub cnv: f32,
    pub vertices: [u32; 3],
    pub material: usize,
}

impl Triangle {
    /// Precomputes the intersection data. As in the original, it writes the face
    /// normal to all three vertices, so shared vertices end up with the normal of
    /// the last triangle created.
    pub(crate) fn new(vs: &mut [Vertex], ids: [u32; 3], material: usize) -> Self {
        let a = vs[ids[0] as usize].pos;
        let b_pos = vs[ids[1] as usize].pos;
        let c_pos = vs[ids[2] as usize].pos;
        let c = (b_pos - a).array();
        let b = (c_pos - a).array();
        let mut n = Vec3::new(b[0], b[1], b[2]).cross(Vec3::new(c[0], c[1], c[2]));
        let k = if n.x.abs() > n.y.abs() {
            if n.x.abs() > n.z.abs() { 0 } else { 2 }
        } else if n.y.abs() > n.z.abs() {
            1
        } else {
            2
        };
        let (u, v) = ((k + 1) % 3, (k + 2) % 3);
        let krec = 1.0 / n.cell(k);
        let nu = n.cell(u) * krec;
        let nv = n.cell(v) * krec;
        let nd = n.dot(a) * krec;
        let reci = 1.0 / (b[u] * c[v] - b[v] * c[u]);
        let tri = Self {
            a,
            n: Vec3::default(),
            k,
            nu,
            nv,
            nd,
            bnu: b[u] * reci,
            bnv: -b[v] * reci,
            cnu: c[v] * reci,
            cnv: -c[u] * reci,
            vertices: ids,
            material,
        };
        n.normalize();
        for id in ids {
            vs[id as usize].normal = n;
        }
        Self { n, ..tri }
    }
}

pub enum Primitive {
    Sphere(Sphere),
    Triangle(Triangle),
    Solid(Solid),
}

impl Primitive {
    #[inline(always)]
    pub fn material(&self) -> usize {
        match self {
            Primitive::Sphere(e) => e.material,
            Primitive::Triangle(t) => t.material,
            Primitive::Solid(s) => s.material,
        }
    }
}

pub struct Scene {
    /// Meshes, then planes, then spheres, the order the original uses.
    pub prims: Vec<Primitive>,
    pub lights: Vec<Sphere>,
    pub vertices: Vec<Vertex>,
    /// Corrected normals of each triangle corner, indexed like `prims`.
    /// Only Enhanced mode uses them; Classic mode keeps `Vertex::normal`.
    pub corner_normals: Vec<[Vec3; 3]>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    /// The original uses these fixed bounds instead of computing them from the scene.
    pub extends: Aabb,
    /// Problems that do not stop the render, such as a missing texture.
    pub warnings: Vec<String>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn f32(&mut self) -> f32 {
        let b = &self.data[self.pos..self.pos + 4];
        self.pos += 4;
        f32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }

    fn vector(&mut self) -> Vec3 {
        let (x, y, z) = (self.f32(), self.f32(), self.f32());
        Vec3::new(x, y, z)
    }

    fn bool(&mut self) -> bool {
        self.pos += 1;
        self.data[self.pos - 1] != 0
    }

    /// Zero-terminated Latin-1 text. The rest of the field often holds stale memory
    /// from the 2008 program and is ignored.
    fn text(&mut self, field_len: usize) -> String {
        let field = &self.data[self.pos..self.pos + field_len];
        self.pos += field_len;
        field.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect()
    }

    fn skip(&mut self, n: usize) {
        self.pos += n;
    }
}

pub(crate) struct MaterialData {
    pub(crate) color: Color,
    pub(crate) refl: f32,
    pub(crate) diff: f32,
    pub(crate) spec: f32,
    pub(crate) refr: f32,
    pub(crate) ior: f32,
    pub(crate) drefl: f32,
    pub(crate) u_scale: f32,
    pub(crate) v_scale: f32,
    pub(crate) texture: String,
}

fn read_material(l: &mut Reader) -> MaterialData {
    let color = l.vector();
    let (refl, diff, spec, refr) = (l.f32(), l.f32(), l.f32(), l.f32());
    let (ior, drefl, u_scale, v_scale) = (l.f32(), l.f32(), l.f32(), l.f32());
    l.skip(8);
    let texture = l.text(1000);
    MaterialData { color, refl, diff, spec, refr, ior, drefl, u_scale, v_scale, texture }
}

/// Stored paths may come from Windows (`C:\...`); only the file name matters.
fn base_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Finds a file ignoring case, so scenes also work on Linux.
fn locate(folder: &Path, name: &str) -> Option<PathBuf> {
    let direto = folder.join(name);
    if direto.is_file() {
        return Some(direto);
    }
    let target = name.to_lowercase();
    fs::read_dir(folder)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().to_lowercase() == target)
        .map(|e| e.path())
}

/// Walks up from the scene until it finds the data folder.
pub(crate) fn find_data_folder(scene: &Path) -> Option<PathBuf> {
    scene.ancestors().flat_map(|p| DATA_FOLDERS.map(|f| p.join(f))).find(|p| p.is_dir())
}

/// Finds a file in the first of `folders` that holds it.
fn locate_in(dir: &Path, folders: [&str; 2], name: &str) -> Option<PathBuf> {
    folders.iter().find_map(|f| locate(&dir.join(f), name))
}

impl Scene {
    pub fn open(path: &Path) -> Result<Self, String> {
        let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("far")) {
            return Self::from_far(&data, path);
        }
        Self::parse(&data, path)
    }

    pub(crate) fn empty() -> Self {
        Scene {
            prims: Vec::new(),
            lights: Vec::new(),
            vertices: Vec::new(),
            corner_normals: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            extends: Aabb::new(Vec3::new(-14.0, -6.0, -6.0), Vec3::new(28.0, 14.0, 36.0)),
            warnings: Vec::new(),
        }
    }

    fn parse(data: &[u8], path: &Path) -> Result<Self, String> {
        let invalid = || format!("{} is not a valid .frt file", path.display());
        if data.len() < HEADER_SIZE {
            return Err(invalid());
        }
        let int_at = |i: usize| {
            i32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]])
        };
        let counts = |i: usize| usize::try_from(int_at(i)).map_err(|_| invalid());
        let (n_spheres, n_planes, n_meshes) = (counts(0)?, counts(4)?, counts(8)?);
        let expected = n_spheres
            .checked_mul(SPHERE_SIZE + MATERIAL_SIZE)
            .zip(n_planes.checked_mul(PLANE_SIZE + MATERIAL_SIZE))
            .zip(n_meshes.checked_mul(MESH_SIZE + MATERIAL_SIZE))
            .and_then(|((a, b), c)| a.checked_add(b)?.checked_add(c)?.checked_add(HEADER_SIZE))
            .ok_or_else(invalid)?;
        if data.len() < expected {
            return Err(invalid());
        }

        let dir = find_data_folder(path);
        let mut scene = Scene::empty();
        let mut cache = HashMap::new();
        let mut l = Reader { data, pos: HEADER_SIZE };

        let mut spheres = Vec::with_capacity(n_spheres);
        for _ in 0..n_spheres {
            let radius = l.f32();
            let center = l.vector();
            let light = l.bool();
            l.skip(SPHERE_SIZE - 17);
            let m = read_material(&mut l);
            let material = scene.add_material(m, dir.as_deref(), &mut cache);
            spheres.push((Sphere::new(center, radius, material), light));
        }

        let mut planes = Vec::with_capacity(n_planes);
        for _ in 0..n_planes {
            let p = [l.vector(), l.vector(), l.vector(), l.vector()];
            l.skip(PLANE_SIZE - 48);
            let m = read_material(&mut l);
            let material = scene.add_material(m, dir.as_deref(), &mut cache);
            planes.push((p, material));
        }

        for _ in 0..n_meshes {
            let file = l.text(1000);
            l.skip(200);
            let pos = l.vector();
            let target_size = l.f32();
            l.skip(4);
            let m = read_material(&mut l);
            let material = scene.add_material(m, dir.as_deref(), &mut cache);
            scene.add_mesh(&file, dir.as_deref(), material, pos, target_size);
        }

        for (p, material) in planes {
            let base = scene.vertices.len() as u32;
            scene.vertices.extend(p.map(Vertex::new));
            for ids in [[base, base + 1, base + 3], [base + 1, base + 2, base + 3]] {
                let t = Triangle::new(&mut scene.vertices, ids, material);
                scene.corner_normals.push([t.n; 3]);
                scene.prims.push(Primitive::Triangle(t));
            }
        }

        for (e, light) in spheres {
            if light {
                scene.lights.push(e);
            } else {
                scene.corner_normals.push([Vec3::default(); 3]);
                scene.prims.push(Primitive::Sphere(e));
            }
        }
        Ok(scene)
    }

    pub(crate) fn add_material(
        &mut self,
        m: MaterialData,
        dir: Option<&Path>,
        cache: &mut HashMap<String, Option<usize>>,
    ) -> usize {
        let texture = if m.texture.is_empty() {
            None
        } else {
            let name = base_name(&m.texture).to_string();
            match cache.get(&name) {
                Some(t) => *t,
                None => {
                    let t = self.load_texture(&name, dir);
                    cache.insert(name, t);
                    t
                }
            }
        };
        self.materials.push(Material {
            color: m.color,
            refl: m.refl,
            diff: m.diff,
            spec: m.spec,
            refr: m.refr,
            ior: m.ior,
            drefl: m.drefl,
            texture,
            ru_scale: 1.0 / m.u_scale,
            rv_scale: 1.0 / m.v_scale,
        });
        self.materials.len() - 1
    }

    fn load_texture(&mut self, name: &str, dir: Option<&Path>) -> Option<usize> {
        let path = dir.and_then(|d| locate_in(d, IMAGE_FOLDERS, name));
        let result = match path {
            Some(c) => Texture::load(&c),
            None => Err("file not found in the images folder".to_string()),
        };
        match result {
            Ok(t) => {
                self.textures.push(t);
                Some(self.textures.len() - 1)
            }
            Err(e) => {
                self.warnings.push(format!("Skipped texture {name}: {e}"));
                None
            }
        }
    }

    /// Loads a `.3ds` file and places the mesh in the scene.
    pub(crate) fn add_mesh(
        &mut self,
        file: &str,
        dir: Option<&Path>,
        material: usize,
        pos: Vec3,
        target_size: f32,
    ) {
        let name = base_name(file);
        let bytes = dir
            .and_then(|d| locate_in(d, MESH_FOLDERS, name))
            .and_then(|c| fs::read(c).ok());
        let Some(bytes) = bytes else {
            self.warnings.push(format!("Skipped mesh {name}: file not found in the meshes folder"));
            return;
        };
        let mesh = mesh3ds::parse(&bytes);
        if mesh.vertices.is_empty() {
            self.warnings.push(format!("Skipped mesh {name}: no vertices found"));
            return;
        }

        let primeiro = mesh.vertices[0];
        let mut min = Vec3::new(primeiro[0], primeiro[1], primeiro[2]);
        let mut max = min;
        for v in &mesh.vertices[1..] {
            if v[0] < min.x { min.x = v[0]; }
            if v[0] > max.x { max.x = v[0]; }
            if v[1] < min.y { min.y = v[1]; }
            if v[1] > max.y { max.y = v[1]; }
            if v[2] < min.z { min.z = v[2]; }
            if v[2] > max.z { max.z = v[2]; }
        }
        let size = max - min;
        let scale = if size.x > size.y && size.x > size.z {
            target_size / size.x
        } else if size.y > size.z {
            target_size / size.y
        } else {
            target_size / size.z
        };
        let center = (min + max) * 0.5;
        let offset = (center * scale) - pos;

        // 3DS has Z up, so Y and Z are swapped. The center was computed before the
        // swap, which shifts the mesh; the original does the same.
        let base = self.vertices.len() as u32;
        for v in &mesh.vertices {
            let (x, z, y) = (v[0], v[1], v[2]);
            let p = Vec3::new((x * scale) - offset.x, (y * scale) - offset.y, (z * scale) - offset.z);
            self.vertices.push(Vertex { u: p.x / target_size, v: p.y / target_size, ..Vertex::new(p) });
        }

        let n_verts = mesh.vertices.len();
        let mut vertex_faces: Vec<Vec<Vec3>> = vec![Vec::new(); n_verts];
        let mut skipped = 0;
        let first_face = self.prims.len();
        for f in &mesh.faces {
            if f.iter().any(|&i| i as usize >= n_verts) {
                skipped += 1;
                continue;
            }
            let ids = f.map(|i| base + i as u32);
            let t = Triangle::new(&mut self.vertices, ids, material);
            // Original bug (`for v < 1`): a face only counts for its first vertex.
            // Vertices that are never first end up with no faces.
            let list = &mut vertex_faces[f[0] as usize];
            if list.len() < 10 {
                list.push(t.n);
            }
            self.prims.push(Primitive::Triangle(t));
        }
        if skipped > 0 {
            self.warnings.push(format!("Mesh {name}: {skipped} faces with invalid indices"));
        }
        self.compute_corner_normals(first_face);

        for (i, faces) in vertex_faces.iter().enumerate() {
            let mut n = Vec3::default();
            for &f in faces {
                n += f;
            }
            // With no faces this is 0 * infinity = NaN. That invalid normal is what
            // creates the black patches on meshes; it stays for fidelity.
            n *= 1.0 / faces.len() as f32;
            self.vertices[base as usize + i].normal = n;
        }
    }

    /// Computes the smooth normals of a mesh for Enhanced mode.
    ///
    /// Each corner sums the faces that touch the same position, weighted by area.
    /// Vertices at the same position count as one, so no seam shows where the
    /// `.3ds` duplicates vertices. Faces beyond the crease angle are left out, so
    /// sharp edges stay sharp.
    fn compute_corner_normals(&mut self, first_face: usize) {
        const COS_CREASE_ANGLE: f32 = 0.5; // 60 degrees
        let triangles: Vec<&Triangle> = self.prims[first_face..]
            .iter()
            .filter_map(|p| match p {
                Primitive::Triangle(t) => Some(t),
                _ => None,
            })
            .collect();
        let key = |v: u32| {
            let p = self.vertices[v as usize].pos;
            (p.x.to_bits(), p.y.to_bits(), p.z.to_bits())
        };
        let area = |t: &Triangle| {
            let [a, b, c] = t.vertices.map(|i| self.vertices[i as usize].pos);
            (b - a).cross(c - a).length()
        };
        let mut faces_at_position: HashMap<(u32, u32, u32), Vec<(Vec3, f32)>> = HashMap::new();
        for t in &triangles {
            let a = area(t);
            if !a.is_finite() || !t.n.x.is_finite() {
                continue;
            }
            for v in t.vertices {
                faces_at_position.entry(key(v)).or_default().push((t.n, a));
            }
        }
        let normals: Vec<[Vec3; 3]> = triangles
            .iter()
            .map(|t| {
                t.vertices.map(|v| {
                    let mut sum = Vec3::default();
                    for &(n, a) in faces_at_position.get(&key(v)).into_iter().flatten() {
                        if n.dot(t.n) >= COS_CREASE_ANGLE {
                            sum += n * a;
                        }
                    }
                    if sum.dot(sum) > 0.0 {
                        sum.normalize();
                        sum
                    } else {
                        t.n
                    }
                })
            })
            .collect();
        self.corner_normals.extend(normals);
    }

    /// Color of a solid at a point.
    pub fn solid_color(&self, s: &Solid, pos: Vec3) -> Color {
        let m = &self.materials[s.material];
        let Some(texture) = m.texture else {
            return m.color;
        };
        let (u, v) = s.uv(pos);
        self.textures[texture].texel_fixed(u * m.ru_scale, v * m.rv_scale) * m.color
    }

    /// Color of a sphere at a point.
    pub fn sphere_color(&self, e: &Sphere, pos: Vec3) -> Color {
        let m = &self.materials[e.material];
        let Some(t) = m.texture else {
            return m.color;
        };
        let vp = (pos - e.center) * e.inv_radius;
        let phi = (-vp.dot(e.vn)).acos();
        let v = phi * m.rv_scale * (1.0 / PI);
        let theta = (e.ve.dot(vp) / phi.sin()).acos() * (2.0 / PI);
        let u = if e.vc.dot(vp) >= 0.0 {
            (1.0 - theta) * m.ru_scale
        } else {
            theta * m.ru_scale
        };
        self.textures[t].texel(u, v) * m.color
    }

    /// Sphere color in Enhanced mode. Clamping the cosine avoids the NaN the
    /// original produces near the poles, seen as black dots on the texture.
    pub fn sphere_color_fixed(&self, e: &Sphere, pos: Vec3) -> Color {
        let m = &self.materials[e.material];
        let Some(t) = m.texture else {
            return m.color;
        };
        let vp = (pos - e.center) * e.inv_radius;
        let phi = (-vp.dot(e.vn)).clamp(-1.0, 1.0).acos();
        let v = phi * m.rv_scale * (1.0 / PI);
        let sine = phi.sin().max(1e-6);
        let theta = (e.ve.dot(vp) / sine).clamp(-1.0, 1.0).acos() * (2.0 / PI);
        let u = if e.vc.dot(vp) >= 0.0 { (1.0 - theta) * m.ru_scale } else { theta * m.ru_scale };
        self.textures[t].texel_fixed(u, v) * m.color
    }

    /// Triangle color in Enhanced mode.
    pub fn triangle_color_fixed(&self, t: &Triangle, uv: [f32; 2]) -> Color {
        let m = &self.materials[t.material];
        let Some(tex) = m.texture else {
            return m.color;
        };
        let [a, b, c] = t.vertices.map(|i| self.vertices[i as usize]);
        let u = a.u + uv[0] * (b.u - a.u) + uv[1] * (c.u - a.u);
        let v = a.v + uv[0] * (b.v - a.v) + uv[1] * (c.v - a.v);
        self.textures[tex].texel_fixed(u, v) * m.color
    }

    /// Color of a triangle; `uv` holds its barycentric coordinates.
    pub fn triangle_color(&self, t: &Triangle, uv: [f32; 2]) -> Color {
        let m = &self.materials[t.material];
        let Some(tex) = m.texture else {
            return m.color;
        };
        let [a, b, c] = t.vertices.map(|i| self.vertices[i as usize]);
        let u = a.u + uv[0] * (b.u - a.u) + uv[1] * (c.u - a.u);
        let v = a.v + uv[0] * (b.v - a.v) + uv[1] * (c.v - a.v);
        self.textures[tex].texel(u, v) * m.color
    }

    /// Interpolated normal of a triangle.
    pub fn triangle_normal(&self, t: &Triangle, uv: [f32; 2]) -> Vec3 {
        let [a, b, c] = t.vertices.map(|i| self.vertices[i as usize].normal);
        let mut n = a + uv[0] * (b - a) + uv[1] * (c - a);
        n.normalize();
        n
    }
}

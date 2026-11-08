//! Scene: primitives, materials and `.far` file loading.

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

#[derive(Clone)]
pub struct Material {
    pub ggx: Option<crate::ggx::Ggx>,
    pub surface: Option<crate::surface::Surface>,
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
    /// Surface detail from a height or normal map; Enhanced mode only.
    pub bump: Option<crate::bump::Bump>,
    /// A height map that moves the surface itself.
    pub displacement: Option<crate::displace::Displacement>,
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

#[derive(Clone)]
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

    /// Distance to the first crossing of the surface, if it lies in (EPSILON, limit).
    pub fn intersect(&self, origin: Vec3, direction: Vec3, limit: f32) -> Option<f32> {
        let to_center = self.center - origin;
        let b = to_center.dot(direction);
        let disc = b * b - to_center.dot(to_center) + self.sq_radius;
        if disc < 0.0 { return None; }
        let root = disc.sqrt();
        [b - root, b + root].into_iter().find(|&t| t > crate::vector::EPSILON && t < limit)
    }
}

#[derive(Clone)]
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

/// A one-sided rectangular emitter. The full edge vectors must be perpendicular.
#[derive(Clone)]
pub struct RectLight {
    pub proxy: Sphere,
    pub edge_u: Vec3,
    pub edge_v: Vec3,
    pub normal: Vec3,
    pub area: f32,
}

impl RectLight {
    pub fn point(&self, u: f32, v: f32) -> Vec3 {
        self.proxy.center + self.edge_u * (u - 0.5) + self.edge_v * (v - 0.5)
    }

    /// Both sides occlude; only the front side emits.
    pub fn intersect(&self, origin: Vec3, direction: Vec3, limit: f32) -> Option<f32> {
        let denom = direction.dot(self.normal);
        if denom.abs() < 1e-7 { return None; }
        let distance = (self.proxy.center - origin).dot(self.normal) / denom;
        if distance <= crate::vector::EPSILON || distance >= limit { return None; }
        let offset = origin + direction * distance - self.proxy.center;
        let u = offset.dot(self.edge_u) / self.edge_u.dot(self.edge_u);
        let v = offset.dot(self.edge_v) / self.edge_v.dot(self.edge_v);
        (u.abs() <= 0.5 && v.abs() <= 0.5).then_some(distance)
    }
}

/// Size of one texture repeat on the ground, in scene units, before `scale_u` and
/// `scale_v`. It matches the planes, whose texture coordinates are the position over 18.
pub const GROUND_TILE: f32 = 18.0;

/// An infinite plane. It has no bounds, so it stays out of the KD-Tree and every
/// ray tests it on its own: one dot product and one division.
#[derive(Clone)]
pub struct Ground {
    pub point: Vec3,
    /// Unit normal, the side that faces up.
    pub normal: Vec3,
    /// Unit directions along the plane, for the texture.
    pub u_axis: Vec3,
    pub v_axis: Vec3,
    pub material: usize,
}

impl Ground {
    /// `normal` must be nonzero; it does not need unit length.
    pub(crate) fn new(point: Vec3, mut normal: Vec3, material: usize) -> Self {
        normal.normalize();
        // Any direction not parallel to the normal gives a tangent. For the usual
        // upward ground this makes u follow X and v follow Z.
        let helper = if normal.z.abs() < 0.9 { Vec3::new(0.0, 0.0, 1.0) } else { Vec3::new(1.0, 0.0, 0.0) };
        let mut u_axis = normal.cross(helper);
        u_axis.normalize();
        let v_axis = u_axis.cross(normal);
        Self { point, normal, u_axis, v_axis, material }
    }

    /// Both sides are solid: 1 from above, -1 from below, 0 for a miss.
    #[inline(always)]
    pub fn intersect(&self, origin: Vec3, direction: Vec3, dist: &mut f32) -> i32 {
        let denom = direction.dot(self.normal);
        if denom == 0.0 {
            return 0;
        }
        let t = (self.point - origin).dot(self.normal) / denom;
        if t <= crate::vector::EPSILON || t >= *dist {
            return 0;
        }
        *dist = t;
        if denom < 0.0 { 1 } else { -1 }
    }

    /// Texture coordinates before the material scale, one unit per texture repeat.
    pub fn uv(&self, pos: Vec3) -> (f32, f32) {
        let offset = pos - self.point;
        (offset.dot(self.u_axis) / GROUND_TILE, offset.dot(self.v_axis) / GROUND_TILE)
    }
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

#[derive(Clone)]
pub enum Primitive {
    Sphere(Sphere),
    Triangle(Triangle),
    Solid(Solid),
    Ground(Ground),
}

impl Primitive {
    #[inline(always)]
    pub fn material(&self) -> usize {
        match self {
            Primitive::Sphere(e) => e.material,
            Primitive::Triangle(t) => t.material,
            Primitive::Solid(s) => s.material,
            Primitive::Ground(g) => g.material,
        }
    }
}

#[derive(Clone)]
pub struct Scene {
    pub environment: Option<crate::environment::Environment>,
    pub camera: crate::camera::Camera,
    /// Meshes, then planes, then spheres, the order the original uses.
    pub prims: Vec<Primitive>,
    pub lights: Vec<Sphere>,
    pub rect_lights: Vec<RectLight>,
    /// Physical spherical emitters: visible, with inverse-square falloff.
    /// Classic mode treats them as legacy point lights.
    pub bulbs: Vec<Sphere>,
    /// Indices in `prims` of the infinite planes, which the KD-Tree leaves out.
    pub grounds: Vec<usize>,
    /// For each primitive, where a displaced triangle lies on the shape it came from. Empty
    /// when nothing is displaced.
    pub displaced: Vec<Option<crate::displace::DisplacedFace>>,
    /// The original shapes of displaced objects, which `displaced` points into.
    pub displaced_sources: Vec<Primitive>,
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

pub(crate) struct MaterialData {
    pub(crate) surface: Option<crate::surface::Surface>,
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
    /// Image, kind and strength of a bump or normal map.
    pub(crate) bump: Option<(String, crate::bump::BumpKind, f32)>,
    /// Image, height and detail of a displacement map.
    pub(crate) displacement: Option<(String, f32, u32)>,
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
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("frt")) {
            return Err(format!("{}: the binary .frt format is no longer supported. Open the .far version of this scene.", path.display()));
        }
        let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_far(&data, path)
    }

    /// Average position of the objects, a natural point to orbit around.
    pub fn center(&self) -> Option<[f32; 3]> {
        let (mut sum, mut count) = (Vec3::default(), 0);
        for prim in &self.prims {
            sum += match prim {
                Primitive::Sphere(s) => s.center,
                Primitive::Solid(s) => s.center,
                Primitive::Triangle(t) => {
                    let [a, b, c] = t.vertices.map(|v| self.vertices[v as usize].pos);
                    (a + b + c) * (1.0 / 3.0)
                }
                // An infinite plane has no center.
                Primitive::Ground(_) => continue,
            };
            count += 1;
        }
        (count > 0).then(|| (sum * (1.0 / count as f32)).array())
    }

    pub(crate) fn empty() -> Self {
        Scene {
            environment: None,
            camera: crate::camera::Camera::default(),
            prims: Vec::new(),
            lights: Vec::new(),
            rect_lights: Vec::new(),
            bulbs: Vec::new(),
            grounds: Vec::new(),
            vertices: Vec::new(),
            corner_normals: Vec::new(),
            displaced: Vec::new(),
            displaced_sources: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            extends: Aabb::new(Vec3::new(-14.0, -6.0, -6.0), Vec3::new(28.0, 14.0, 36.0)),
            warnings: Vec::new(),
        }
    }

    pub(crate) fn add_material(
        &mut self,
        m: MaterialData,
        dir: Option<&Path>,
        cache: &mut HashMap<String, Option<usize>>,
    ) -> usize {
        let mut problems = Vec::new();
        let mut texture_named = |path: &str| {
            let name = base_name(path).to_string();
            match cache.get(&name) {
                Some(t) => *t,
                None => {
                    let t = self.load_texture(&name, dir);
                    cache.insert(name, t);
                    t
                }
            }
        };
        let texture = (!m.texture.is_empty()).then(|| texture_named(&m.texture)).flatten();
        let bump = match &m.bump {
            Some((map, _, strength)) if !strength.is_finite() || *strength < 0.0 => {
                problems.push(format!("Skipped bump map {map}: its strength must be zero or more"));
                None
            }
            Some((map, kind, strength)) => texture_named(map)
                .map(|texture| crate::bump::Bump { texture, kind: *kind, strength: *strength }),
            None => None,
        };
        let displacement = match &m.displacement {
            Some((map, height, detail)) if !height.is_finite() || *height < 0.0 || *detail < 4 || *detail > 4096 => {
                problems.push(format!(
                    "Skipped displacement map {map}: its height must be zero or more and its detail 4 to 4096"));
                None
            }
            Some((map, height, detail)) => texture_named(map)
                .map(|texture| crate::displace::Displacement { texture, height: *height, detail: *detail }),
            None => None,
        };
        let surface = m.surface;
        let mut material = Material {
            ggx: None,
            surface: None,
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
            bump,
            displacement,
        };
        self.warnings.extend(problems);
        if let Some(surface) = surface { surface.apply(&mut material); }
        self.materials.push(material);
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

    /// Color of the ground at a point.
    pub fn ground_color(&self, g: &Ground, pos: Vec3) -> Color {
        let m = &self.materials[g.material];
        let Some(texture) = m.texture else {
            return m.color;
        };
        let (u, v) = g.uv(pos);
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

    /// Texture coordinates at a point of a primitive, as its Enhanced color lookup reads them,
    /// scale included. The point may lie slightly off the surface. The primitive need not be in
    /// `prims`: displaced shapes keep their original here.
    pub fn texture_uv_on(&self, p: &Primitive, pos: Vec3) -> Option<(f32, f32)> {
        let m = &self.materials[p.material()];
        Some(match p {
            Primitive::Sphere(e) => {
                let mut vp = pos - e.center;
                vp.normalize();
                let phi = (-vp.dot(e.vn)).clamp(-1.0, 1.0).acos();
                let v = phi * m.rv_scale * (1.0 / PI);
                let sine = phi.sin().max(1e-6);
                let theta = (e.ve.dot(vp) / sine).clamp(-1.0, 1.0).acos() * (2.0 / PI);
                let u = if e.vc.dot(vp) >= 0.0 { (1.0 - theta) * m.ru_scale } else { theta * m.ru_scale };
                (u, v)
            }
            Primitive::Solid(s) => {
                let (u, v) = s.uv(pos);
                (u * m.ru_scale, v * m.rv_scale)
            }
            Primitive::Ground(g) => {
                let (u, v) = g.uv(pos);
                (u * m.ru_scale, v * m.rv_scale)
            }
            Primitive::Triangle(t) => {
                // Barycentric coordinates of the point projected onto the triangle's plane.
                let [a, b, c] = t.vertices.map(|i| self.vertices[i as usize]);
                let (e1, e2, rel) = (b.pos - a.pos, c.pos - a.pos, pos - a.pos);
                let (d11, d12, d22) = (e1.dot(e1), e1.dot(e2), e2.dot(e2));
                let (r1, r2) = (rel.dot(e1), rel.dot(e2));
                let det = d11 * d22 - d12 * d12;
                if det.abs() < 1e-20 {
                    return None;
                }
                let s1 = (d22 * r1 - d12 * r2) / det;
                let s2 = (d11 * r2 - d12 * r1) / det;
                (a.u + s1 * (b.u - a.u) + s2 * (c.u - a.u), a.v + s1 * (b.v - a.v) + s2 * (c.v - a.v))
            }
        })
    }

    /// Color of a primitive at a point, with its texture, as Enhanced mode reads it.
    pub fn color_on(&self, p: &Primitive, pos: Vec3) -> Color {
        let m = &self.materials[p.material()];
        match (m.texture, self.texture_uv_on(p, pos)) {
            (Some(texture), Some((u, v))) => self.textures[texture].texel_fixed(u, v) * m.color,
            _ => m.color,
        }
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

//! Path tracing on the GPU, with wgpu: one compute shader (`trace.wgsl`) for Metal, DirectX 12
//! and Vulkan. The scene is packed into flat buffers, with the BVH the CPU walks, and each
//! dispatch adds one camera sample per pixel to a buffer of sums, which is read back as linear
//! light; tone mapping and denoising stay with the CPU engine.
//!
//! It follows the CPU's Enhanced integrator: every shape, the 2008 material, GGX and glass with
//! absorption, color textures, every kind of light with one picked of many, the environment
//! drawn by its light, MIS for GGX reflections, bump and normal maps, fog and smoke, and depth
//! of field, roughness, metallic, emission and alpha maps, and clear coat, sheen and
//! anisotropy, spotlight cones and IES profiles, dispersion in glass, and displacement (the
//! ground marched, objects shaded from the shape under their mesh). `render` reports what a
//! scene uses beyond that (subsurface), and the CPU engine remains the reference.

use bytemuck::{Pod, Zeroable};
use far_core::bvh::Bvh;
use far_core::camera::View;
use far_core::scene::Primitive;
use far_core::solid::Shape;
use far_core::{Options, Scene};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    origin: [f32; 3], width: u32,
    corner: [f32; 3], height: u32,
    dx: [f32; 3], pass_index: u32,
    dy: [f32; 3], max_bounces: u32,
    plane_count: u32, light_count: u32, has_env: u32, env_visible: u32,
    exposure: f32, cap: f32, env_intensity: f32, env_rotation: f32,
    right: [f32; 3], aperture: f32,
    up: [f32; 3], focus: f32,
    forward: [f32; 3], blades: u32,
    blade_rotation: f32, has_glass: u32, strength: f32, clamp: f32,
    max_depth: u32, glossy_max: u32, transmission_max: u32, env_tables: u32,
    stride: u32, features: u32, glow_at: u32, volume_first: u32,
    volume_count: u32,
    /// far-core's spectral::white, which a wavelength's color is divided by.
    white: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Node { low: [f32; 3], first: u32, high: [f32; 3], count: u32 }

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Tri {
    p0: [f32; 3], material: u32,
    e1: [f32; 3], u0: f32,
    e2: [f32; 3], v0: f32,
    n0: [f32; 3], u1: f32,
    n1: [f32; 3], v1: f32,
    n2: [f32; 3], u2: f32,
    v2: f32, pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuShape {
    center: [f32; 3], kind: u32,
    ax: [f32; 3], material: u32,
    ay: [f32; 3], radius: f32,
    az: [f32; 3], pad1: u32,
    half: [f32; 3], pad2: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Material {
    color: [f32; 3], kind: u32,
    emission: [f32; 3], roughness: f32,
    metallic: f32, ior: f32, diff: f32, refl: f32,
    spec: f32, texture: u32, u_scale: f32, v_scale: f32,
    refr: f32, absorption_depth: f32,
    /// The bump or normal map's texture, with the top bit set for a normal map; NO_TEXTURE
    /// without one.
    bump: u32, bump_strength: f32,
    /// Maps that scale the roughness and how metallic a GGX surface is, that tint its
    /// emission, and that cut holes where darker than half gray; NO_TEXTURE where absent.
    roughness_map: u32, metallic_map: u32, emission_map: u32, alpha_map: u32,
    /// GGX layers, as far-core's Ggx: sheen, clear coat, anisotropy and its rotation in radians.
    sheen_color: [f32; 3], sheen: f32,
    clearcoat: f32, clearcoat_roughness: f32, anisotropy: f32, rotation: f32,
    /// The glass's Abbe number, 0 when it does not disperse.
    abbe: f32,
    /// The displacement map and the height white reaches; NO_TEXTURE without one.
    displacement: u32, displacement_height: f32, pad: u32,
    /// The share of diffuse light that walks under a GGX surface, and the mean step there.
    subsurface: f32, subsurface_radius: f32, pad2: [u32; 2],
}

/// In Material::bump, a normal map rather than a height map.
const NORMAL_MAP: u32 = 0x8000_0000;

/// No texture, in Material::texture.
const NO_TEXTURE: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Light {
    pos: [f32; 3], kind: u32,
    color: [f32; 3], radius: f32,
    u: [f32; 3], area: f32,
    v: [f32; 3], pad1: u32,
    normal: [f32; 3], pad2: u32,
}

/// The scene as the shader reads it.
struct Packed {
    nodes: Vec<Node>,
    refs: Vec<u32>,
    tris: Vec<Tri>,
    /// Grounds first, then the shapes the BVH holds, then two entries per volume.
    shapes: Vec<GpuShape>,
    plane_count: u32,
    volume_first: u32,
    volume_count: u32,
    /// For each texture its first texel, width and height, then the texels as RGB bytes.
    texels: Vec<u32>,
    materials: Vec<Material>,
    lights: Vec<Light>,
    /// What the scene uses that this renderer leaves out.
    notes: Vec<String>,
}

const SHAPE: u32 = 0x8000_0000;
/// Volumes the shader takes; its arrays for where a ray crosses them have this size.
const MAX_VOLUMES: usize = 8;

/// Packs a triangle, sphere or solid into `tris` or `shapes`, giving where it went: a triangle
/// index, or a shape index with SHAPE set; u32::MAX for a ground, which goes elsewhere.
fn pack_primitive(scene: &Scene, prim: &Primitive, normals: [far_core::vector::Vec3; 3], tris: &mut Vec<Tri>,
    shapes: &mut Vec<GpuShape>) -> u32 {
    match prim {
        Primitive::Triangle(t) => {
            let [va, vb, vc] = t.vertices.map(|v| scene.vertices[v as usize]);
            let (a, b, c) = (va.pos, vb.pos, vc.pos);
            let normals = normals.map(|n| if n.x.is_finite() && n.y.is_finite() && n.z.is_finite() { n.array() } else { [0.0; 3] });
            tris.push(Tri {
                p0: a.array(), material: t.material as u32,
                e1: (b - a).array(), u0: va.u,
                e2: (c - a).array(), v0: va.v,
                n0: normals[0], u1: vb.u, n1: normals[1], v1: vb.v, n2: normals[2], u2: vc.u,
                v2: vc.v, pad: [0; 3],
            });
            tris.len() as u32 - 1
        }
        Primitive::Sphere(s) => {
            shapes.push(GpuShape {
                center: s.center.array(), kind: 0, ax: [1.0, 0.0, 0.0], material: s.material as u32,
                ay: [0.0, 1.0, 0.0], radius: s.radius, az: [0.0, 0.0, 1.0], pad1: 0, half: [s.radius; 3], pad2: 0,
            });
            SHAPE | (shapes.len() as u32 - 1)
        }
        Primitive::Solid(s) => {
            // The kinds and fields trace.wgsl reads: see its Shape.
            let (kind, radius, half) = match s.shape {
                Shape::Box { half } => (1, 0.0, half.array()),
                Shape::Cylinder { radius, half_height } => (2, radius, [0.0, half_height, 0.0]),
                Shape::Tube { radius, inner_radius, half_height } => (3, radius, [inner_radius, half_height, 0.0]),
                Shape::Cone { radius, half_height } => (4, radius, [0.0, half_height, 0.0]),
                Shape::Torus { major_radius, minor_radius } => (5, major_radius, [minor_radius, minor_radius, 0.0]),
            };
            let [ax, ay, az] = s.axes().map(|a| a.array());
            shapes.push(GpuShape { center: s.center.array(), kind, ax, material: s.material as u32, ay, radius, az, pad1: 0, half, pad2: 0 });
            SHAPE | (shapes.len() as u32 - 1)
        }
        Primitive::Ground(_) => u32::MAX,
    }
}

fn pack(scene: &Scene, bvh: &Bvh) -> Packed {
    let mut notes = Vec::new();
    let mut note = |text: &str| {
        if !notes.iter().any(|n| n == text) {
            notes.push(text.to_string());
        }
    };
    // Where each primitive went: a triangle index, or a shape index with the top bit set.
    let mut slot = vec![u32::MAX; scene.prims.len()];
    let (mut tris, mut shapes) = (Vec::new(), Vec::new());
    // Grounds, as shapes of kind 6 with their texture axes, ahead of the others.
    for prim in &scene.prims {
        if let Primitive::Ground(g) = prim {
            shapes.push(GpuShape {
                center: g.point.array(), kind: 6, ax: g.normal.array(), material: g.material as u32,
                ay: g.u_axis.array(), radius: 0.0, az: g.v_axis.array(), pad1: 0, half: [0.0; 3], pad2: 0,
            });
        }
    }
    let plane_count = shapes.len() as u32;
    for (i, prim) in scene.prims.iter().enumerate() {
        let normals = scene.corner_normals.get(i).copied().unwrap_or_default();
        slot[i] = pack_primitive(scene, prim, normals, &mut tris, &mut shapes);
    }
    let nodes = bvh.nodes.iter().map(|n| Node { low: n.min, first: n.first, high: n.max, count: n.count }).collect();
    let refs = bvh.prims.iter().map(|&p| slot[p as usize]).collect();

    let texture = |map: Option<usize>| map.map_or(NO_TEXTURE, |t| t as u32);
    let materials = scene.materials.iter().map(|m| {
        let emission = m.maps.emission.map_or([0.0; 3], |e| e.color.array());
        let (kind, roughness, metallic) = match (&m.ggx, m.refr > 0.0) {
            (_, true) => (2, m.glass.map_or(0.0, |g| g.roughness), 0.0),
            (Some(ggx), _) => (1, ggx.roughness, ggx.metallic),
            (None, _) => (0, m.drefl, 0.0),
        };
        // A 2008 material may reflect and scatter more light than arrives; the CPU scales its
        // parts down by the same factor.
        let k = if m.ggx.is_none() { 1.0 / (m.diff + m.refl + m.refr).max(1.0).sqrt() } else { 1.0 };
        Material { color: m.color.array(), kind, emission, roughness, metallic, ior: m.ior, diff: m.diff * k, refl: m.refl * k,
            spec: if kind == 0 { m.spec } else { 0.0 }, texture: m.texture.map_or(NO_TEXTURE, |t| t as u32),
            u_scale: m.ru_scale, v_scale: m.rv_scale, refr: m.refr * k,
            absorption_depth: m.glass.and_then(|g| g.absorption_depth).unwrap_or(0.0),
            bump: m.bump.map_or(NO_TEXTURE, |b| {
                b.texture as u32 | if b.kind == far_core::bump::BumpKind::Normal { NORMAL_MAP } else { 0 }
            }),
            bump_strength: m.bump.map_or(0.0, |b| b.strength),
            roughness_map: texture(m.maps.roughness), metallic_map: texture(m.maps.metallic),
            emission_map: texture(m.maps.emission.and_then(|e| e.map)), alpha_map: texture(m.maps.alpha),
            sheen_color: m.ggx.map_or([1.0; 3], |g| g.sheen_color.array()), sheen: m.ggx.map_or(0.0, |g| g.sheen),
            clearcoat: m.ggx.map_or(0.0, |g| g.clearcoat), clearcoat_roughness: m.ggx.map_or(0.03, |g| g.clearcoat_roughness),
            anisotropy: m.ggx.map_or(0.0, |g| g.anisotropy), rotation: m.ggx.map_or(0.0, |g| g.rotation),
            abbe: m.glass.and_then(|g| g.abbe).unwrap_or(0.0),
            displacement: texture(m.displacement.map(|d| d.texture)), displacement_height: m.displacement.map_or(0.0, |d| d.height),
            pad: 0,
            // As the CPU, only a GGX surface walks light under it.
            subsurface: m.subsurface.filter(|_| m.ggx.is_some() && m.refr <= 0.0).map_or(0.0, |s| s.amount),
            subsurface_radius: m.subsurface.map_or(1.0, |s| s.radius), pad2: [0; 2] }
    }).collect();

    let color = |material: usize| scene.materials[material].color.array();
    let mut lights = Vec::new();
    for l in &scene.lights {
        lights.push(Light { pos: l.center.array(), kind: 0, color: color(l.material), radius: l.radius, ..Zeroable::zeroed() });
    }
    // A bulb's profile, as trace.wgsl's bulb_factor reads it: its axis in normal and the IES
    // table's 0° horizontal direction in u; the cone's edge and inner cosines in v; in pad2, bit
    // 0 for a cone and bit 1 for any profile; in pad1, where its IES table starts in the texels.
    let mut ies_tables = Vec::new();
    for (i, l) in scene.bulbs.iter().enumerate() {
        let mut light = Light { pos: l.center.array(), kind: 1, color: color(l.material), radius: l.radius, pad1: NO_TEXTURE, ..Zeroable::zeroed() };
        if let Some(Some(profile)) = scene.bulb_profiles.get(i) {
            light.normal = profile.axis.array();
            light.u = profile.across.array();
            light.pad2 = 2 | profile.cone.is_some() as u32;
            if let Some((outer, inner)) = profile.cone { light.v = [outer, inner, 0.0]; }
            if let Some(ies) = &profile.ies {
                let (vertical, horizontal, values) = ies.table();
                let mut table = vec![vertical.len() as f32, horizontal.len() as f32];
                table.extend(vertical.iter().chain(horizontal).chain(values.iter().flatten()));
                ies_tables.push((lights.len(), table));
            }
        }
        lights.push(light);
    }
    for l in &scene.rect_lights {
        lights.push(Light {
            pos: l.proxy.center.array(), kind: 2, color: color(l.proxy.material), radius: 0.0,
            u: l.edge_u.array(), area: l.area, v: l.edge_v.array(), pad1: 0, normal: l.normal.array(), pad2: 0,
        });
    }

    // Each volume as two entries after the shapes, never reached through the BVH: its shape
    // (kind 0 a box with its axes and half sizes, 1 a sphere), then its medium, as trace.wgsl's
    // volume functions read them.
    let volume_first = shapes.len() as u32;
    if scene.volumes.len() > MAX_VOLUMES { note("volumes past the eighth"); }
    let volumes: Vec<_> = scene.volumes.iter().take(MAX_VOLUMES).collect();
    for v in &volumes {
        shapes.push(match v.shape {
            far_core::volume::Shape::Box { center, axes, half } => GpuShape {
                center: center.array(), kind: 0, ax: axes[0].array(), material: 0, ay: axes[1].array(), radius: 0.0,
                az: axes[2].array(), pad1: 0, half: half.array(), pad2: 0,
            },
            far_core::volume::Shape::Sphere { center, radius } => GpuShape {
                center: center.array(), kind: 1, radius, ..Zeroable::zeroed()
            },
        });
        let (base, scale) = v.falloff.unwrap_or((0.0, 0.0));
        let noise = v.noise.unwrap_or(far_core::volume::Noise { scale: 1.0, octaves: 0, coverage: 0.0, seed: 0 });
        shapes.push(GpuShape {
            center: v.albedo.array(), kind: v.falloff.is_some() as u32 | noise.octaves << 8,
            ax: [v.density.max(0.0), v.anisotropy, base], material: noise.seed,
            ay: [scale, noise.scale, noise.coverage], ..Zeroable::zeroed()
        });
    }
    let volume_count = volumes.len() as u32;

    // Displaced objects, which far-core turned into meshes: the shapes they came from go after
    // the volumes (or after the triangles, for a mesh), out of the BVH's reach. Each displaced
    // triangle points, by one more than the index in its pad[0], to a record after the
    // triangles: in p0, e1 and e2 the corners of the shape under it, in n0 to n2 its normals
    // there, and in material where that shape went.
    let sources: Vec<u32> = scene.displaced_sources.iter()
        .map(|p| pack_primitive(scene, p, [far_core::vector::Vec3::default(); 3], &mut tris, &mut shapes))
        .collect();
    for (i, face) in scene.displaced.iter().enumerate() {
        let Some(face) = face else { continue };
        let [b0, b1, b2] = face.base.map(|v| v.array());
        let [n0, n1, n2] = face.normals.map(|v| v.array());
        tris[slot[i] as usize].pad[0] = tris.len() as u32 + 1;
        tris.push(Tri { p0: b0, material: sources[face.source as usize], e1: b1, e2: b2, n0, n1, n2, ..Zeroable::zeroed() });
    }

    // Texels as the CPU stores them, which are bytes over 256.
    let mut texels: Vec<u32> = Vec::with_capacity(3 * scene.textures.len());
    let mut first = 3 * scene.textures.len() as u32;
    for t in &scene.textures {
        texels.extend([first, t.width as u32, t.height as u32]);
        first += t.image.len() as u32;
    }
    for t in &scene.textures {
        let byte = |x: f32| (x * 256.0).round().clamp(0.0, 255.0) as u32;
        texels.extend(t.image.iter().map(|c| byte(c.x) | byte(c.y) << 8 | byte(c.z) << 16));
    }
    // IES tables after the textures, as float bits.
    for (light, table) in ies_tables {
        lights[light].pad1 = texels.len() as u32;
        texels.extend(table.iter().map(|v| v.to_bits()));
    }
    Packed { nodes, refs, tris, shapes, plane_count, volume_first, volume_count, texels, materials, lights, notes }
}

/// What the scene uses that the GPU renderer leaves out, as `Rendered::notes` reports it.
pub fn leaves_out(scene: &Scene, bvh: &Bvh) -> Vec<String> {
    pack(scene, bvh).notes
}

/// What the samples of a GPU render add up to, per pixel of the whole image, row by row.
pub struct Sums {
    pub width: u32,
    pub height: u32,
    /// Samples each pixel has.
    pub passes: u32,
    /// Exposed light, capped as the CPU caps it.
    pub light: Vec<[f32; 3]>,
    /// The squared luminance of that light.
    pub luminance2: Vec<f32>,
    /// What camera rays met first, when the render was asked to denoise.
    pub primary: Option<Vec<far_core::denoise::Primary>>,
    /// The light over the bloom threshold, when the render was asked for bloom.
    pub glow: Option<Vec<[f32; 3]>>,
}

impl Sums {
    /// Mean linear light of each pixel: what the tone mapping takes.
    pub fn mean(&self) -> Vec<[f32; 3]> {
        let n = self.passes.max(1) as f32;
        self.light.iter().map(|l| [l[0] / n, l[1] / n, l[2] / n]).collect()
    }

    /// The same light, as far-core's display, denoiser and bloom take it.
    pub fn accumulation(&self, op: &Options) -> far_core::enhanced::Accumulation {
        far_core::enhanced::Accumulation::gathered(op, self.passes, &self.light, &self.luminance2,
            self.primary.as_deref(), self.glow.as_deref())
    }
}

/// What a GPU render gives back.
pub struct Rendered {
    pub sums: Sums,
    /// The graphics card and API that rendered.
    pub adapter: String,
    /// What the scene uses that the GPU renderer leaves out for now.
    pub notes: Vec<String>,
    /// It stopped early, and `sums` has the passes done by then.
    pub cancelled: bool,
}

/// A buffer the shader reads; an empty list still needs one element to bind.
fn storage<T: Pod + Zeroable>(device: &wgpu::Device, label: &str, data: &[T]) -> wgpu::Buffer {
    let one = [T::zeroed()];
    let contents = if data.is_empty() { bytemuck::cast_slice(&one) } else { bytemuck::cast_slice(data) };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage: wgpu::BufferUsages::STORAGE })
}

/// Renders `op.samples` passes of the scene on the GPU. `cancel` is checked between passes.
pub fn render(scene: &Scene, bvh: &Bvh, op: &Options, cancel: &dyn Fn() -> bool) -> Result<Rendered, String> {
    Gpu::new()?.render(scene, bvh, op, cancel, None, &mut |_| {})
}

/// The graphics card, ready to render: made once and kept, since compiling the shader takes a
/// moment the first time.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    /// One pipeline per set of features, made on first use: see `Features`.
    pipelines: std::sync::Mutex<std::collections::HashMap<u32, wgpu::ComputePipeline>>,
    adapter: String,
}

/// What a scene uses, for the shader's override constants: the compiler leaves out the code for
/// the rest, as Cycles specializes its kernels.
struct Features(u32);

impl Features {
    /// The constants of trace.wgsl, in the order of their bits.
    const NAMES: [&'static str; 15] = ["HAS_VOLUMES", "HAS_GLASS", "HAS_ENV", "HAS_GGX", "HAS_TEXTURES", "HAS_SOLIDS",
        "HAS_LENS", "HAS_BUMP", "HAS_MAPS", "HAS_ALPHA", "HAS_COATS", "HAS_PROFILES", "HAS_DISPERSION", "HAS_DISPLACEMENT",
        "HAS_SUBSURFACE"];

    fn of(scene: &Scene, packed: &Packed, environment: bool, aperture: f32) -> Self {
        let uses = |m: &dyn Fn(&Material) -> bool| packed.materials.iter().any(m);
        let on = [
            packed.volume_count > 0,
            uses(&|m| m.kind == 2 || m.refr > 0.0),
            environment,
            uses(&|m| m.kind == 1),
            uses(&|m| m.texture != NO_TEXTURE) && !scene.textures.is_empty(),
            packed.shapes.iter().take(packed.volume_first as usize).any(|s| (1..=5).contains(&s.kind)),
            aperture > 0.0,
            uses(&|m| m.bump != NO_TEXTURE),
            uses(&|m| m.roughness_map != NO_TEXTURE || m.metallic_map != NO_TEXTURE || m.emission_map != NO_TEXTURE),
            uses(&|m| m.alpha_map != NO_TEXTURE),
            uses(&|m| m.kind == 1 && (m.clearcoat > 0.0 || m.sheen > 0.0 || m.anisotropy != 0.0)),
            packed.lights.iter().any(|l| l.pad2 & 2 != 0),
            uses(&|m| m.kind == 2 && m.abbe > 0.0),
            uses(&|m| m.displacement != NO_TEXTURE),
            uses(&|m| m.subsurface > 0.0),
        ];
        Self(on.iter().enumerate().map(|(i, &b)| (b as u32) << i).sum())
    }

    fn constants(&self) -> Vec<(&'static str, f64)> {
        Self::NAMES.iter().enumerate().map(|(i, &name)| (name, (self.0 >> i & 1) as f64)).collect()
    }
}

impl Gpu {
    /// The fastest graphics card, or why there is none to render with.
    pub fn new() -> Result<Self, String> {
        pollster::block_on(Self::open())
    }

    /// The graphics card and API, such as "Apple M2 (Metal)".
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    async fn open() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })
            .await
            .map_err(|e| format!("no graphics card for rendering: {e}"))?;
        let info = adapter.get_info();
        // A software renderer (WARP, llvmpipe) is slower than the CPU engine, and WARP has
        // crashed on this shader.
        if info.device_type == wgpu::DeviceType::Cpu {
            return Err(format!("no graphics card for rendering: only a software renderer ({})", info.name));
        }
        let limits = adapter.limits();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("far-gpu"),
                required_limits: wgpu::Limits {
                    max_storage_buffers_per_shader_stage: 8,
                    max_storage_buffer_binding_size: limits.max_storage_buffer_binding_size,
                    max_buffer_size: limits.max_buffer_size,
                    ..wgpu::Limits::default()
                },
                ..Default::default()
            })
            .await
            .map_err(|e| format!("the graphics card refused the renderer: {e}"))?;

        let buffer_entry = |binding: u32, ty: wgpu::BufferBindingType| wgpu::BindGroupLayoutEntry {
            binding, visibility: wgpu::ShaderStages::COMPUTE, count: None,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
        };
        let mut layout_entries = vec![buffer_entry(0, wgpu::BufferBindingType::Uniform)];
        for binding in 1..=7 {
            layout_entries.push(buffer_entry(binding, wgpu::BufferBindingType::Storage { read_only: true }));
        }
        layout_entries.push(buffer_entry(8, wgpu::BufferBindingType::Storage { read_only: false }));
        layout_entries.push(wgpu::BindGroupLayoutEntry {
            binding: 9, visibility: wgpu::ShaderStages::COMPUTE, count: None,
            // Read texel by texel, so the float texture needs no filtering support.
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2, multisampled: false,
            },
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("trace"), entries: &layout_entries });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("trace"), source: wgpu::ShaderSource::Wgsl(include_str!("trace.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("trace"), bind_group_layouts: &[Some(&layout)], immediate_size: 0,
        });
        Ok(Self {
            device, queue, layout, shader, pipeline_layout, pipelines: Default::default(),
            adapter: format!("{} ({:?})", info.name, info.backend),
        })
    }

    /// The shader compiled for `features`, kept for later renders.
    fn pipeline(&self, features: &Features) -> wgpu::ComputePipeline {
        let mut pipelines = self.pipelines.lock().unwrap();
        pipelines.entry(features.0).or_insert_with(|| {
            self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("trace"), layout: Some(&self.pipeline_layout), module: &self.shader, entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions { constants: &features.constants(), ..Default::default() },
                cache: None,
            })
        }).clone()
    }

    /// Renders `op.samples` passes of the whole image; `op.region` and adaptive sampling are
    /// left to the CPU. `cancel` is checked between passes. With `frames`, `on_frame` gets the
    /// sums so far about that often, for a progressive display.
    pub fn render(&self, scene: &Scene, bvh: &Bvh, op: &Options, cancel: &dyn Fn() -> bool,
        frames: Option<std::time::Duration>, on_frame: &mut dyn FnMut(&Sums)) -> Result<Rendered, String> {
        let (device, queue) = (&self.device, &self.queue);
        let packed = pack(scene, bvh);
        let (w, h) = (op.width, op.height);
        let pose = op.pose.unwrap_or(scene.camera.pose());
        let view = View::new(&pose, w, h);
        let environment = scene.environment.as_ref().filter(|e| e.active());
        let bounces = far_core::Bounces::resolve(scene, op);
        // The light per pixel, then the first hits for the denoiser, then the light for bloom.
        let features = op.denoise;
        let glow_at = if op.bloom { 1 + 2 * features as u32 } else { 0 };
        let stride = 1 + 2 * features as u32 + op.bloom as u32;
        let mut params = Params {
            origin: view.origin.array(), width: w,
            corner: view.plane_point(-1.0, 1.0).array(), height: h,
            dx: (view.right * (2.0 * view.half_width / w as f32)).array(), pass_index: 0,
            dy: (view.up * (-2.0 * view.half_height / h as f32)).array(),
            max_bounces: if op.indirect { bounces.max } else { 0 },
            plane_count: packed.plane_count, light_count: packed.lights.len() as u32,
            has_env: environment.is_some() as u32,
            env_visible: environment.is_some_and(|e| e.config.visible) as u32,
            // The CPU's compensation for 2008 lights, which add up with no falloff.
            exposure: op.exposure / (scene.lights.len().max(1) as f32).powf(0.35),
            cap: if environment.is_some() { 1e9 } else { 6.0 },
            env_intensity: environment.map_or(0.0, |e| e.config.intensity),
            env_rotation: environment.map_or(0.0, |e| e.config.rotation.rem_euclid(360.0).to_radians()),
            right: view.right.array(), aperture: op.aperture.unwrap_or(scene.camera.aperture),
            up: view.up.array(), focus: op.focus_distance.unwrap_or(scene.camera.focus_distance),
            forward: view.forward.array(), blades: op.blades.unwrap_or(scene.camera.blades),
            blade_rotation: op.blade_rotation.unwrap_or(scene.camera.blade_rotation).to_radians(),
            has_glass: scene.materials.iter().any(|m| m.refr > 0.0) as u32,
            strength: bounces.strength, clamp: bounces.clamp,
            // Mirror-like and glass segments of a path, as the CPU counts depth from 1.
            max_depth: op.max_depth.max(1) as u32 - 1,
            glossy_max: op.glossy_depth.map_or(u32::MAX, |d| d.max(0) as u32),
            transmission_max: op.transmission_depth.map_or(u32::MAX, |d| d.max(0) as u32),
            env_tables: 0,
            stride, features: features as u32, glow_at,
            volume_first: packed.volume_first, volume_count: packed.volume_count,
            white: far_core::spectral::white().array(),
        };

        // The environment's sampling tables go after the texels, as float bits.
        let uses = Features::of(scene, &packed, environment.is_some(), params.aperture);
        let mut texels = packed.texels;
        if let Some(e) = environment {
            params.env_tables = texels.len() as u32;
            texels.extend(e.map.sampling_tables().iter().map(|v| v.to_bits()));
        }

        // The environment as a texture; a single black texel when there is none.
        let (env_w, env_h, env_data): (u32, u32, Vec<f32>) = match environment {
            Some(e) => {
                let (width, height, pixels) = e.map.pixels();
                (width as u32, height as u32, pixels.iter().flat_map(|p| [p.x, p.y, p.z, 1.0]).collect())
            }
            None => (1, 1, vec![0.0, 0.0, 0.0, 1.0]),
        };
        let env_size = wgpu::Extent3d { width: env_w, height: env_h, depth_or_array_layers: 1 };
        let env = device.create_texture_with_data(queue, &wgpu::TextureDescriptor {
            label: Some("environment"), size: env_size, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
        }, wgpu::util::TextureDataOrder::LayerMajor, bytemuck::cast_slice(&env_data));
        let env_view = env.create_view(&wgpu::TextureViewDescriptor::default());
        let pipeline = self.pipeline(&uses);

        let pixels = (w as usize) * (h as usize);
        let accum_size = (pixels * 16 * stride as usize) as u64;
        let accum = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("accum"), size: accum_size, mapped_at_creation: false,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"), size: accum_size, mapped_at_creation: false,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        });
        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"), size: std::mem::size_of::<Params>() as u64, mapped_at_creation: false,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let buffers = [
            storage(device, "nodes", &packed.nodes),
            storage(device, "refs", &packed.refs),
            storage(device, "tris", &packed.tris),
            storage(device, "shapes", &packed.shapes),
            storage(device, "texels", &texels),
            storage(device, "materials", &packed.materials),
            storage(device, "lights", &packed.lights),
        ];
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: params_buffer.as_entire_binding() }];
        for (i, buffer) in buffers.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry { binding: i as u32 + 1, resource: buffer.as_entire_binding() });
        }
        entries.push(wgpu::BindGroupEntry { binding: 8, resource: accum.as_entire_binding() });
        entries.push(wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&env_view) });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("trace"), layout: &self.layout, entries: &entries });

        // Copies the sums so far back from the graphics card.
        let read = |passes: u32| -> Result<Sums, String> {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            encoder.copy_buffer_to_buffer(&accum, 0, &readback, 0, accum_size);
            queue.submit([encoder.finish()]);
            let slice = readback.slice(..);
            let (sender, receiver) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| { let _ = sender.send(result); });
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            receiver.recv().map_err(|e| e.to_string())?.map_err(|e| format!("could not read the image back: {e}"))?;
            let data = slice.get_mapped_range().map_err(|e| format!("could not read the image back: {e}"))?;
            let cells: &[[f32; 4]] = bytemuck::cast_slice(&data);
            let pixel = |p: usize| &cells[p * stride as usize..(p + 1) * stride as usize];
            let sums = Sums {
                width: w, height: h, passes,
                light: (0..pixels).map(|p| { let c = pixel(p)[0]; [c[0], c[1], c[2]] }).collect(),
                luminance2: (0..pixels).map(|p| pixel(p)[0][3]).collect(),
                primary: features.then(|| (0..pixels).map(|p| {
                    let (a, n) = (pixel(p)[1], pixel(p)[2]);
                    far_core::denoise::Primary { albedo: [a[0], a[1], a[2]], normal: [n[0], n[1], n[2]], depth: a[3] }
                }).collect()),
                glow: (glow_at > 0).then(|| (0..pixels).map(|p| { let g = pixel(p)[glow_at as usize]; [g[0], g[1], g[2]] }).collect()),
            };
            drop(data);
            readback.unmap();
            Ok(sums)
        };

        let samples = op.samples.max(1);
        let mut done = 0;
        let mut last_frame = std::time::Instant::now();
        let mut cancelled = false;
        // One pass per submission: a long command is killed by the system's GPU watchdog.
        while done < samples {
            if cancel() {
                cancelled = true;
                break;
            }
            params.pass_index = done;
            queue.write_buffer(&params_buffer, 0, bytemuck::bytes_of(&params));
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
            }
            queue.submit([encoder.finish()]);
            done += 1;
            // Keeps the queue short, so memory and the watchdog see one pass at a time.
            if done % 8 == 0 {
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            }
            if frames.is_some_and(|every| done < samples && last_frame.elapsed() >= every) {
                on_frame(&read(done)?);
                last_frame = std::time::Instant::now();
            }
        }
        Ok(Rendered { sums: read(done)?, adapter: self.adapter.clone(), notes: packed.notes, cancelled })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use far_core::{Image, KdTree, Mode, Progress};
    use std::path::Path;

    /// Mean brightness of the CPU's and the GPU's render of a scene, or None without a GPU.
    fn brightness(name: &str) -> Option<(f32, f32)> {
        let scene = Scene::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes").join(name)).unwrap();
        let op = Options { mode: Mode::Enhanced, width: 96, height: 72, samples: 256, ..Options::default() };
        let rendered = match render(&scene, &Bvh::build(&scene), &op, &|| false) {
            Ok(rendered) => rendered,
            Err(e) if e.contains("no graphics card") => {
                eprintln!("skipped: {e}");
                return None;
            }
            Err(e) => panic!("{name}: {e}"),
        };
        let mean = rendered.sums.mean();
        let gpu: f32 = mean.iter()
            .map(|&m| far_core::tonemap::display(op.tone_mapping, m).iter().sum::<f32>() / 3.0)
            .sum::<f32>() / mean.len() as f32;
        let image = Image::new(op.width, op.height);
        far_core::render(&scene, Some(&KdTree::build_corrected(&scene)), &op, &image, &Progress::default());
        let rgba = image.rgba();
        let cpu = rgba.chunks_exact(4).map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 765.0).sum::<f32>()
            / (rgba.len() / 4) as f32;
        Some((cpu, gpu))
    }

    #[test]
    fn the_gpu_lights_a_scene_as_the_cpu_does() {
        // 2008 lights, panels, solids, glass, GGX, textures with many lights, and the sky.
        for name in ["cornell-box.far", "2008/scene-b.far", "techniques/rectangular-lights.far", "techniques/primitives.far",
            "materials/glass.far", "materials/ggx-materials.far", "christmas.far", "techniques/sun-and-sky.far",
            // Even fog, and smoke shaped by noise; height and normal maps.
            "techniques/light-shafts.far", "techniques/smoke.far", "materials/bump-maps.far",
            // Roughness, metallic, emission and alpha maps; clear coat, sheen and anisotropy.
            "materials/maps.far", "materials/coatings.far",
            // Spotlight cones and an IES profile, in fog; glass that disperses.
            "techniques/spotlights.far", "materials/glass.far",
            // A displaced ground, spheres and a box; light walking under surfaces; and the art
            // example, with glass holding wine.
            "materials/displacement.far", "materials/subsurface.far", "still-life.far"] {
            let Some((cpu, gpu)) = brightness(name) else { return };
            assert!(cpu > 0.02, "{name}: the CPU image is black");
            assert!((gpu - cpu).abs() < 0.05 * cpu + 0.005, "{name}: CPU {cpu} against GPU {gpu}");
        }
    }

    #[test]
    fn what_the_gpu_leaves_out_is_reported() {
        // Every example renders on the GPU now; more than eight volumes is what it leaves out.
        let mut scene = Scene::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes/techniques/smoke.far")).unwrap();
        let volume = scene.volumes[0];
        scene.volumes.resize(9, volume);
        let notes = pack(&scene, &Bvh::build(&scene)).notes;
        assert!(notes.iter().any(|n| n.contains("volumes")), "{notes:?}");
    }
}

//! Path tracing on the GPU, with wgpu: one compute shader (`trace.wgsl`) for Metal, DirectX 12
//! and Vulkan. The scene is packed into flat buffers, with the BVH the CPU walks, and each
//! dispatch adds one camera sample per pixel to a buffer of sums, which is read back as linear
//! light; tone mapping and denoising stay with the CPU engine.
//!
//! This is the first cut, for previews: triangles, spheres, boxes and grounds; diffuse, GGX-like
//! and glass materials without textures; every kind of light; the environment by bounces only.
//! `render` reports what a scene uses beyond that, and the CPU engine remains the reference.

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
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Node { low: [f32; 3], first: u32, high: [f32; 3], count: u32 }

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Tri {
    p0: [f32; 3], material: u32,
    e1: [f32; 3], pad1: u32,
    e2: [f32; 3], pad2: u32,
    n0: [f32; 3], pad3: u32,
    n1: [f32; 3], pad4: u32,
    n2: [f32; 3], pad5: u32,
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
struct Plane { point: [f32; 3], material: u32, normal: [f32; 3], pad: u32 }

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Material {
    color: [f32; 3], kind: u32,
    emission: [f32; 3], roughness: f32,
    metallic: f32, ior: f32, diff: f32, refl: f32,
    spec: f32, pad: [u32; 3],
}

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
    shapes: Vec<GpuShape>,
    planes: Vec<Plane>,
    materials: Vec<Material>,
    lights: Vec<Light>,
    /// What the scene uses that this renderer leaves out.
    notes: Vec<String>,
}

const SHAPE: u32 = 0x8000_0000;

fn pack(scene: &Scene, bvh: &Bvh) -> Result<Packed, String> {
    let mut notes = Vec::new();
    let mut note = |text: &str| {
        if !notes.iter().any(|n| n == text) {
            notes.push(text.to_string());
        }
    };
    // Where each primitive went: a triangle index, or a shape index with the top bit set.
    let mut slot = vec![u32::MAX; scene.prims.len()];
    let (mut tris, mut shapes, mut planes) = (Vec::new(), Vec::new(), Vec::new());
    for (i, prim) in scene.prims.iter().enumerate() {
        match prim {
            Primitive::Triangle(t) => {
                let [a, b, c] = t.vertices.map(|v| scene.vertices[v as usize].pos);
                let normals = scene.corner_normals.get(i).copied().unwrap_or_default().map(|n| {
                    if n.x.is_finite() && n.y.is_finite() && n.z.is_finite() { n.array() } else { [0.0; 3] }
                });
                slot[i] = tris.len() as u32;
                tris.push(Tri {
                    p0: a.array(), material: t.material as u32,
                    e1: (b - a).array(), pad1: 0,
                    e2: (c - a).array(), pad2: 0,
                    n0: normals[0], pad3: 0, n1: normals[1], pad4: 0, n2: normals[2], pad5: 0,
                });
            }
            Primitive::Sphere(s) => {
                slot[i] = SHAPE | shapes.len() as u32;
                shapes.push(GpuShape {
                    center: s.center.array(), kind: 0, ax: [1.0, 0.0, 0.0], material: s.material as u32,
                    ay: [0.0, 1.0, 0.0], radius: s.radius, az: [0.0, 0.0, 1.0], pad1: 0, half: [s.radius; 3], pad2: 0,
                });
            }
            Primitive::Solid(s) => match s.shape {
                Shape::Box { half } => {
                    let [ax, ay, az] = s.axes().map(|a| a.array());
                    slot[i] = SHAPE | shapes.len() as u32;
                    shapes.push(GpuShape {
                        center: s.center.array(), kind: 1, ax, material: s.material as u32,
                        ay, radius: 0.0, az, pad1: 0, half: half.array(), pad2: 0,
                    });
                }
                _ => return Err("the GPU renderer does not trace cylinders, tubes, cones and tori yet".into()),
            },
            Primitive::Ground(g) => planes.push(Plane {
                point: g.point.array(), material: g.material as u32, normal: g.normal.array(), pad: 0,
            }),
        }
    }
    let nodes = bvh.nodes.iter().map(|n| Node { low: n.min, first: n.first, high: n.max, count: n.count }).collect();
    let refs = bvh.prims.iter().map(|&p| slot[p as usize]).collect();

    let materials = scene.materials.iter().map(|m| {
        if m.texture.is_some() || m.maps != Default::default() { note("textures and maps"); }
        if m.bump.is_some() { note("bump and normal maps"); }
        if m.subsurface.is_some() { note("subsurface scattering"); }
        if m.displacement.is_some() { note("displacement of the ground"); }
        let emission = m.maps.emission.map_or([0.0; 3], |e| e.color.array());
        let (kind, roughness, metallic) = match (&m.ggx, m.refr > 0.0) {
            (_, true) => (2, m.glass.map_or(0.0, |g| g.roughness), 0.0),
            (Some(ggx), _) => {
                if ggx.clearcoat > 0.0 || ggx.sheen > 0.0 || ggx.anisotropy != 0.0 { note("clear coat, sheen and anisotropy"); }
                (1, ggx.roughness, ggx.metallic)
            }
            (None, _) => (0, m.drefl, 0.0),
        };
        // A 2008 material may reflect and scatter more light than arrives; the CPU scales its
        // parts down by the same factor.
        let k = if kind == 0 { 1.0 / (m.diff + m.refl + m.refr).max(1.0).sqrt() } else { 1.0 };
        Material { color: m.color.array(), kind, emission, roughness, metallic, ior: m.ior, diff: m.diff * k, refl: m.refl * k,
            spec: if kind == 0 { m.spec } else { 0.0 }, pad: [0; 3] }
    }).collect();

    let color = |material: usize| scene.materials[material].color.array();
    let mut lights = Vec::new();
    for l in &scene.lights {
        lights.push(Light { pos: l.center.array(), kind: 0, color: color(l.material), radius: l.radius, ..Zeroable::zeroed() });
    }
    for (i, l) in scene.bulbs.iter().enumerate() {
        if scene.bulb_profiles.get(i).is_some_and(|p| p.is_some()) { note("spotlight cones and IES profiles"); }
        lights.push(Light { pos: l.center.array(), kind: 1, color: color(l.material), radius: l.radius, ..Zeroable::zeroed() });
    }
    for l in &scene.rect_lights {
        lights.push(Light {
            pos: l.proxy.center.array(), kind: 2, color: color(l.proxy.material), radius: 0.0,
            u: l.edge_u.array(), area: l.area, v: l.edge_v.array(), pad1: 0, normal: l.normal.array(), pad2: 0,
        });
    }
    if !scene.volumes.is_empty() { note("fog and volumes"); }
    Ok(Packed { nodes, refs, tris, shapes, planes, materials, lights, notes })
}

/// What a GPU render gives back.
pub struct Rendered {
    /// Mean linear light of each pixel, exposed, row by row: what the tone mapping takes.
    pub mean: Vec<[f32; 3]>,
    /// The graphics card and API that rendered.
    pub adapter: String,
    /// What the scene uses that the GPU renderer leaves out for now.
    pub notes: Vec<String>,
}

/// A buffer the shader reads; an empty list still needs one element to bind.
fn storage<T: Pod + Zeroable>(device: &wgpu::Device, label: &str, data: &[T]) -> wgpu::Buffer {
    let one = [T::zeroed()];
    let contents = if data.is_empty() { bytemuck::cast_slice(&one) } else { bytemuck::cast_slice(data) };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage: wgpu::BufferUsages::STORAGE })
}

/// Renders `op.samples` passes of the scene on the GPU. `cancel` is checked between passes.
pub fn render(scene: &Scene, bvh: &Bvh, op: &Options, cancel: &dyn Fn() -> bool) -> Result<Rendered, String> {
    pollster::block_on(render_async(scene, bvh, op, cancel))
}

async fn render_async(scene: &Scene, bvh: &Bvh, op: &Options, cancel: &dyn Fn() -> bool) -> Result<Rendered, String> {
    let packed = pack(scene, bvh)?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })
        .await
        .map_err(|e| format!("no graphics card for rendering: {e}"))?;
    let info = adapter.get_info();
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

    let (w, h) = (op.width, op.height);
    let pose = op.pose.unwrap_or(scene.camera.pose());
    let view = View::new(&pose, w, h);
    let environment = scene.environment.as_ref().filter(|e| e.active());
    let mut params = Params {
        origin: view.origin.array(), width: w,
        corner: view.plane_point(-1.0, 1.0).array(), height: h,
        dx: (view.right * (2.0 * view.half_width / w as f32)).array(), pass_index: 0,
        dy: (view.up * (-2.0 * view.half_height / h as f32)).array(),
        max_bounces: op.max_bounces.unwrap_or(far_core::Bounces::for_scene(scene).max).min(64),
        plane_count: packed.planes.len() as u32, light_count: packed.lights.len() as u32,
        has_env: environment.is_some() as u32,
        env_visible: environment.is_some_and(|e| e.config.visible) as u32,
        // The CPU's compensation for 2008 lights, which add up with no falloff.
        exposure: op.exposure / (scene.lights.len().max(1) as f32).powf(0.35),
        cap: if environment.is_some() { 1e9 } else { 6.0 },
        env_intensity: environment.map_or(0.0, |e| e.config.intensity),
        env_rotation: environment.map_or(0.0, |e| e.config.rotation.rem_euclid(360.0).to_radians()),
    };

    // The environment as a texture; a single black texel when there is none.
    let (env_w, env_h, env_data): (u32, u32, Vec<f32>) = match environment {
        Some(e) => {
            let (width, height, pixels) = e.map.pixels();
            (width as u32, height as u32, pixels.iter().flat_map(|p| [p.x, p.y, p.z, 1.0]).collect())
        }
        None => (1, 1, vec![0.0, 0.0, 0.0, 1.0]),
    };
    let env_size = wgpu::Extent3d { width: env_w, height: env_h, depth_or_array_layers: 1 };
    let env = device.create_texture_with_data(&queue, &wgpu::TextureDescriptor {
        label: Some("environment"), size: env_size, mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
    }, wgpu::util::TextureDataOrder::LayerMajor, bytemuck::cast_slice(&env_data));
    let env_view = env.create_view(&wgpu::TextureViewDescriptor::default());

    let pixels = (w as usize) * (h as usize);
    let accum_size = (pixels * 16) as u64;
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
        storage(&device, "nodes", &packed.nodes),
        storage(&device, "refs", &packed.refs),
        storage(&device, "tris", &packed.tris),
        storage(&device, "shapes", &packed.shapes),
        storage(&device, "planes", &packed.planes),
        storage(&device, "materials", &packed.materials),
        storage(&device, "lights", &packed.lights),
    ];

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
    let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: params_buffer.as_entire_binding() }];
    for (i, buffer) in buffers.iter().enumerate() {
        entries.push(wgpu::BindGroupEntry { binding: i as u32 + 1, resource: buffer.as_entire_binding() });
    }
    entries.push(wgpu::BindGroupEntry { binding: 8, resource: accum.as_entire_binding() });
    entries.push(wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&env_view) });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("trace"), layout: &layout, entries: &entries });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("trace"), source: wgpu::ShaderSource::Wgsl(include_str!("trace.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("trace"), bind_group_layouts: &[Some(&layout)], immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("trace"), layout: Some(&pipeline_layout), module: &shader, entry_point: Some("main"),
        compilation_options: Default::default(), cache: None,
    });

    // One pass per submission: a long command is killed by the system's GPU watchdog.
    for pass_index in 0..op.samples.max(1) {
        if cancel() {
            return Err("cancelled".into());
        }
        params.pass_index = pass_index;
        queue.write_buffer(&params_buffer, 0, bytemuck::bytes_of(&params));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        }
        queue.submit([encoder.finish()]);
        // Keeps the queue short, so memory and the watchdog see one pass at a time.
        if pass_index % 8 == 7 {
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        }
    }

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_buffer_to_buffer(&accum, 0, &readback, 0, accum_size);
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| { let _ = sender.send(result); });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    receiver.recv().map_err(|e| e.to_string())?.map_err(|e| format!("could not read the image back: {e}"))?;
    let data = slice.get_mapped_range().map_err(|e| format!("could not read the image back: {e}"))?;
    let sums: &[[f32; 4]] = bytemuck::cast_slice(&data);
    let mean = sums.iter().map(|s| {
        let n = s[3].max(1.0);
        [s[0] / n, s[1] / n, s[2] / n]
    }).collect();
    drop(data);
    readback.unmap();
    Ok(Rendered { mean, adapter: format!("{} ({:?})", info.name, info.backend), notes: packed.notes })
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
        let gpu: f32 = rendered.mean.iter()
            .map(|&m| far_core::tonemap::display(op.tone_mapping, m).iter().sum::<f32>() / 3.0)
            .sum::<f32>() / rendered.mean.len() as f32;
        let image = Image::new(op.width, op.height);
        far_core::render(&scene, Some(&KdTree::build_corrected(&scene)), &op, &image, &Progress::default());
        let rgba = image.rgba();
        let cpu = rgba.chunks_exact(4).map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 765.0).sum::<f32>()
            / (rgba.len() / 4) as f32;
        Some((cpu, gpu))
    }

    #[test]
    fn the_gpu_lights_a_scene_as_the_cpu_does() {
        for name in ["cornell-box.far", "2008/scene-b.far", "techniques/rectangular-lights.far"] {
            let Some((cpu, gpu)) = brightness(name) else { return };
            assert!(cpu > 0.02, "{name}: the CPU image is black");
            assert!((gpu - cpu).abs() < 0.08 * cpu + 0.005, "{name}: CPU {cpu} against GPU {gpu}");
        }
    }

    #[test]
    fn unsupported_shapes_are_refused_with_a_reason() {
        let scene = Scene::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes/techniques/primitives.far")).unwrap();
        let op = Options { width: 16, height: 12, samples: 1, ..Options::default() };
        let error = render(&scene, &Bvh::build(&scene), &op, &|| false).err().unwrap();
        assert!(error.contains("cylinders"), "{error}");
    }
}

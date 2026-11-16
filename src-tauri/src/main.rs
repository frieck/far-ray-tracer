//! Far Ray Tracer desktop app: opens a scene and shows the render as it happens.
//!
//! The engine writes to a shared image and this layer sends a copy of it to the
//! window about 30 times per second. Each message has a 24-byte header (kind,
//! tiles done, total tiles, milliseconds, current pass, total passes) followed by
//! the RGBA pixels.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use far_core::environment::{Environment, EnvironmentConfig, EnvironmentMap};
use far_core::{render, Image, KdTree, Mode, Options, Progress, Scene};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, Response};
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

mod updater;

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
/// Larger images go out less often, so sending them stays a small part of the work.
const PIXELS_PER_FRAME_INTERVAL: u32 = 1_000_000;
const MAX_IMAGE_SIDE: u32 = 8192;
const MESSAGE_BUILDING_KD: u32 = 0;
const MESSAGE_FRAME: u32 = 1;
const MESSAGE_PHOTON_MAP: u32 = 2;
/// How often a GPU render reads its image back to show it: each read stalls the card a little.
const GPU_FRAME_INTERVAL: Duration = Duration::from_millis(200);
const HEADER_SIZE: usize = 24;
const MENU_OPEN: &str = "open";
const MENU_SAVE: &str = "save";
const MENU_SAVE_SCENE: &str = "save_scene";
const MENU_SAVE_SCENE_AS: &str = "save_scene_as";
const MENU_RENDER: &str = "render";
const MENU_EXAMPLE: &str = "example:";
const MENU_ABOUT: &str = "about";
const MENU_UNDO: &str = "undo";
const MENU_REDO: &str = "redo";
const REPOSITORY_URL: &str = "https://github.com/frieck/far-ray-tracer";

struct OpenScene {
    scene: Scene,
    /// The file the scene was read from, which the paths inside it are relative to.
    path: PathBuf,
    /// Where Save writes: the file itself, or where it was last saved. None until it is
    /// saved, when the file cannot be written in place (a bundled example or a model).
    save_to: Mutex<Option<PathBuf>>,
    /// Built on first use: Classic mode needs the original tree, the others the corrected one.
    kd_classic: OnceLock<KdTree>,
    kd: OnceLock<KdTree>,
    /// The BVH the GPU renderer walks, built on its first render.
    bvh: OnceLock<far_core::bvh::Bvh>,
    /// The scene as the interface last changed it, edited and with what it hides left out, by
    /// the key of those changes; see `OpenScene::view`.
    view: Mutex<Option<(String, Arc<Prepared>)>>,
    /// The scene with only the selected object, for its outline behind others.
    alone: Mutex<Option<(String, Arc<Prepared>)>>,
}

/// A scene to render and the trees built for it on first use.
struct Prepared {
    scene: Scene,
    kd_classic: OnceLock<KdTree>,
    kd: OnceLock<KdTree>,
    bvh: OnceLock<far_core::bvh::Bvh>,
}

/// What the interface changes in the scene itself: objects and lights it hides, by their
/// indices in the file, and edits to their entries.
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
struct SceneView {
    hidden_objects: Vec<usize>,
    hidden_lights: Vec<usize>,
    entries: far_core::scene_save::EntryEdits,
}

impl SceneView {
    /// What is left out: what the outliner hides and what was deleted, sorted.
    fn hidden(&self) -> (Vec<usize>, Vec<usize>) {
        let merged = |a: &[usize], b: &[usize]| {
            let mut items: Vec<usize> = a.iter().chain(b).copied().collect();
            items.sort_unstable();
            items.dedup();
            items
        };
        (merged(&self.hidden_objects, &self.entries.removed_objects), merged(&self.hidden_lights, &self.entries.removed_lights))
    }

    /// The lights left in the scene, by the index the file and the added ones give them.
    fn shown_lights(&self, scene: &Scene) -> Vec<u32> {
        let count = scene.scene_lights.len() + self.entries.added_lights.len();
        let hidden = self.hidden().1;
        (0..count as u32).filter(|l| !hidden.contains(&(*l as usize))).collect()
    }
}

impl OpenScene {
    /// The scene as `view` changes it, edited and with what it hides left out, and its trees;
    /// None when it changes nothing. The last one is kept, so a new image of the same view, a
    /// click or an outline reuse it.
    fn view(&self, view: &SceneView) -> Result<Option<Arc<Prepared>>, String> {
        let (mut objects, mut lights) = view.hidden();
        let edited = !view.entries.is_empty();
        if !edited {
            // Without added entries, only the open scene's indices mean anything.
            objects.retain(|&o| o < self.scene.objects.len());
            lights.retain(|&l| l < self.scene.scene_lights.len());
        }
        if !edited && objects.is_empty() && lights.is_empty() {
            return Ok(None);
        }
        let key = serde_json::to_string(&(&view.entries, &objects, &lights)).map_err(|e| e.to_string())?;
        let mut cache = self.view.lock().unwrap();
        if let Some((kept, prepared)) = cache.as_ref() {
            if *kept == key {
                return Ok(Some(prepared.clone()));
            }
        }
        let base = if edited { Some(self.scene.edited(&self.path, &view.entries)?) } else { None };
        let counted = base.as_ref().unwrap_or(&self.scene);
        // Indices past the scene's, the added entries counted, mean nothing.
        let objects: Vec<usize> = objects.into_iter().filter(|&o| o < counted.objects.len()).collect();
        let lights: Vec<usize> = lights.into_iter().filter(|&l| l < counted.scene_lights.len()).collect();
        let scene = match base {
            Some(scene) if objects.is_empty() && lights.is_empty() => scene,
            _ => counted.without(&objects, &lights),
        };
        let prepared = Arc::new(Prepared { scene, kd_classic: OnceLock::new(), kd: OnceLock::new(), bvh: OnceLock::new() });
        *cache = Some((key, prepared.clone()));
        Ok(Some(prepared))
    }

    /// The scene, edited as `entries` say, with only object `object` and no lights: what of it
    /// lies behind others.
    fn alone(&self, entries: &far_core::scene_save::EntryEdits, object: usize) -> Result<Arc<Prepared>, String> {
        let key = serde_json::to_string(&(entries, object)).map_err(|e| e.to_string())?;
        let mut cache = self.alone.lock().unwrap();
        if let Some((kept, prepared)) = cache.as_ref() {
            if *kept == key {
                return Ok(prepared.clone());
            }
        }
        let edited = if entries.is_empty() { None } else { Some(self.scene.edited(&self.path, entries)?) };
        let base = edited.as_ref().unwrap_or(&self.scene);
        if object >= base.objects.len() {
            return Err("Unknown object".into());
        }
        let others: Vec<usize> = (0..base.objects.len()).filter(|&o| o != object).collect();
        let lights: Vec<usize> = (0..base.scene_lights.len()).collect();
        let prepared = Arc::new(Prepared {
            scene: base.without(&others, &lights), kd_classic: OnceLock::new(), kd: OnceLock::new(), bvh: OnceLock::new(),
        });
        *cache = Some((key, prepared.clone()));
        Ok(prepared)
    }

    /// The scene to work on for `view`, and its trees.
    fn parts<'a>(&'a self, prepared: &'a Option<Arc<Prepared>>) -> (&'a Scene, &'a OnceLock<KdTree>, &'a OnceLock<KdTree>, &'a OnceLock<far_core::bvh::Bvh>) {
        match prepared {
            Some(p) => (&p.scene, &p.kd_classic, &p.kd, &p.bvh),
            None => (&self.scene, &self.kd_classic, &self.kd, &self.bvh),
        }
    }
}

/// Largest side of an object ID map.
const MAX_ID_SIDE: u32 = 4096;

fn id_map_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || width > MAX_ID_SIDE || height > MAX_ID_SIDE {
        return Err(format!("An ID map is 1 to {MAX_ID_SIDE} pixels a side"));
    }
    Ok(())
}

/// An object of the scene file, as the outliner lists it.
#[derive(Serialize)]
struct ObjectInfo {
    name: String,
    kind: &'static str,
    /// Its materials' ids, as in `SceneInfo::materials`.
    materials: Vec<usize>,
    /// Its entry in the file: the values the Object tab edits, and what a duplicate copies.
    source: serde_json::Value,
}

/// A light of the scene file.
#[derive(Serialize)]
struct LightInfo {
    name: String,
    kind: &'static str,
    /// Its entry in the file.
    source: serde_json::Value,
}

/// What a click on the image found: an object or a light, by its index.
#[derive(Serialize, PartialEq, Debug)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
enum PickedInfo {
    Object(usize),
    Light(usize),
}

fn neutral_white() -> f32 {
    far_core::lens::NEUTRAL_KELVIN
}

fn reference_iso() -> f32 {
    far_core::lens::REFERENCE_ISO
}

fn full() -> f32 {
    1.0
}

struct FinishedImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    /// The options it was shown with (exposure, white balance and vignette), when the kept
    /// accumulation holds its light, which can then be saved as EXR.
    shown_with: Option<Options>,
}

#[derive(Default)]
struct AppState {
    environment_cache: Mutex<Option<(String, Arc<EnvironmentMap>)>>,
    /// The last LUT read, by path and the time its file changed.
    lut_cache: Mutex<Option<(PathBuf, Option<std::time::SystemTime>, Arc<far_core::lut::Lut>)>>,
    scene: Mutex<Option<Arc<OpenScene>>>,
    progress: Arc<Progress>,
    rendering: AtomicBool,
    last_image: Mutex<Option<FinishedImage>>,
    /// What the last kept render accumulated, with the settings it depends on: the next render
    /// continues from it when only its passes grow, or shows it again when only exposure, tone
    /// mapping, denoise or bloom changed.
    accumulation: Mutex<Option<(String, far_core::enhanced::Accumulation)>>,
    /// The scene has edits not saved, as the interface last said.
    unsaved: AtomicBool,
    /// Quitting was confirmed, so the exit request goes through.
    quitting: AtomicBool,
    /// The folder of example scenes, bundled or in the checkout; they are saved elsewhere.
    examples: Mutex<Option<PathBuf>>,
    /// The studio environment that lights material previews, loaded on first use.
    studio: OnceLock<Option<Environment>>,
    /// The material preview being rendered, so a newer one can cancel it.
    /// The material previews being rendered, by view, so a newer one in a view can cancel it.
    previews: Mutex<std::collections::HashMap<String, Arc<Progress>>>,
    /// The graphics card, opened on the first render that asks for it, or why it cannot render.
    gpu: OnceLock<Result<far_gpu::Gpu, String>>,
}

#[derive(Serialize)]
struct SceneMaterial {
    id: usize,
    label: String,
    color: [f32; 3],
    ior: f32,
    surface: Option<far_core::surface::Surface>,
    bump: Option<BumpInfo>,
    /// The glow, as a color whose brightest channel is 1 and a strength; None for none.
    emission: Option<far_core::surface::EmissionEdit>,
}

#[derive(Serialize)]
struct BumpInfo {
    kind: far_core::bump::BumpKind,
    strength: f32,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct MaterialOverride {
    id: usize,
    #[serde(default)]
    surface: Option<far_core::surface::Surface>,
    #[serde(default)]
    bump_strength: Option<f32>,
    #[serde(default)]
    emission: Option<far_core::surface::EmissionEdit>,
}

impl MaterialOverride {
    fn edit(&self) -> far_core::surface::MaterialEdit {
        far_core::surface::MaterialEdit { surface: self.surface.clone(), bump_strength: self.bump_strength, emission: self.emission }
    }
}

#[derive(Serialize)]
struct SceneInfo {
    environment: Option<EnvironmentConfig>,
    materials: Vec<SceneMaterial>,
    camera: far_core::Camera,
    center: Option<[f32; 3]>,
    name: String,
    path: String,
    primitives: usize,
    lights: usize,
    /// The file's objects and lights, in its order, for the outliner.
    objects: Vec<ObjectInfo>,
    light_list: Vec<LightInfo>,
    warnings: Vec<String>,
    /// The automatic indirect light settings for this scene's lights.
    bounces: BounceInfo,
    /// The app's render settings saved with the scene, if any.
    render: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct BounceInfo {
    strength: f32,
    max: u32,
    clamp: f32,
}

struct SceneEntry {
    name: String,
    path: String,
    group: String,
}

#[derive(Serialize)]
struct Startup {
    cores: usize,
    initial_scene: Option<String>,
}

/// A menu choice, handled by the interface.
#[derive(Clone, Serialize)]
struct MenuAction {
    action: &'static str,
    path: Option<String>,
}

#[derive(Deserialize)]
struct RenderRequest {
    #[serde(default)]
    environment: Option<EnvironmentConfig>,
    #[serde(default)]
    material_overrides: Vec<MaterialOverride>,
    pose: far_core::Pose,
    aperture: f32,
    focus_distance: f32,
    #[serde(default)]
    blades: u32,
    #[serde(default)]
    blade_rotation: f32,
    mode: String,
    width: u32,
    height: u32,
    depth: i32,
    /// Limits within `depth` for reflections and for glass crossings; absent follows `depth`.
    #[serde(default)]
    glossy_depth: Option<i32>,
    #[serde(default)]
    transmission_depth: Option<i32>,
    threads: usize,
    use_kd: bool,
    samples: u32,
    indirect: bool,
    /// Indirect light overrides; absent means automatic. A clamp of 0 means no limit.
    #[serde(default)]
    indirect_strength: Option<f32>,
    #[serde(default)]
    max_bounces: Option<u32>,
    #[serde(default)]
    indirect_clamp: Option<f32>,
    denoise: bool,
    #[serde(default)]
    bloom: bool,
    exposure: f32,
    #[serde(default)]
    tone_mapping: far_core::ToneMapping,
    /// The camera's white balance in kelvin and its tint, vignetting, chromatic aberration,
    /// film grain from 0 to 1 at the camera's ISO, and halation. `exposure` already holds what
    /// the ISO and shutter make of the light.
    #[serde(default = "neutral_white")]
    white_balance: f32,
    #[serde(default)]
    tint: f32,
    #[serde(default)]
    vignette: f32,
    #[serde(default)]
    chromatic_aberration: f32,
    #[serde(default)]
    grain: f32,
    #[serde(default = "reference_iso")]
    iso: f32,
    #[serde(default)]
    halation: f32,
    /// A color look graded over the finished picture: a bundled look by name, such as
    /// `warm-film`, or the path of a .cube file; and how strongly, from 0 to 1.
    #[serde(default)]
    lut: Option<String>,
    #[serde(default = "full")]
    lut_strength: f32,
    /// What the outliner hides: indices in the scene's objects and lights.
    #[serde(default)]
    hidden_objects: Vec<usize>,
    #[serde(default)]
    hidden_lights: Vec<usize>,
    /// Moved or retuned objects and lights.
    #[serde(default)]
    entries: far_core::scene_save::EntryEdits,
    caustics: bool,
    photons: u32,
    caustic_radius: f32,
    caustic_strength: f32,
    caustics_only: bool,
    /// The image the Save command should write. Comparison renders do not count.
    keep: bool,
    /// Renders only this rectangle, in pixels: [x0, y0, x1, y1], the end excluded.
    #[serde(default)]
    region: Option<[u32; 4]>,
    /// Adaptive sampling: a pixel stops once its error is below this; 0 samples evenly.
    #[serde(default)]
    noise_threshold: f32,
    #[serde(default)]
    device: Device,
}

/// Where Enhanced renders run. Auto takes the graphics card for scenes it renders as the CPU
/// does, and the CPU for the rest; the other stages always run on the CPU. With caustics, the
/// CPU traces the photons before a GPU render.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "snake_case")]
enum Device {
    Auto,
    Gpu,
    #[default]
    Cpu,
}

#[derive(Serialize)]
struct CausticReport {
    emitted: u32,
    stored: usize,
    trace_ms: u64,
    memory_bytes: usize,
    /// Reads of the map, and the photons each used on average: counted on the CPU only.
    lookups: Option<u64>,
    photons_per_lookup: Option<f64>,
}

#[derive(Serialize)]
struct RenderResult {
    kd_ms: Option<u64>,
    render_ms: u64,
    rays: u64,
    cancelled: bool,
    caustics: Option<CausticReport>,
    /// The denoiser the render used, if any.
    denoiser: Option<far_core::engine::Denoiser>,
    /// The image is the last render shown again with new exposure, tone mapping, denoise or bloom.
    redisplayed: bool,
    /// The render continued from this many passes of the last one.
    continued_from: Option<u32>,
    /// The graphics card that rendered, when it was not the CPU.
    gpu: Option<String>,
    /// What the scene uses that the graphics card left out.
    gpu_notes: Vec<String>,
    /// Why the CPU rendered although the graphics card was asked for.
    cpu_reason: Option<String>,
}

/// Entry `index` of the scene file's `list`, or null: what the Object tab edits, and what a
/// duplicate copies, its material included.
fn entry(scene: &Scene, list: &str, index: usize) -> serde_json::Value {
    scene.source.as_deref().and_then(|s| s.get(list)).and_then(|l| l.get(index)).cloned().unwrap_or_default()
}

fn open_scene(path: &Path, state: &AppState) -> Result<SceneInfo, String> {
    let scene = Scene::open(path)?;
    let mut seen = std::collections::HashSet::new();
    let materials = scene.prims.iter().filter_map(|primitive| {
        let id = primitive.material();
        if !seen.insert(id) { return None; }
        let kind = match primitive {
            far_core::scene::Primitive::Sphere(_) => "sphere",
            far_core::scene::Primitive::Triangle(_) => "surface / mesh",
            far_core::scene::Primitive::Solid(_) => "solid",
            far_core::scene::Primitive::Ground(_) => "ground",
        };
        let m = &scene.materials[id];
        // Named by its object: "Ball", or "teapot part 2" for a model's.
        let label = scene.objects.iter().find(|o| o.materials.contains(&id)).map(|o| match o.materials.len() {
            1 => o.name.clone(),
            _ => format!("{} part {}", o.name, id - o.materials.start + 1),
        }).unwrap_or_else(|| format!("Material {} ({kind})", seen.len()));
        Some(SceneMaterial { id, label, color: m.color.array(), ior: m.ior, surface: m.surface.clone(),
            bump: m.bump.map(|b| BumpInfo { kind: b.kind, strength: b.strength }),
            emission: far_core::surface::EmissionEdit::of(m) })
    }).collect();
    let info = SceneInfo {
        environment: scene.environment.as_ref().map(|e| e.config.clone()),
        materials,
        camera: scene.camera,
        center: scene.center(),
        name: path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path: path.to_string_lossy().into_owned(),
        primitives: scene.prims.len(),
        lights: scene.lights.len() + scene.rect_lights.len() + scene.bulbs.len(),
        objects: scene.objects.iter().enumerate().map(|(i, o)| ObjectInfo {
            name: o.name.clone(), kind: o.kind, materials: o.materials.clone().collect(), source: entry(&scene, "objects", i),
        }).collect(),
        light_list: scene.scene_lights.iter().enumerate().map(|(i, l)| LightInfo {
            name: l.name.clone(), kind: l.kind, source: entry(&scene, "lights", i),
        }).collect(),
        warnings: scene.warnings.clone(),
        bounces: {
            let auto = far_core::Bounces::for_scene(&scene);
            BounceInfo { strength: auto.strength, max: auto.max, clamp: auto.clamp }
        },
        render: scene.render_settings.clone(),
    };
    let save_to = writable_in_place(path, app_resources(state)).then(|| path.to_path_buf());
    *state.scene.lock().unwrap() = Some(Arc::new(OpenScene {
        scene, path: path.to_path_buf(), save_to: Mutex::new(save_to), kd_classic: OnceLock::new(), kd: OnceLock::new(), bvh: OnceLock::new(),
        view: Mutex::new(None), alone: Mutex::new(None),
    }));
    Ok(info)
}

/// The scenes folder: the one inside the app bundle or, in development, the repository's.
fn scenes_folder(app: &AppHandle) -> Option<PathBuf> {
    let bundled = app.path().resource_dir().ok().map(|d| d.join("assets/scenes"));
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/scenes");
    bundled.into_iter().chain([repository]).find(|p| p.is_dir())
}

/// Loads Intel Open Image Denoise from the app bundle, or from the checkout in development, on a
/// thread of its own: it takes a moment, and until it is ready the engine's own filter denoises.
fn load_denoiser(app: &AppHandle) {
    let bundled = app.path().resource_dir().ok().map(|d| d.join("oidn/lib"));
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("oidn/lib");
    let folder = bundled.into_iter().chain([repository]).find(|p| p.is_dir());
    if let Some(folder) = folder {
        std::thread::spawn(move || {
            if let Err(e) = far_core::oidn::load(&folder) {
                eprintln!("Denoising without Open Image Denoise: {e}");
            }
        });
    }
}

/// Example folders and their submenu titles, in menu order.
const EXAMPLE_GROUPS: [(&str, &str); 3] = [("materials", "Materials"), ("techniques", "Techniques"), ("2008", "From 2008")];
/// Folders of scenes that only the tests and the validation against the original use.
const HIDDEN_EXAMPLES: [&str; 2] = ["debug", "validation"];

fn collect_scenes(folder: &Path, group: &str, scenes: &mut Vec<SceneEntry>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for path in entries.flatten().map(|e| e.path()) {
        let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if path.is_dir() {
            if !HIDDEN_EXAMPLES.contains(&name.as_str()) {
                collect_scenes(&path, &name, scenes);
            }
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("far")) {
            let path = path.to_string_lossy().into_owned();
            scenes.push(SceneEntry { name, path, group: group.to_string() });
        }
    }
}

fn example_scenes(app: &AppHandle) -> Vec<SceneEntry> {
    let mut scenes = Vec::new();
    if let Some(folder) = scenes_folder(app) {
        collect_scenes(&folder, "", &mut scenes);
    }
    scenes.sort_by(|a, b| (&a.group, &a.name).cmp(&(&b.group, &b.name)));
    scenes
}

/// Turns a file name such as `plane-and-mesh` into `Plane and mesh`, keeping acronyms such
/// as GGX, HDRI and EXR in capitals.
fn pretty_name(name: &str) -> String {
    const ACRONYMS: [&str; 6] = ["ggx", "hdri", "hdr", "exr", "png", "jpeg"];
    let words: Vec<String> = name.split('-').map(|word| {
        if ACRONYMS.contains(&word) { word.to_uppercase() } else { word.to_string() }
    }).collect();
    let text = words.join(" ");
    let mut letters = text.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => text,
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let scenes = example_scenes(app);
    let item = |scene: &SceneEntry| {
        MenuItemBuilder::with_id(format!("{MENU_EXAMPLE}{}", scene.path), pretty_name(&scene.name)).build(app)
    };

    let mut examples = SubmenuBuilder::new(app, "Open Example");
    for scene in scenes.iter().filter(|s| s.group.is_empty()) {
        examples = examples.item(&item(scene)?);
    }
    // One submenu per folder, in this order; folders not listed follow, by name.
    let mut groups: Vec<&str> = scenes.iter().map(|s| s.group.as_str()).filter(|g| !g.is_empty()).collect();
    groups.dedup();
    groups.sort_by_key(|g| (EXAMPLE_GROUPS.iter().position(|(folder, _)| folder == g).unwrap_or(usize::MAX), *g));
    if !groups.is_empty() {
        examples = examples.separator();
    }
    for group in groups {
        let title = EXAMPLE_GROUPS.iter().find(|(folder, _)| *folder == group)
            .map_or_else(|| pretty_name(group), |(_, title)| title.to_string());
        let mut submenu = SubmenuBuilder::new(app, title);
        for scene in scenes.iter().filter(|s| s.group == group) {
            submenu = submenu.item(&item(scene)?);
        }
        examples = examples.item(&submenu.build()?);
    }

    let open = MenuItemBuilder::with_id(MENU_OPEN, "Open Scene…").accelerator("CmdOrCtrl+O").build(app)?;
    let save_scene = MenuItemBuilder::with_id(MENU_SAVE_SCENE, "Save Scene").accelerator("CmdOrCtrl+S").build(app)?;
    let save_scene_as = MenuItemBuilder::with_id(MENU_SAVE_SCENE_AS, "Save Scene As…").accelerator("CmdOrCtrl+Shift+S").build(app)?;
    let save = MenuItemBuilder::with_id(MENU_SAVE, "Export Image…").accelerator("CmdOrCtrl+E").build(app)?;
    let render = MenuItemBuilder::with_id(MENU_RENDER, "Render").accelerator("CmdOrCtrl+R").build(app)?;
    // The app's own undo, of scene and render edits; in a text field it undoes the typing.
    let undo = MenuItemBuilder::with_id(MENU_UNDO, "Undo").accelerator("CmdOrCtrl+Z").build(app)?;
    let redo = MenuItemBuilder::with_id(MENU_REDO, "Redo").accelerator("Shift+CmdOrCtrl+Z").build(app)?;
    let about = MenuItemBuilder::with_id(MENU_ABOUT, "About Far Ray Tracer").build(app)?;
    let check_updates = MenuItemBuilder::with_id(updater::MENU_CHECK_UPDATES, "Check for Updates…").build(app)?;

    // macOS keeps About in the application menu; elsewhere it goes under Help.
    let application = SubmenuBuilder::new(app, "Far Ray Tracer");
    #[cfg(target_os = "macos")]
    let application = application.item(&about).item(&check_updates).separator();
    let application = application
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let file = SubmenuBuilder::new(app, "File")
        .item(&open)
        .item(&examples.build()?)
        .separator()
        .item(&save_scene)
        .item(&save_scene_as)
        .item(&save)
        .separator()
        .close_window()
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .item(&undo)
        .item(&redo)
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let render_menu = SubmenuBuilder::new(app, "Render").item(&render).build()?;
    let view = SubmenuBuilder::new(app, "View").fullscreen().build()?;
    let window = SubmenuBuilder::new(app, "Window").minimize().maximize().build()?;
    let menu = MenuBuilder::new(app).items(&[&application, &file, &edit, &render_menu, &view, &window]);
    #[cfg(not(target_os = "macos"))]
    let menu = menu.item(&SubmenuBuilder::new(app, "Help").item(&check_updates).separator().item(&about).build()?);
    menu.build()
}

fn on_menu(app: &AppHandle, id: &str) {
    if id == updater::MENU_CHECK_UPDATES {
        return updater::check(app, false);
    }
    if id == MENU_ABOUT {
        let _ = app.emit("show-about", ());
        return;
    }
    let action = match id {
        MENU_OPEN => MenuAction { action: "open", path: None },
        MENU_SAVE => MenuAction { action: "save", path: None },
        MENU_SAVE_SCENE => MenuAction { action: "save_scene", path: None },
        MENU_SAVE_SCENE_AS => MenuAction { action: "save_scene_as", path: None },
        MENU_RENDER => MenuAction { action: "render", path: None },
        MENU_UNDO => MenuAction { action: "undo", path: None },
        MENU_REDO => MenuAction { action: "redo", path: None },
        _ => match id.strip_prefix(MENU_EXAMPLE) {
            Some(path) => MenuAction { action: "example", path: Some(path.to_string()) },
            None => return,
        },
    };
    let _ = app.emit("menu-action", action);
}

/// Opens the project's GitHub page in the default browser.
#[tauri::command]
fn open_repository() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    command.arg(REPOSITORY_URL).spawn().map(|_| ()).map_err(|error| error.to_string())
}

#[tauri::command]
fn startup(app: AppHandle) -> Startup {
    let default_scene = example_scenes(&app).into_iter().find(|s| s.name == "cube").map(|s| s.path);
    Startup {
        cores: std::thread::available_parallelism().map_or(1, |n| n.get()),
        initial_scene: std::env::args().nth(1).or(default_scene),
    }
}

#[tauri::command(async)]
fn choose_scene(app: AppHandle, state: State<'_, AppState>) -> Result<Option<SceneInfo>, String> {
    // A model opens on its own in a small studio; see `Scene::open`.
    let mut dialog = app.dialog().file()
        .add_filter("Scenes and models", &["far", "obj", "gltf", "glb"])
        .add_filter("Far Ray Tracer scenes", &["far"])
        .add_filter("Models (OBJ, glTF)", &["obj", "gltf", "glb"]);
    if let Some(folder) = scenes_folder(&app) {
        dialog = dialog.set_directory(folder);
    }
    let Some(file) = dialog.blocking_pick_file() else { return Ok(None) };
    let path = file.into_path().map_err(|e| e.to_string())?;
    guarded("open the scene", || open_scene(&path, &state)).map(Some)
}

#[tauri::command(async)]
fn choose_environment(app: AppHandle, state: State<'_, AppState>) -> Result<Option<EnvironmentConfig>, String> {
    let Some(file) = app.dialog().file().add_filter("Environment image", &["hdr", "exr", "png", "jpg", "jpeg"]).blocking_pick_file() else {
        return Ok(None);
    };
    let path = file.into_path().map_err(|e| e.to_string())?;
    let config = EnvironmentConfig { path: path.to_string_lossy().into_owned(), ..EnvironmentConfig::default() };
    // Decode and build the sampling table once; slider edits reuse the shared map.
    let environment = Environment::load(config.clone())?;
    *state.environment_cache.lock().unwrap() = Some((config.map_key(), environment.map));
    Ok(Some(config))
}

/// Picks a .cube LUT and reads it, so a broken file is reported at once; gives its path.
#[tauri::command(async)]
fn choose_lut(app: AppHandle, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let Some(file) = app.dialog().file().add_filter("LUT", &["cube"]).blocking_pick_file() else {
        return Ok(None);
    };
    let path = file.into_path().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
    load_lut(&app, &state, &path)?;
    Ok(Some(path))
}

/// The folder of bundled looks, or the checkout's in development.
fn luts_folder(app: &AppHandle) -> Option<PathBuf> {
    let bundled = app.path().resource_dir().ok().map(|d| d.join("assets/luts"));
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/luts");
    bundled.into_iter().chain([repository]).find(|p| p.is_dir())
}

/// Where a LUT a render asks for is: a bundled look's name, such as `warm-film`, or a path.
fn lut_path(luts: Option<PathBuf>, name: &str) -> Result<PathBuf, String> {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Ok(luts.ok_or("The bundled looks are missing")?.join(format!("{name}.cube")));
    }
    Ok(PathBuf::from(name))
}

/// Reads a LUT, or takes the one read last while its file is unchanged: slider edits show the
/// picture again many times a second.
fn load_lut(app: &AppHandle, state: &AppState, name: &str) -> Result<Arc<far_core::lut::Lut>, String> {
    let path = lut_path(luts_folder(app), name)?;
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut cache = state.lut_cache.lock().unwrap();
    if let Some((cached, time, lut)) = cache.as_ref() {
        if *cached == path && *time == modified {
            return Ok(lut.clone());
        }
    }
    let lut = Arc::new(far_core::lut::Lut::open(&path)?);
    *cache = Some((path, modified, lut.clone()));
    Ok(lut)
}

/// Largest side of a material preview, in pixels.
const MAX_PREVIEW_SIDE: u32 = 2048;

/// The bundled studio environment, or the repository's in development.
fn studio_environment(app: &AppHandle) -> Option<Environment> {
    let bundled = app.path().resource_dir().ok().map(|d| d.join("assets/environments/studio.hdr"));
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/environments/studio.hdr");
    let path = bundled.into_iter().chain([repository]).find(|p| p.is_file())?;
    let config = EnvironmentConfig { path: path.to_string_lossy().into_owned(), rotation: 140.0, ..EnvironmentConfig::default() };
    Environment::load(config).ok()
}

/// Most samples a material preview may take.
const MAX_PREVIEW_SAMPLES: u32 = 256;

/// Where the material ball is seen from, for orbiting it.
#[tauri::command]
fn material_ball_pose() -> far_core::Pose {
    far_core::preview::MATERIAL_BALL_POSE
}

/// Renders one of the open scene's materials, with an optional edit, on a shader ball and
/// returns RGBA. A newer preview in the same view cancels this one, which then returns an error.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn material_preview(
    id: usize,
    surface: Option<far_core::surface::Surface>,
    bump_strength: Option<f32>,
    emission: Option<far_core::surface::EmissionEdit>,
    width: u32,
    height: u32,
    samples: u32,
    pose: Option<far_core::Pose>,
    view: String,
    device: Option<Device>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let device = device.unwrap_or_default();
    let valid_side = 1..=MAX_PREVIEW_SIDE;
    if !valid_side.contains(&width) || !valid_side.contains(&height) || !(1..=MAX_PREVIEW_SAMPLES).contains(&samples) {
        return Err(format!("Previews need 1 to {MAX_PREVIEW_SIDE} pixels and 1 to {MAX_PREVIEW_SAMPLES} samples"));
    }
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    let environment = state.studio.get_or_init(|| studio_environment(&app)).clone();
    let data = scenes_folder(&app).unwrap_or_default();
    let edit = far_core::surface::MaterialEdit { surface, bump_strength, emission };
    let ball = far_core::preview::material_ball(&open.scene, id, Some(&edit), environment, &data)?;
    if let Some(pose) = &pose {
        pose.validate()?;
    }
    let progress = Arc::new(Progress::default());
    if let Some(previous) = state.previews.lock().unwrap().insert(view, progress.clone()) {
        previous.cancel.store(true, Relaxed);
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    // While a render runs, half the cores keep the preview from stalling it.
    let threads = if state.rendering.load(Relaxed) { (cores / 2).max(1) } else { cores };
    let options = Options {
        mode: Mode::Enhanced,
        pose,
        width,
        height,
        samples,
        denoise: true,
        max_depth: 6,
        threads,
        ..Options::default()
    };
    let image = Image::new(width, height);
    // The graphics card renders the ball as it would the scene: Auto leaves the CPU a material
    // it would render differently, and a failed GPU preview falls back to the CPU.
    let gpu = match device {
        Device::Gpu | Device::Auto => state.gpu.get_or_init(far_gpu::Gpu::new).as_ref().ok(),
        Device::Cpu => None,
    };
    let on_gpu = guarded("render the material preview", || {
        Ok(gpu.is_some_and(|gpu| ball_on_gpu(gpu, &ball, &options, device == Device::Auto, &image, &progress)))
    })?;
    if !on_gpu && !progress.cancel.load(Relaxed) {
        // A displaced ball is a dense mesh; the few shapes of a plain one are quicker without a tree.
        let kd = (ball.prims.len() >= far_core::preview::BALL_TREE_FROM).then(|| KdTree::build_corrected(&ball));
        let options = Options { use_kd: kd.is_some(), ..options };
        guarded("render the material preview", || {
            render(&ball, kd.as_ref(), &options, &image, &progress);
            Ok(())
        })?;
    }
    if progress.cancel.load(Relaxed) {
        return Err("Replaced by a newer preview".into());
    }
    Ok(Response::new(image.rgba()))
}

/// Renders the material ball on the graphics card into `image`, denoised as on the CPU. Gives
/// false, rendering nothing, when the card failed or, with `strict`, leaves out something the
/// ball uses, so the CPU renders it instead.
fn ball_on_gpu(gpu: &far_gpu::Gpu, ball: &Scene, options: &Options, strict: bool, image: &Image, progress: &Progress) -> bool {
    let bvh = far_core::bvh::Bvh::build(ball);
    if strict && !far_gpu::leaves_out(ball, &bvh).is_empty() {
        return false;
    }
    let cancel = || progress.cancel.load(Relaxed);
    match gpu.render(ball, &bvh, options, None, None, &cancel, None, &mut |_| {}) {
        Ok(rendered) => {
            if !rendered.cancelled {
                far_core::enhanced::redisplay(&rendered.sums.accumulation(options), ball, options, image, progress);
            }
            true
        }
        Err(e) => {
            eprintln!("The GPU material preview failed, so the CPU renders it: {e}");
            false
        }
    }
}

fn render_environment(config: &EnvironmentConfig, scene: Option<&Scene>, state: &AppState) -> Result<Environment, String> {
    config.validate()?;
    if let Some(environment) = scene.and_then(|scene| scene.environment.as_ref()) {
        if environment.config.map_key() == config.map_key() {
            return Ok(Environment { config: config.clone(), map: environment.map.clone() });
        }
    }
    if let Some((path, map)) = state.environment_cache.lock().unwrap().as_ref() {
        if path == &config.map_key() {
            return Ok(Environment { config: config.clone(), map: map.clone() });
        }
    }
    let environment = Environment::load(config.clone())?;
    *state.environment_cache.lock().unwrap() = Some((config.map_key(), environment.map.clone()));
    Ok(environment)
}

/// Widest environment preview, in pixels.
const MAX_ENVIRONMENT_PREVIEW: u32 = 1024;

/// The environment as a panorama for the Light panel, `width` by `width / 2` RGBA pixels: at its
/// intensity, exposed, white balanced and tone-mapped as renders show light, and unrotated, since the panel
/// turns it. The map is the one renders use, loaded once.
#[tauri::command(async)]
fn environment_preview(
    environment: EnvironmentConfig,
    width: u32,
    exposure: f32,
    tone_mapping: far_core::tonemap::ToneMapping,
    white_balance: Option<f32>,
    tint: Option<f32>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    if !(2..=MAX_ENVIRONMENT_PREVIEW).contains(&width) || !exposure.is_finite() || exposure < 0.0 {
        return Err(format!("Environment previews need 2 to {MAX_ENVIRONMENT_PREVIEW} pixels and a finite exposure"));
    }
    let open = state.scene.lock().unwrap().clone();
    let environment = guarded("preview the environment", || {
        render_environment(&environment, open.as_ref().map(|open| &open.scene), &state)
    })?;
    let white = far_core::lens::white_balance(white_balance.unwrap_or(far_core::lens::NEUTRAL_KELVIN), tint.unwrap_or(0.0));
    Ok(Response::new(environment_pixels(&environment, width, exposure, white, tone_mapping)))
}

/// The RGBA pixels of `environment_preview`.
fn environment_pixels(environment: &Environment, width: u32, exposure: f32, white: [f32; 3],
    tone_mapping: far_core::tonemap::ToneMapping) -> Vec<u8> {
    let scale = environment.config.intensity * exposure;
    environment.map.thumbnail(width as usize).iter().flat_map(|c| {
        let [r, g, b] = far_core::tonemap::display(tone_mapping,
            [c.x * scale * white[0], c.y * scale * white[1], c.z * scale * white[2]]);
        [r, g, b].map(|v| (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8).into_iter().chain([255])
    }).collect()
}

#[tauri::command(async)]
fn load_scene(path: String, state: State<'_, AppState>) -> Result<SceneInfo, String> {
    guarded("open the scene", || open_scene(Path::new(&path), &state))
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn pick_focus(pose: far_core::Pose, width: u32, height: u32, x: f32, y: f32, use_kd: bool, view: Option<SceneView>,
    state: State<'_, AppState>) -> Result<Option<f32>, String> {
    pose.validate()?;
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    // What is hidden cannot be focused on, and what moved is focused where it is.
    let prepared = open.view(&view.unwrap_or_default())?;
    let (scene, _, kd, _) = open.parts(&prepared);
    Ok(far_core::camera::pick_focus(scene, if use_kd { kd.get() } else { None }, &pose, width, height, x, y))
}

/// The object or light under a point of the image, with the hidden ones left out.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn pick_object(pose: far_core::Pose, width: u32, height: u32, x: f32, y: f32, use_kd: bool, view: SceneView,
    state: State<'_, AppState>) -> Result<Option<PickedInfo>, String> {
    pose.validate()?;
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    let prepared = open.view(&view)?;
    let (scene, _, kd, _) = open.parts(&prepared);
    let shown = view.shown_lights(&open.scene);
    let picked = far_core::camera::pick(scene, if use_kd { kd.get() } else { None }, &pose, width, height, x, y);
    Ok(picked.and_then(|(found, _)| found).map(|found| match found {
        far_core::scene::Picked::Object(id) => PickedInfo::Object(id),
        // Hidden lights were taken out; the index is the one the file gives.
        far_core::scene::Picked::Light(id) => PickedInfo::Light(shown.get(id).map_or(id, |&l| l as usize)),
    }))
}

/// What each pixel of the image shows first, as 16-bit little-endian IDs row by row, with the
/// hidden objects and lights left out; see `far_core::camera::object_ids`. 0 is nothing, an
/// object is its index plus one, and a light is its index, as the file gives it, with the top
/// bit set: `ID_LIGHT`. Objects past the first 32766 are not told apart.
#[tauri::command(async)]
fn object_ids(pose: far_core::Pose, width: u32, height: u32, view: SceneView, state: State<'_, AppState>) -> Result<Response, String> {
    pose.validate()?;
    id_map_size(width, height)?;
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    let prepared = open.view(&view)?;
    let (scene, _, kd, _) = open.parts(&prepared);
    let kd = kd.get_or_init(|| KdTree::for_mode(scene, Mode::Enhanced));
    let shown = view.shown_lights(&open.scene);
    let ids = far_core::camera::object_ids(scene, Some(kd), &pose, width, height);
    Ok(Response::new(ids.into_iter().flat_map(|id| short_id(id, &shown).to_le_bytes()).collect::<Vec<u8>>()))
}

/// In the app's 16-bit ID maps, a light's index has this bit set.
const ID_LIGHT: u16 = 0x8000;

/// An ID of `far_core::camera::object_ids` in 16 bits, a light by the index the file gives it:
/// `shown` holds those indices for the lights left in the scene.
fn short_id(id: u32, shown: &[u32]) -> u16 {
    if id & far_core::camera::LIGHT_ID != 0 {
        let light = shown.get((id & !far_core::camera::LIGHT_ID) as usize).copied().unwrap_or(u32::MAX);
        if light < ID_LIGHT as u32 { ID_LIGHT | light as u16 } else { 0 }
    } else if id < ID_LIGHT as u32 {
        id as u16
    } else {
        0
    }
}

/// Where object `object` would show if nothing else were in the scene, one byte per pixel,
/// 1 or 0: with the ID map, what of it lies behind other things.
#[tauri::command(async)]
fn object_xray(pose: far_core::Pose, width: u32, height: u32, object: usize, view: Option<SceneView>, state: State<'_, AppState>)
    -> Result<Response, String> {
    pose.validate()?;
    id_map_size(width, height)?;
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    let alone = open.alone(&view.unwrap_or_default().entries, object)?;
    let kd = alone.kd.get_or_init(|| KdTree::for_mode(&alone.scene, Mode::Enhanced));
    let ids = far_core::camera::object_ids(&alone.scene, Some(kd), &pose, width, height);
    Ok(Response::new(ids.into_iter().map(|id| (id == object as u32 + 1) as u8).collect::<Vec<u8>>()))
}

#[tauri::command(async)]
fn scene_geometry(view: Option<SceneView>, state: State<'_, AppState>) -> Result<far_core::preview::PreviewScene, String> {
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    // The quick 3D view shows the scene as the interface changed it.
    let prepared = open.view(&view.unwrap_or_default())?;
    Ok(far_core::preview::describe(open.parts(&prepared).0))
}

#[tauri::command]
fn cancel(state: State<'_, AppState>) {
    state.progress.cancel.store(true, Relaxed);
}

#[tauri::command(async)]
fn save_image(name: String, app: AppHandle, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let dialog = app.dialog().file().add_filter("PNG image", &["png"])
        .add_filter("OpenEXR: linear light and passes", &["exr"]).set_file_name(format!("{name}.png"));
    let Some(file) = dialog.blocking_save_file() else { return Ok(None) };
    let path = file.into_path().map_err(|e| e.to_string())?;
    let guard = state.last_image.lock().unwrap();
    let image = guard.as_ref().ok_or("There is no finished image to save")?;
    // An EXR file takes the light behind the image, before tone mapping, and its passes.
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exr")) {
        let options = image.shown_with.as_ref().ok_or("Only a finished Enhanced render has linear light to save as EXR")?;
        let kept = state.accumulation.lock().unwrap();
        let (_, accumulation) = kept.as_ref().ok_or("Only a finished Enhanced render has linear light to save as EXR")?;
        guarded("save the EXR file", || accumulation.layers(options).write_exr(&path))?;
        return Ok(Some(path.to_string_lossy().into_owned()));
    }
    let rgb: Vec<u8> = image.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    image::save_buffer(&path, &rgb, image.width, image.height, image::ColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// The folder of example scenes, set at startup.
fn app_resources(state: &AppState) -> Option<PathBuf> {
    state.examples.lock().unwrap().clone()
}

/// A scene can be saved over itself when it is a `.far` file that is not one of the examples.
fn writable_in_place(path: &Path, examples: Option<PathBuf>) -> bool {
    let far = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("far"));
    let example = examples.and_then(|e| e.canonicalize().ok())
        .is_some_and(|e| path.canonicalize().is_ok_and(|p| p.starts_with(e)));
    far && !example && !std::fs::metadata(path).is_ok_and(|m| m.permissions().readonly())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveSceneRequest {
    camera: far_core::Camera,
    environment: Option<EnvironmentConfig>,
    #[serde(default)]
    material_overrides: Vec<MaterialOverride>,
    render: serde_json::Value,
    /// Moved or retuned objects and lights.
    #[serde(default)]
    entries: far_core::scene_save::EntryEdits,
}

#[derive(Serialize)]
struct SavedScene {
    path: String,
    warnings: Vec<String>,
    /// Objects or lights were added or deleted, so their indices changed: the app opens the
    /// saved file again.
    reopen: bool,
}

/// The interface tells whether the scene has unsaved edits, so quitting can ask first.
#[tauri::command]
fn set_unsaved(unsaved: bool, state: State<'_, AppState>) {
    state.unsaved.store(unsaved, Relaxed);
}

/// Quits once the interface has dealt with unsaved edits.
#[tauri::command]
fn quit(app: AppHandle, state: State<'_, AppState>) {
    state.quitting.store(true, Relaxed);
    app.exit(0);
}

/// Asks what to do with a scene's unsaved edits: "save", "discard" or "cancel".
#[tauri::command(async)]
fn ask_unsaved(name: String, app: AppHandle) -> &'static str {
    use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogKind, MessageDialogResult};
    let answer = app.dialog().message("Your edits to the camera, environment, materials or render settings will be lost if you don't save them.")
        .title(format!("Save changes to “{name}”?"))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::YesNoCancelCustom("Save".into(), "Don't Save".into(), "Cancel".into()))
        .blocking_show_with_result();
    match answer {
        MessageDialogResult::Yes => "save",
        MessageDialogResult::No => "discard",
        MessageDialogResult::Custom(label) if label == "Save" => "save",
        MessageDialogResult::Custom(label) if label == "Don't Save" => "discard",
        _ => "cancel",
    }
}

/// Saves the open scene with the app's edits: over its file, or to a file the user picks when
/// `choose` is set or the scene has no file it can write.
#[tauri::command(async)]
fn save_scene(choose: bool, request: SaveSceneRequest, app: AppHandle, state: State<'_, AppState>) -> Result<Option<SavedScene>, String> {
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    request.camera.validate()?;
    if let Some(environment) = &request.environment {
        environment.validate()?;
    }
    let current = open.save_to.lock().unwrap().clone();
    let target = match current.filter(|_| !choose) {
        Some(path) => path,
        None => {
            let name = open.path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "scene".into());
            let mut dialog = app.dialog().file().add_filter("Far Ray Tracer scene", &["far"]).set_file_name(format!("{name}.far"));
            // Examples live inside the app; start somewhere the user can write.
            if !writable_in_place(&open.path, app_resources(&state)) {
                if let Ok(documents) = app.path().document_dir() {
                    dialog = dialog.set_directory(documents);
                }
            }
            let Some(file) = dialog.blocking_save_file() else { return Ok(None) };
            let mut path = file.into_path().map_err(|e| e.to_string())?;
            if path.extension().is_none() {
                path.set_extension("far");
            }
            path
        }
    };
    let edits = far_core::scene_save::SceneEdits {
        camera: request.camera,
        environment: request.environment,
        materials: request.material_overrides.iter().map(|m| (m.id, m.edit())).collect(),
        render: Some(request.render),
        entries: request.entries,
    };
    let reopen = edits.entries.changes_entries();
    let warnings = far_core::scene_save::save(&open.scene, &open.path, &target, &edits)?;
    *open.save_to.lock().unwrap() = Some(target.clone());
    Ok(Some(SavedScene { path: target.to_string_lossy().into_owned(), warnings, reopen }))
}

fn message(fields: [u32; 6], pixels: Option<&Image>) -> Response {
    let size = pixels.map_or(0, |p| (p.width * p.height * 4) as usize);
    let mut bytes = vec![0u8; HEADER_SIZE + size];
    for (i, field) in fields.into_iter().enumerate() {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&field.to_le_bytes());
    }
    if let Some(p) = pixels {
        p.copy_rgba(&mut bytes[HEADER_SIZE..]);
    }
    Response::new(bytes)
}

/// Clears the "rendering" flag even if the render ends with an error.
struct RenderGuard<'a>(&'a AtomicBool);

impl Drop for RenderGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Relaxed);
    }
}

/// Runs `work`, turning a crash inside it into an error the app shows, so a bug in a file
/// reader or the engine never closes the app.
fn guarded<T>(what: &str, work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|panic| {
        let message = panic.downcast_ref::<String>().map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown failure");
        Err(format!("Could not {what}: an internal error happened ({message}). Please report it with the file."))
    })
}

/// Most photons a caustic render may trace: each one takes about 40 bytes in the map.
const MAX_PHOTONS: u32 = 50_000_000;

/// Poll the thread itself: a success-only flag would never be set after a panic.
fn render_with_frames(
    work: impl FnOnce() + Send,
    frame_interval: Duration,
    mut send_frame: impl FnMut(),
) -> Result<u64, String> {
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let begin = Instant::now();
            work();
            begin.elapsed().as_millis() as u64
        });
        while !worker.is_finished() {
            std::thread::sleep(frame_interval);
            send_frame();
        }
        worker.join().map_err(|panic| {
            let message = panic.downcast_ref::<String>().map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("unknown worker failure");
            format!("Render failed: {message}")
        })
    })
}

#[tauri::command(async)]
fn render_scene(
    app: AppHandle,
    request: RenderRequest,
    channel: Channel<Response>,
    state: State<'_, AppState>,
) -> Result<RenderResult, String> {
    let valid_side = 1..=MAX_IMAGE_SIDE;
    if !valid_side.contains(&request.width) || !valid_side.contains(&request.height) {
        return Err(format!("Image sides must be between 1 and {MAX_IMAGE_SIDE} pixels"));
    }
    if request.caustics && request.photons > MAX_PHOTONS {
        return Err(format!("Caustics take at most {} million photons", MAX_PHOTONS / 1_000_000));
    }
    if state.rendering.swap(true, Relaxed) {
        return Err("A render is already running".into());
    }
    let _guard = RenderGuard(&state.rendering);
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    let progress = &state.progress;
    progress.cancel.store(false, Relaxed);
    *progress.photon_preview.lock().unwrap() = None;
    progress.capture_photon_preview.store(request.keep, Relaxed);

    let mode = match request.mode.as_str() {
        "classic" => Mode::Classic,
        "fixed" => Mode::Fixed,
        "enhanced" => Mode::Enhanced,
        other => return Err(format!("Unknown mode: {other}")),
    };

    // What the interface edits and hides changes the scene, which then has trees of its own.
    let view = SceneView {
        hidden_objects: request.hidden_objects.clone(), hidden_lights: request.hidden_lights.clone(), entries: request.entries.clone(),
    };
    let hidden = open.view(&view)?;
    let (base, kd_classic, kd_tree, bvh_cache) = open.parts(&hidden);
    let mut kd_ms = None;
    let kd = request.use_kd.then(|| {
        let cache = if mode == Mode::Classic { kd_classic } else { kd_tree };
        cache.get_or_init(|| {
            let _ = channel.send(message([MESSAGE_BUILDING_KD, 0, 0, 0, 0, 0], None));
            let start = Instant::now();
            let tree = KdTree::for_mode(base, mode);
            kd_ms = Some(start.elapsed().as_millis() as u64);
            tree
        })
    });
    // A private snapshot keeps UI edits isolated from the original/comparison scenes.
    // Geometry is unchanged, so the already-built KD-Tree remains valid.
    let mut edited = None;
    if mode == Mode::Enhanced && (!request.material_overrides.is_empty()
        || request.environment.is_some() || base.environment.is_some()) {
        let mut copy = base.clone();
        copy.environment = request.environment.as_ref()
            .map(|config| render_environment(config, Some(base), &state)).transpose()?;
        for edit in &request.material_overrides {
            // Checked on the whole scene: a hidden object's material can still be edited.
            if !open.scene.prims.iter().any(|p| p.material() == edit.id) {
                return Err("Unknown object material".into());
            }
            edit.edit().apply(&mut copy.materials[edit.id])?;
        }
        // A changed glow changes which triangles are lights.
        if request.material_overrides.iter().any(|edit| edit.emission.is_some()) {
            copy.mesh_lights = Default::default();
        }
        edited = Some(copy);
    }
    let scene = edited.as_ref().unwrap_or(base);
    far_core::Camera {
        aperture: request.aperture,
        focus_distance: request.focus_distance,
        blades: request.blades,
        blade_rotation: request.blade_rotation,
        ..far_core::Camera::default()
    }.validate()?;
    request.pose.validate()?;
    if !(0.0..=1.0).contains(&request.lut_strength) {
        return Err("The LUT's strength is 0 to 1".into());
    }
    let lut = match request.lut.as_deref().filter(|_| mode == Mode::Enhanced) {
        Some(name) => Some(load_lut(&app, &state, name)?),
        None => None,
    };
    let options = Options {
        pose: Some(request.pose),
        aperture: Some(request.aperture),
        focus_distance: Some(request.focus_distance),
        blades: Some(request.blades),
        blade_rotation: Some(request.blade_rotation),
        mode,
        samples: request.samples,
        indirect: request.indirect,
        indirect_strength: request.indirect_strength,
        max_bounces: request.max_bounces,
        indirect_clamp: request.indirect_clamp.map(|limit| if limit > 0.0 { limit } else { f32::INFINITY }),
        denoise: request.denoise,
        bloom: request.bloom,
        exposure: request.exposure,
        tone_mapping: request.tone_mapping,
        white_balance: request.white_balance,
        tint: request.tint,
        vignette: request.vignette,
        chromatic_aberration: request.chromatic_aberration,
        grain: far_core::lens::grain_at_iso(request.grain, request.iso),
        halation: request.halation,
        lut,
        lut_strength: request.lut_strength,
        caustics: request.caustics,
        photons: request.photons,
        caustic_radius: request.caustic_radius,
        caustic_strength: request.caustic_strength,
        caustics_only: request.caustics_only,
        width: request.width,
        height: request.height,
        use_kd: request.use_kd,
        max_depth: request.depth,
        glossy_depth: request.glossy_depth,
        transmission_depth: request.transmission_depth,
        threads: request.threads.max(1),
        continuous_rng: false,
        region: request.region.map(|[x0, y0, x1, y1]| far_core::engine::Region { x0, y0, x1, y1 }),
        noise_threshold: request.noise_threshold,
    };
    // The graphics card renders the Enhanced stage; Auto leaves the CPU the scenes it would
    // render differently.
    let mut gpu_notes = Vec::new();
    let mut cpu_reason = None;
    let gpu = match request.device {
        Device::Gpu | Device::Auto if mode == Mode::Enhanced => match state.gpu.get_or_init(far_gpu::Gpu::new) {
            Err(e) => {
                cpu_reason = Some(format!("Rendered on the CPU: {e}"));
                None
            }
            Ok(gpu) => {
                let bvh = bvh_cache.get_or_init(|| far_core::bvh::Bvh::build(base));
                gpu_notes = far_gpu::leaves_out(scene, bvh);
                if request.device == Device::Auto && !gpu_notes.is_empty() {
                    cpu_reason = Some(format!("Rendered on the CPU: the GPU leaves out {}", gpu_notes.join(", ")));
                    None
                } else {
                    Some((gpu, bvh))
                }
            }
        },
        _ => None,
    };
    let passes = if mode == Mode::Enhanced { request.samples } else { 1 };
    let image = Image::new(options.width, options.height);
    let start = Instant::now();
    // Everything the accumulated light depends on; exposure, tone mapping, denoise, bloom and the
    // number of passes are left out, since the engine reuses the light across them.
    let reuse_key = (mode == Mode::Enhanced && request.keep).then(|| format!(
        "{:p}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}|{}|{:?}|{:?}|{:?}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}|{}",
        Arc::as_ptr(&open), request.environment, request.material_overrides, request.pose, request.aperture,
        request.focus_distance, request.blades, request.blade_rotation, request.indirect, request.indirect_strength,
        request.max_bounces, request.indirect_clamp, request.caustics, request.photons, request.caustic_radius,
        request.caustic_strength, request.width, request.height, request.use_kd, request.depth, request.region,
        request.noise_threshold) + &format!("|{:?}|{:?}|{}|{:?}|{:?}", request.glossy_depth, request.transmission_depth, gpu.is_some(),
        hidden.as_ref().map(Arc::as_ptr), (&request.hidden_objects, &request.hidden_lights, &request.entries)));
    // A new kept render replaces what was kept, which frees its memory early.
    let previous = reuse_key.as_ref().and_then(|key| match state.accumulation.lock().unwrap().take() {
        Some((kept, accumulation)) if &kept == key => Some(accumulation),
        _ => None,
    });
    let send_frame = || {
        if let Some(preview) = progress.photon_preview.lock().unwrap().take() {
            // Diagnostic messages carry dimensions instead of render progress.
            channel.send(message([MESSAGE_PHOTON_MAP, preview.width, preview.height, 0, 0, 0], Some(&preview)))?;
        }
        let done = progress.tiles_done.load(Relaxed) as u32;
        let total = progress.tiles_total.load(Relaxed) as u32;
        let pass = progress.pass_index.load(Relaxed) as u32;
        let ms = start.elapsed().as_millis() as u32;
        channel.send(message([MESSAGE_FRAME, done, total, ms, pass, passes], Some(&image)))
    };

    // The same light with as many passes: shown again with the new exposure, tone mapping,
    // denoise or bloom, without rendering.
    if let (Some(key), Some(accumulation)) = (&reuse_key, previous.as_ref()) {
        if accumulation.passes() == options.samples && far_core::enhanced::redisplay(accumulation, scene, &options, &image, progress) {
            progress.pass_index.store(options.samples as usize, Relaxed);
            progress.tiles_total.store(1, Relaxed);
            progress.tiles_done.store(1, Relaxed);
            send_frame().map_err(|e| e.to_string())?;
            *state.last_image.lock().unwrap() =
                Some(FinishedImage { width: options.width, height: options.height, rgba: image.rgba(), shown_with: Some(options.clone()) });
            let denoiser = *progress.denoiser.lock().unwrap();
            *state.accumulation.lock().unwrap() = previous.map(|a| (key.clone(), a));
            return Ok(RenderResult {
                kd_ms, render_ms: start.elapsed().as_millis() as u64, rays: 0, cancelled: false,
                caustics: None, denoiser, redisplayed: true, continued_from: None,
                gpu: None, gpu_notes: Vec::new(), cpu_reason: None,
            });
        }
    }
    // Fewer passes than kept cannot reuse them; more continue from them, on the CPU or the GPU,
    // whichever rendered them, since the key holds the device.
    let previous = previous.filter(|a| a.passes() < options.samples);
    let (previous, resumed) = match gpu {
        Some(_) => (None, previous.and_then(|a| a.resume(&options))),
        None => (previous, None),
    };
    let continued_from = previous.as_ref().map(|a| a.passes()).or(resumed.as_ref().map(|s| s.passes));

    let frame_interval = FRAME_INTERVAL * (image.width * image.height / PIXELS_PER_FRAME_INTERVAL).max(1);
    let mut accumulated = None;
    let mut adapter = None;
    let render_ms = if let Some((gpu, bvh)) = gpu {
        let (rendered, ms) = render_on_gpu(gpu, bvh, kd, scene, &options, resumed, &image, progress, frame_interval, send_frame)?;
        adapter = Some(rendered.0);
        accumulated = rendered.1;
        ms
    } else {
        render_with_frames(
            || {
                if mode == Mode::Enhanced {
                    accumulated = far_core::enhanced::render_continuing(scene, kd, &options, &image, progress, previous);
                } else {
                    render(scene, kd, &options, &image, progress);
                }
            },
            frame_interval,
            || { let _ = send_frame(); },
        )?
    };
    send_frame().map_err(|e| e.to_string())?;
    let mut linear = false;
    if let (Some(key), Some(accumulation)) = (reuse_key, accumulated) {
        *state.accumulation.lock().unwrap() = Some((key, accumulation));
        linear = true;
    }

    if request.keep {
        let rgba = image.rgba();
        *state.last_image.lock().unwrap() = Some(FinishedImage {
            width: options.width, height: options.height, rgba, shown_with: linear.then(|| options.clone()),
        });
    }
    let on_gpu = adapter.is_some();
    let caustics = progress.photon_stats.lock().unwrap().map(|stats| {
        let lookups = progress.photon_lookups.load(Relaxed);
        let gathered = progress.photons_gathered.load(Relaxed);
        CausticReport {
            emitted: stats.emitted,
            stored: stats.stored,
            trace_ms: stats.trace_ms,
            memory_bytes: stats.memory_bytes,
            lookups: (!on_gpu).then_some(lookups),
            photons_per_lookup: (!on_gpu).then(|| gathered as f64 / lookups.max(1) as f64),
        }
    });
    Ok(RenderResult {
        kd_ms,
        render_ms,
        rays: progress.rays.load(Relaxed),
        cancelled: progress.cancel.load(Relaxed),
        caustics,
        denoiser: *progress.denoiser.lock().unwrap(),
        redisplayed: false,
        continued_from,
        gpu: adapter,
        gpu_notes,
        cpu_reason,
    })
}

/// Renders on the graphics card, showing its image through the CPU's display, denoiser and
/// bloom as it builds up, continuing from `previous` when there is one. Gives back the card's
/// name, what the render accumulated, even when cancelled, and how long it took.
#[allow(clippy::too_many_arguments)]
fn render_on_gpu(
    gpu: &far_gpu::Gpu,
    bvh: &far_core::bvh::Bvh,
    kd: Option<&KdTree>,
    scene: &Scene,
    options: &Options,
    previous: Option<far_gpu::Sums>,
    image: &Image,
    progress: &Progress,
    frame_interval: Duration,
    send_frame: impl Fn() -> Result<(), tauri::Error>,
) -> Result<((String, Option<far_core::enhanced::Accumulation>), u64), String> {
    // The card takes every pixel of the image or region the same number of samples; frames in
    // between skip the filters.
    let options = Options { noise_threshold: 0.0, ..options.clone() };
    let quick = Options { denoise: false, bloom: false, ..options.clone() };
    let first = previous.as_ref().map_or(0, |p| p.passes) as usize;
    progress.tiles_total.store(options.samples.max(1) as usize, Relaxed);
    progress.tiles_done.store(first, Relaxed);
    progress.pass_index.store(first, Relaxed);
    progress.rays.store(0, Relaxed);
    *progress.denoiser.lock().unwrap() = None;
    *progress.photon_stats.lock().unwrap() = None;
    let show = |sums: &far_gpu::Sums, op: &Options| {
        let accumulation = sums.accumulation(op);
        far_core::enhanced::redisplay(&accumulation, scene, op, image, progress);
        progress.pass_index.store(sums.passes as usize, Relaxed);
        progress.tiles_done.store(sums.passes as usize, Relaxed);
        accumulation
    };
    let mut outcome = Err(String::new());
    let mut previous = previous;
    let ms = render_with_frames(
        || {
            let cancel = || progress.cancel.load(Relaxed);
            // The photons for caustics are traced on the CPU first, as for a CPU render, through
            // the KD-Tree when there is one.
            let pose = options.pose.unwrap_or(scene.camera.pose());
            let photons = far_core::enhanced::caustic_map(scene, kd, &options, &pose, progress);
            outcome = gpu.render(scene, bvh, &options, photons.as_ref(), previous.take(), &cancel, Some(GPU_FRAME_INTERVAL), &mut |sums| { show(sums, &quick); })
                .map(|rendered| {
                    let accumulation = show(&rendered.sums, &options);
                    // Every pass is whole, so a render stopped early can be continued too.
                    (rendered.adapter, (rendered.sums.passes > 0).then_some(accumulation))
                });
        },
        frame_interval,
        || { let _ = send_frame(); },
    )?;
    outcome.map(|o| (o, ms)).map_err(|e| format!("The GPU render failed: {e}"))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .setup(|app| {
            *app.state::<AppState>().examples.lock().unwrap() = scenes_folder(app.handle());
            load_denoiser(app.handle());
            let menu = build_menu(app.handle())?;
            app.set_menu(menu)?;
            // Development builds have no signed release to compare against.
            if !cfg!(debug_assertions) {
                updater::check(app.handle(), true);
            }
            Ok(())
        })
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .invoke_handler(tauri::generate_handler![
            startup,
            open_repository,
            choose_scene,
            choose_environment,
            choose_lut,
            pick_object,
            object_ids,
            object_xray,
            environment_preview,
            load_scene,
            cancel,
            save_image,
            save_scene,
            ask_unsaved,
            set_unsaved,
            quit,
            render_scene,
            pick_focus,
            scene_geometry,
            material_preview,
            material_ball_pose
        ])
        .build(tauri::generate_context!())
        .expect("failed to start Far Ray Tracer")
        .run(|app, event| {
            // Quitting from the menu skips the window's close request; with unsaved edits the
            // interface asks first and quits itself if the user agrees.
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = event {
                let state = app.state::<AppState>();
                if state.unsaved.load(Relaxed) && !state.quitting.load(Relaxed) {
                    api.prevent_exit();
                    let _ = app.emit("quit-requested", ());
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_requests_read_what_the_app_sends() {
        let request: SaveSceneRequest = serde_json::from_str(r#"{
            "camera": {"position": [0, 1, -5], "target": [0, 0, 0], "zoom": 1.2, "aperture": 0.6, "focus_distance": 7,
                       "blades": 6, "blade_rotation": 0},
            "environment": {"path": "", "sky": {"elevation": 35, "azimuth": 140, "turbidity": 3, "sun_size": 0.53,
                            "sun_strength": 1, "time": {"hour": 15, "latitude": -27, "day": 80}},
                            "intensity": 1, "rotation": 0, "visible": true, "enabled": true},
            "material_overrides": [{"id": 1, "surface": null, "bump_strength": 0.5}],
            "render": {"app": 1, "quality": 64, "toneMapping": "agx_punchy", "bounces": null}
        }"#).unwrap();
        assert_eq!(request.camera.blades, 6);
        assert!(request.environment.unwrap().sky.unwrap().time.is_some());
        assert_eq!(request.render["toneMapping"], "agx_punchy");
    }

    #[test]
    fn the_outliner_hides_objects_and_lights_and_finds_them_in_the_image() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/scenes/cornell-box.far");
        let scene = Scene::open(&path).unwrap();
        let (objects, lights) = (scene.objects.len(), scene.scene_lights.len());
        assert!(objects > 3 && lights > 0, "{objects} objects, {lights} lights");
        let open = OpenScene {
            scene, path, save_to: Mutex::new(None), kd_classic: OnceLock::new(), kd: OnceLock::new(), bvh: OnceLock::new(),
            view: Mutex::new(None), alone: Mutex::new(None),
        };
        let hide = |objects: &[usize], lights: &[usize]| SceneView {
            hidden_objects: objects.to_vec(), hidden_lights: lights.to_vec(), ..Default::default()
        };
        assert!(open.view(&hide(&[], &[])).unwrap().is_none());
        assert!(open.view(&hide(&[objects + 5], &[])).unwrap().is_none(), "indices past the end are ignored");
        // The same hidden set, in any order, is the same scene.
        let first = open.view(&hide(&[2, 0], &[0])).unwrap().unwrap();
        let again = open.view(&hide(&[0, 2, 2], &[0])).unwrap().unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(first.scene.scene_lights.len(), lights - 1);
        // Edits read the scene again, cached by what they change; one the file cannot take is
        // an error.
        let moved = |intensity: f32| SceneView {
            entries: far_core::scene_save::EntryEdits {
                lights: vec![(0, serde_json::json!({"intensity": intensity}).as_object().unwrap().clone())], ..Default::default()
            },
            ..Default::default()
        };
        let edited = open.view(&moved(2.5)).unwrap().unwrap();
        assert!(Arc::ptr_eq(&edited, &open.view(&moved(2.5)).unwrap().unwrap()));
        assert_eq!(edited.scene.prims.len(), open.scene.prims.len());
        assert!(open.view(&moved(-1.0)).is_err());
        // An added sphere comes after the file's objects; a deleted object is left out, as a
        // hidden one, and the added and deleted lights keep their indices.
        let grown = SceneView {
            entries: far_core::scene_save::EntryEdits {
                added_objects: vec![serde_json::json!({"type": "sphere", "name": "Ball", "center": [0, 1, 0], "radius": 0.3})],
                added_lights: vec![serde_json::json!({"type": "sphere", "name": "Lamp", "position": [0, 4, 0], "radius": 0.1})],
                removed_objects: vec![0],
                removed_lights: vec![0],
                ..Default::default()
            },
            ..Default::default()
        };
        let prepared = open.view(&grown).unwrap().unwrap();
        assert_eq!(prepared.scene.objects.len(), objects + 1);
        assert!((0..prepared.scene.prims.len()).any(|p| prepared.scene.object_of(p) == Some(objects)));
        assert!((0..prepared.scene.prims.len()).all(|p| prepared.scene.object_of(p) != Some(0)));
        assert_eq!(grown.shown_lights(&open.scene), (1..=lights as u32).collect::<Vec<_>>());
        assert!(first.scene.prims.len() < open.scene.prims.len());
        assert!((0..first.scene.prims.len()).all(|p| !matches!(first.scene.object_of(p), Some(0) | Some(2))));
        // The image's middle shows one of the box's objects, named as the outliner lists them.
        let picked = far_core::camera::pick(&open.scene, None, &open.scene.camera.pose(), 400, 300, 0.5, 0.5);
        assert!(matches!(picked, Some((Some(far_core::scene::Picked::Object(_)), _))), "{picked:?}");
        assert_eq!(serde_json::to_value(PickedInfo::Light(2)).unwrap(), serde_json::json!({"kind": "light", "id": 2}));
        // In 16 bits, a light keeps the index the file gives it with the lights before it hidden.
        assert_eq!(short_id(far_core::camera::LIGHT_ID | 1, &[0, 2]), ID_LIGHT | 2);
        assert_eq!(short_id(7, &[]), 7);
        assert_eq!(short_id(40_000, &[]), 0);
    }

    #[test]
    fn a_lut_is_a_bundled_look_by_name_or_a_file() {
        let luts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/luts");
        for look in ["warm-film", "teal-orange", "bleach-bypass", "black-and-white", "faded"] {
            let path = lut_path(Some(luts.clone()), look).unwrap();
            assert!(far_core::lut::Lut::open(&path).is_ok(), "{}", path.display());
        }
        assert_eq!(lut_path(None, "/my/looks/Kodak 2383.cube").unwrap(), PathBuf::from("/my/looks/Kodak 2383.cube"));
        assert_eq!(lut_path(None, "film.cube").unwrap(), PathBuf::from("film.cube"));
        assert!(lut_path(None, "warm-film").is_err());
    }

    #[test]
    fn material_overrides_read_a_surface_a_bump_strength_or_both() {
        let only_bump: MaterialOverride = serde_json::from_str(r#"{"id":2,"surface":null,"bump_strength":0.5}"#).unwrap();
        assert_eq!((only_bump.id, only_bump.bump_strength), (2, Some(0.5)));
        assert!(only_bump.surface.is_none());
        let surface: MaterialOverride = serde_json::from_str(
            r#"{"id":0,"surface":{"preset":"metal","roughness":0.2,"metallic":1,"ior":1.5,"color":null}}"#).unwrap();
        assert!(surface.surface.is_some() && surface.bump_strength.is_none());
        assert!(serde_json::from_str::<MaterialOverride>(r#"{"id":0,"bump":1}"#).is_err());
        let glow: MaterialOverride = serde_json::from_str(r#"{"id":1,"surface":null,"bump_strength":null,
            "emission":{"color":[1,0.5,0.25],"strength":4}}"#).unwrap();
        assert_eq!(glow.edit().emission, Some(far_core::surface::EmissionEdit { color: [1.0, 0.5, 0.25], strength: 4.0 }));
    }

    #[test]
    fn a_glow_edit_makes_a_plane_a_mesh_light() {
        let text = r#"{"lights":[],"objects":[
            {"type":"plane","points":[[0,0,5],[1,0,5],[1,1,5],[0,1,5]],"material":{"color":[0.5,0.5,0.5]}}]}"#;
        let path = std::env::temp_dir().join(format!("far-glow-edit-{}.far", std::process::id()));
        std::fs::write(&path, text).unwrap();
        let scene = Scene::open(&path);
        let _ = std::fs::remove_file(&path);
        let scene = scene.unwrap();
        assert!(scene.mesh_lights().is_empty());
        // As the render applies edits: a copy of the scene, whose table of lights starts over.
        let mut copy = scene.clone();
        let edit: MaterialOverride = serde_json::from_str(r#"{"id":0,"emission":{"color":[1,1,1],"strength":2}}"#).unwrap();
        edit.edit().apply(&mut copy.materials[0]).unwrap();
        copy.mesh_lights = Default::default();
        assert_eq!(copy.mesh_lights().lights.len(), 2);
    }

    #[test]
    fn worker_panic_returns_an_error_instead_of_waiting_forever() {
        let result = render_with_frames(|| panic!("photon regression"), Duration::from_millis(1), || {});
        assert_eq!(result.unwrap_err(), "Render failed: photon regression");
        // A later job can still complete normally.
        assert!(render_with_frames(|| {}, Duration::from_millis(1), || {}).is_ok());
    }

    #[test]
    fn the_environment_preview_shows_the_map_as_renders_do() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let config = EnvironmentConfig {
            path: root.join("assets/environments/studio.hdr").to_string_lossy().into_owned(), ..EnvironmentConfig::default()
        };
        let environment = Environment::load(config).unwrap();
        let tone = far_core::tonemap::ToneMapping::Agx;
        let pixels = environment_pixels(&environment, 256, 1.0, [1.0; 3], tone);
        assert_eq!(pixels.len(), 256 * 128 * 4);
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
        // The warm panel, left of the back (+Z at the center), is the brightest thing in it.
        let brightness = |p: &[u8]| p[0] as u32 + p[1] as u32 + p[2] as u32;
        let brightest = pixels.chunks_exact(4).enumerate().max_by_key(|(_, p)| brightness(p)).unwrap().0;
        let (x, y) = (brightest % 256, brightest / 256);
        assert!(y > 20 && y < 60 && (x as f32 / 256.0 - (3.9 / std::f32::consts::TAU)).abs() < 0.1, "{x} {y}");
        // Brighter light shows brighter, and intensity counts as exposure does.
        let mean = |p: &[u8]| p.iter().map(|&v| v as u64).sum::<u64>();
        let brighter = environment_pixels(&environment, 256, 2.0, [1.0; 3], tone);
        assert!(mean(&brighter) > mean(&pixels));
        let mut doubled = environment.clone();
        doubled.config.intensity = 2.0;
        assert_eq!(environment_pixels(&doubled, 256, 1.0, [1.0; 3], tone), brighter);
    }

    #[test]
    fn the_material_ball_renders_on_the_gpu_as_on_the_cpu() {
        let gpu = match far_gpu::Gpu::new() {
            Ok(gpu) => gpu,
            Err(e) => return eprintln!("skipped: {e}"),
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let scene = Scene::open(&root.join("assets/scenes/cornell-box.far")).unwrap();
        let config = EnvironmentConfig {
            path: root.join("assets/environments/studio.hdr").to_string_lossy().into_owned(), rotation: 140.0,
            ..EnvironmentConfig::default()
        };
        let environment = Environment::load(config).unwrap();
        let ball = far_core::preview::material_ball(&scene, 0, None, Some(environment), &root.join("assets/scenes")).unwrap();
        let options = Options { mode: Mode::Enhanced, width: 128, height: 128, samples: 64, denoise: true, max_depth: 6, ..Options::default() };
        let mean = |image: &Image| image.rgba().chunks(4).map(|p| p[..3].iter().map(|&c| c as f32).sum::<f32>()).sum::<f32>() / (3.0 * 128.0 * 128.0);
        let (on_gpu, progress) = (Image::new(128, 128), Progress::default());
        assert!(ball_on_gpu(&gpu, &ball, &options, true, &on_gpu, &progress), "the ball fell back to the CPU");
        let on_cpu = Image::new(128, 128);
        render(&ball, None, &options, &on_cpu, &Progress::default());
        let (g, c) = (mean(&on_gpu), mean(&on_cpu));
        assert!(c > 20.0 && (g - c).abs() < 0.05 * c, "GPU {g} against CPU {c}");
    }

    #[test]
    fn a_gpu_render_shows_frames_then_a_denoised_image_it_can_show_again() {
        let gpu = match far_gpu::Gpu::new() {
            Ok(gpu) => gpu,
            Err(e) => return eprintln!("skipped: {e}"),
        };
        let scene = Scene::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/scenes/cornell-box.far")).unwrap();
        let bvh = far_core::bvh::Bvh::build(&scene);
        let options = Options {
            mode: Mode::Enhanced, width: 640, height: 480, samples: 2048, denoise: true, bloom: true,
            region: Some(far_core::engine::Region { x0: 200, y0: 200, x1: 232, y1: 232 }), ..Options::default()
        };
        let (image, progress) = (Image::new(options.width, options.height), Progress::default());
        let frames = AtomicUsize::new(0);
        let ((adapter, accumulation), _) = render_on_gpu(&gpu, &bvh, None, &scene, &options, None, &image, &progress,
            Duration::from_millis(5), || { frames.fetch_add(1, Relaxed); Ok(()) }).unwrap();
        assert!(!adapter.is_empty());
        assert!(frames.load(Relaxed) > 1, "no frames while rendering");
        assert_eq!(progress.pass_index.load(Relaxed), 2048);
        assert!(progress.denoiser.lock().unwrap().is_some(), "not denoised");
        // The region is lit; far from it, the image is left as it was.
        let rgba = image.rgba();
        let mean = |x0: usize, y0: usize| (y0..y0 + 32).flat_map(|y| (x0..x0 + 32).map(move |x| (y * 640 + x) * 4))
            .map(|i| rgba[i..i + 3].iter().map(|&c| c as f32).sum::<f32>() / 3.0).sum::<f32>() / 1024.0;
        assert!(mean(200, 200) > 20.0, "the region is dark: {}", mean(200, 200));
        assert_eq!(mean(500, 400), 0.0, "the image outside the region was rendered");
        // Exposure changes show the kept light again without rendering.
        let accumulation = accumulation.unwrap();
        let brighter = Options { exposure: 2.0, bloom: false, ..options.clone() };
        assert!(far_core::enhanced::redisplay(&accumulation, &scene, &brighter, &image, &progress));
    }

    #[test]
    fn a_longer_gpu_render_continues_from_the_last_one() {
        let gpu = match far_gpu::Gpu::new() {
            Ok(gpu) => gpu,
            Err(e) => return eprintln!("skipped: {e}"),
        };
        let scene = Scene::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/scenes/cornell-box.far")).unwrap();
        let bvh = far_core::bvh::Bvh::build(&scene);
        let short = Options { mode: Mode::Enhanced, width: 96, height: 72, samples: 64, denoise: true, ..Options::default() };
        let long = Options { samples: 256, ..short.clone() };
        let render = |op: &Options, previous: Option<far_gpu::Sums>| {
            let (image, progress) = (Image::new(op.width, op.height), Progress::default());
            let ((_, accumulation), _) = render_on_gpu(&gpu, &bvh, None, &scene, op, previous, &image, &progress,
                Duration::from_millis(50), || Ok(())).unwrap();
            assert_eq!(progress.pass_index.load(Relaxed), op.samples as usize);
            (image.rgba(), accumulation.unwrap())
        };
        let (_, kept) = render(&short, None);
        let resumed = kept.resume(&long).expect("the longer render cannot continue");
        assert_eq!(resumed.passes, 64);
        // The kept passes are the first ones a full render takes, so the image is the same.
        let (continued, accumulation) = render(&long, Some(resumed));
        assert_eq!(accumulation.passes(), 256);
        let (whole, _) = render(&long, None);
        assert!(continued == whole, "continuing gives another image than rendering at once");
        // Another size, or fewer passes, start over.
        assert!(accumulation.resume(&Options { samples: 128, ..long.clone() }).is_none());
    }
}

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
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, Response};
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
/// Larger images go out less often, so sending them stays a small part of the work.
const PIXELS_PER_FRAME_INTERVAL: u32 = 1_000_000;
const MAX_IMAGE_SIDE: u32 = 8192;
const MESSAGE_BUILDING_KD: u32 = 0;
const MESSAGE_FRAME: u32 = 1;
const MESSAGE_PHOTON_MAP: u32 = 2;
const HEADER_SIZE: usize = 24;
const MENU_OPEN: &str = "open";
const MENU_SAVE: &str = "save";
const MENU_RENDER: &str = "render";
const MENU_EXAMPLE: &str = "example:";

struct OpenScene {
    scene: Scene,
    /// Built on first use: Classic mode needs the original tree, the others the corrected one.
    kd_classic: OnceLock<KdTree>,
    kd: OnceLock<KdTree>,
}

struct FinishedImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Default)]
struct AppState {
    environment_cache: Mutex<Option<(String, Arc<EnvironmentMap>)>>,
    scene: Mutex<Option<Arc<OpenScene>>>,
    progress: Arc<Progress>,
    rendering: AtomicBool,
    last_image: Mutex<Option<FinishedImage>>,
    /// The studio environment that lights material previews, loaded on first use.
    studio: OnceLock<Option<Environment>>,
    /// The material preview being rendered, so a newer one can cancel it.
    /// The material previews being rendered, by view, so a newer one in a view can cancel it.
    previews: Mutex<std::collections::HashMap<String, Arc<Progress>>>,
}

#[derive(Serialize)]
struct SceneMaterial {
    id: usize,
    label: String,
    color: [f32; 3],
    ior: f32,
    surface: Option<far_core::surface::Surface>,
    bump: Option<BumpInfo>,
}

#[derive(Serialize)]
struct BumpInfo {
    kind: far_core::bump::BumpKind,
    strength: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialOverride {
    id: usize,
    #[serde(default)]
    surface: Option<far_core::surface::Surface>,
    #[serde(default)]
    bump_strength: Option<f32>,
}

impl MaterialOverride {
    fn edit(&self) -> far_core::surface::MaterialEdit {
        far_core::surface::MaterialEdit { surface: self.surface.clone(), bump_strength: self.bump_strength }
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
    warnings: Vec<String>,
    /// The automatic indirect light settings for this scene's lights.
    bounces: BounceInfo,
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
    mode: String,
    width: u32,
    height: u32,
    depth: i32,
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
    caustics: bool,
    photons: u32,
    caustic_radius: f32,
    caustic_strength: f32,
    caustics_only: bool,
    /// The image the Save command should write. Comparison renders do not count.
    keep: bool,
}

#[derive(Serialize)]
struct CausticReport {
    emitted: u32,
    stored: usize,
    trace_ms: u64,
    memory_bytes: usize,
    lookups: u64,
    photons_per_lookup: f64,
}

#[derive(Serialize)]
struct RenderResult {
    kd_ms: Option<u64>,
    render_ms: u64,
    rays: u64,
    cancelled: bool,
    caustics: Option<CausticReport>,
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
        Some(SceneMaterial { id, label: format!("Material {} ({kind})", seen.len()), color: m.color.array(), ior: m.ior, surface: m.surface.clone(),
            bump: m.bump.map(|b| BumpInfo { kind: b.kind, strength: b.strength }) })
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
        warnings: scene.warnings.clone(),
        bounces: {
            let auto = far_core::Bounces::for_scene(&scene);
            BounceInfo { strength: auto.strength, max: auto.max, clamp: auto.clamp }
        },
    };
    *state.scene.lock().unwrap() = Some(Arc::new(OpenScene { scene, kd_classic: OnceLock::new(), kd: OnceLock::new() }));
    Ok(info)
}

/// The scenes folder: the one inside the app bundle or, in development, the repository's.
fn scenes_folder(app: &AppHandle) -> Option<PathBuf> {
    let bundled = app.path().resource_dir().ok().map(|d| d.join("assets/scenes"));
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/scenes");
    bundled.into_iter().chain([repository]).find(|p| p.is_dir())
}

fn collect_scenes(folder: &Path, group: &str, scenes: &mut Vec<SceneEntry>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for path in entries.flatten().map(|e| e.path()) {
        let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if path.is_dir() {
            if name != "debug" {
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

/// Turns a file name such as `plane-and-mesh` into `Plane and mesh`.
fn pretty_name(name: &str) -> String {
    let text = name.replace('-', " ");
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
    let mut thesis = SubmenuBuilder::new(app, "Thesis Tests");
    let mut has_thesis_scenes = false;
    for scene in scenes.iter().filter(|s| !s.group.is_empty()) {
        thesis = thesis.item(&item(scene)?);
        has_thesis_scenes = true;
    }
    if has_thesis_scenes {
        examples = examples.separator().item(&thesis.build()?);
    }

    let open = MenuItemBuilder::with_id(MENU_OPEN, "Open Scene…").accelerator("CmdOrCtrl+O").build(app)?;
    let save = MenuItemBuilder::with_id(MENU_SAVE, "Save PNG…").accelerator("CmdOrCtrl+S").build(app)?;
    let render = MenuItemBuilder::with_id(MENU_RENDER, "Render").accelerator("CmdOrCtrl+R").build(app)?;

    let application = SubmenuBuilder::new(app, "Far Ray Tracer")
        .about(None)
        .separator()
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
        .item(&save)
        .separator()
        .close_window()
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let render_menu = SubmenuBuilder::new(app, "Render").item(&render).build()?;
    let view = SubmenuBuilder::new(app, "View").fullscreen().build()?;
    let window = SubmenuBuilder::new(app, "Window").minimize().maximize().build()?;
    MenuBuilder::new(app).items(&[&application, &file, &edit, &render_menu, &view, &window]).build()
}

fn on_menu(app: &AppHandle, id: &str) {
    let action = match id {
        MENU_OPEN => MenuAction { action: "open", path: None },
        MENU_SAVE => MenuAction { action: "save", path: None },
        MENU_RENDER => MenuAction { action: "render", path: None },
        _ => match id.strip_prefix(MENU_EXAMPLE) {
            Some(path) => MenuAction { action: "example", path: Some(path.to_string()) },
            None => return,
        },
    };
    let _ = app.emit("menu-action", action);
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
    let mut dialog = app.dialog().file().add_filter("Far Ray Tracer scenes", &["far"]);
    if let Some(folder) = scenes_folder(&app) {
        dialog = dialog.set_directory(folder);
    }
    let Some(file) = dialog.blocking_pick_file() else { return Ok(None) };
    let path = file.into_path().map_err(|e| e.to_string())?;
    open_scene(&path, &state).map(Some)
}

#[tauri::command(async)]
fn choose_environment(app: AppHandle, state: State<'_, AppState>) -> Result<Option<EnvironmentConfig>, String> {
    let Some(file) = app.dialog().file().add_filter("HDR environment", &["hdr"]).blocking_pick_file() else {
        return Ok(None);
    };
    let path = file.into_path().map_err(|e| e.to_string())?;
    let config = EnvironmentConfig { path: path.to_string_lossy().into_owned(), ..EnvironmentConfig::default() };
    // Decode and build the sampling table once; slider edits reuse the shared map.
    let environment = Environment::load(config.clone())?;
    *state.environment_cache.lock().unwrap() = Some((config.path.clone(), environment.map));
    Ok(Some(config))
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
    width: u32,
    height: u32,
    samples: u32,
    pose: Option<far_core::Pose>,
    view: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let valid_side = 1..=MAX_PREVIEW_SIDE;
    if !valid_side.contains(&width) || !valid_side.contains(&height) || !(1..=MAX_PREVIEW_SAMPLES).contains(&samples) {
        return Err(format!("Previews need 1 to {MAX_PREVIEW_SIDE} pixels and 1 to {MAX_PREVIEW_SAMPLES} samples"));
    }
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    let environment = state.studio.get_or_init(|| studio_environment(&app)).clone();
    let data = scenes_folder(&app).unwrap_or_default();
    let edit = far_core::surface::MaterialEdit { surface, bump_strength };
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
        use_kd: false,
        threads,
        ..Options::default()
    };
    let image = Image::new(width, height);
    render(&ball, None, &options, &image, &progress);
    if progress.cancel.load(Relaxed) {
        return Err("Replaced by a newer preview".into());
    }
    Ok(Response::new(image.rgba()))
}

fn render_environment(config: &EnvironmentConfig, scene: &Scene, state: &AppState) -> Result<Environment, String> {
    config.validate()?;
    if let Some(environment) = &scene.environment {
        if environment.config.path == config.path {
            return Ok(Environment { config: config.clone(), map: environment.map.clone() });
        }
    }
    if let Some((path, map)) = state.environment_cache.lock().unwrap().as_ref() {
        if path == &config.path {
            return Ok(Environment { config: config.clone(), map: map.clone() });
        }
    }
    let environment = Environment::load(config.clone())?;
    *state.environment_cache.lock().unwrap() = Some((config.path.clone(), environment.map.clone()));
    Ok(environment)
}

#[tauri::command(async)]
fn load_scene(path: String, state: State<'_, AppState>) -> Result<SceneInfo, String> {
    open_scene(Path::new(&path), &state)
}

#[tauri::command(async)]
fn pick_focus(pose: far_core::Pose, width: u32, height: u32, x: f32, y: f32, use_kd: bool, state: State<'_, AppState>) -> Result<Option<f32>, String> {
    pose.validate()?;
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    Ok(far_core::camera::pick_focus(&open.scene, if use_kd { open.kd.get() } else { None }, &pose, width, height, x, y))
}

#[tauri::command(async)]
fn scene_geometry(state: State<'_, AppState>) -> Result<far_core::preview::PreviewScene, String> {
    let open = state.scene.lock().unwrap().clone().ok_or("No scene is open")?;
    Ok(far_core::preview::describe(&open.scene))
}

#[tauri::command]
fn cancel(state: State<'_, AppState>) {
    state.progress.cancel.store(true, Relaxed);
}

#[tauri::command(async)]
fn save_image(name: String, app: AppHandle, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let dialog = app.dialog().file().add_filter("PNG image", &["png"]).set_file_name(format!("{name}.png"));
    let Some(file) = dialog.blocking_save_file() else { return Ok(None) };
    let path = file.into_path().map_err(|e| e.to_string())?;
    let guard = state.last_image.lock().unwrap();
    let image = guard.as_ref().ok_or("There is no finished image to save")?;
    let rgb: Vec<u8> = image.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    image::save_buffer(&path, &rgb, image.width, image.height, image::ColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
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
    request: RenderRequest,
    channel: Channel<Response>,
    state: State<'_, AppState>,
) -> Result<RenderResult, String> {
    let valid_side = 1..=MAX_IMAGE_SIDE;
    if !valid_side.contains(&request.width) || !valid_side.contains(&request.height) {
        return Err(format!("Image sides must be between 1 and {MAX_IMAGE_SIDE} pixels"));
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

    let mut kd_ms = None;
    let kd = request.use_kd.then(|| {
        let cache = if mode == Mode::Classic { &open.kd_classic } else { &open.kd };
        cache.get_or_init(|| {
            let _ = channel.send(message([MESSAGE_BUILDING_KD, 0, 0, 0, 0, 0], None));
            let start = Instant::now();
            let tree = KdTree::for_mode(&open.scene, mode);
            kd_ms = Some(start.elapsed().as_millis() as u64);
            tree
        })
    });
    // A private snapshot keeps UI edits isolated from the original/comparison scenes.
    // Geometry is unchanged, so the already-built KD-Tree remains valid.
    let mut edited = None;
    if mode == Mode::Enhanced && (!request.material_overrides.is_empty()
        || request.environment.is_some() || open.scene.environment.is_some()) {
        let mut copy = open.scene.clone();
        copy.environment = request.environment.as_ref()
            .map(|config| render_environment(config, &open.scene, &state)).transpose()?;
        for edit in &request.material_overrides {
            if !copy.prims.iter().any(|p| p.material() == edit.id) {
                return Err("Unknown object material".into());
            }
            edit.edit().apply(&mut copy.materials[edit.id])?;
        }
        edited = Some(copy);
    }
    let scene = edited.as_ref().unwrap_or(&open.scene);
    far_core::Camera { aperture: request.aperture, focus_distance: request.focus_distance, ..far_core::Camera::default() }.validate()?;
    request.pose.validate()?;
    let options = Options {
        pose: Some(request.pose),
        aperture: Some(request.aperture),
        focus_distance: Some(request.focus_distance),
        mode,
        samples: request.samples,
        indirect: request.indirect,
        indirect_strength: request.indirect_strength,
        max_bounces: request.max_bounces,
        indirect_clamp: request.indirect_clamp.map(|limit| if limit > 0.0 { limit } else { f32::INFINITY }),
        denoise: request.denoise,
        bloom: request.bloom,
        exposure: request.exposure,
        caustics: request.caustics,
        photons: request.photons,
        caustic_radius: request.caustic_radius,
        caustic_strength: request.caustic_strength,
        caustics_only: request.caustics_only,
        width: request.width,
        height: request.height,
        use_kd: request.use_kd,
        max_depth: request.depth,
        threads: request.threads.max(1),
        continuous_rng: false,
    };
    let passes = if mode == Mode::Enhanced { request.samples } else { 1 };
    let image = Image::new(options.width, options.height);
    let start = Instant::now();
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

    let frame_interval = FRAME_INTERVAL * (image.width * image.height / PIXELS_PER_FRAME_INTERVAL).max(1);
    let render_ms = render_with_frames(
        || render(scene, kd, &options, &image, progress),
        frame_interval,
        || { let _ = send_frame(); },
    )?;
    send_frame().map_err(|e| e.to_string())?;

    if request.keep {
        let rgba = image.rgba();
        *state.last_image.lock().unwrap() =
            Some(FinishedImage { width: options.width, height: options.height, rgba });
    }
    let caustics = progress.photon_stats.lock().unwrap().map(|stats| {
        let lookups = progress.photon_lookups.load(Relaxed);
        let gathered = progress.photons_gathered.load(Relaxed);
        CausticReport {
            emitted: stats.emitted,
            stored: stats.stored,
            trace_ms: stats.trace_ms,
            memory_bytes: stats.memory_bytes,
            lookups,
            photons_per_lookup: gathered as f64 / lookups.max(1) as f64,
        }
    });
    Ok(RenderResult {
        kd_ms,
        render_ms,
        rays: progress.rays.load(Relaxed),
        cancelled: progress.cancel.load(Relaxed),
        caustics,
    })
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            let menu = build_menu(app.handle())?;
            app.set_menu(menu)?;
            Ok(())
        })
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .invoke_handler(tauri::generate_handler![
            startup,
            choose_scene,
            choose_environment,
            load_scene,
            cancel,
            save_image,
            render_scene,
            pick_focus,
            scene_geometry,
            material_preview,
            material_ball_pose
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Far Ray Tracer");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_overrides_read_a_surface_a_bump_strength_or_both() {
        let only_bump: MaterialOverride = serde_json::from_str(r#"{"id":2,"surface":null,"bump_strength":0.5}"#).unwrap();
        assert_eq!((only_bump.id, only_bump.bump_strength), (2, Some(0.5)));
        assert!(only_bump.surface.is_none());
        let surface: MaterialOverride = serde_json::from_str(
            r#"{"id":0,"surface":{"preset":"metal","roughness":0.2,"metallic":1,"ior":1.5,"color":null}}"#).unwrap();
        assert!(surface.surface.is_some() && surface.bump_strength.is_none());
        assert!(serde_json::from_str::<MaterialOverride>(r#"{"id":0,"bump":1}"#).is_err());
    }

    #[test]
    fn worker_panic_returns_an_error_instead_of_waiting_forever() {
        let result = render_with_frames(|| panic!("photon regression"), Duration::from_millis(1), || {});
        assert_eq!(result.unwrap_err(), "Render failed: photon regression");
        // A later job can still complete normally.
        assert!(render_with_frames(|| {}, Duration::from_millis(1), || {}).is_ok());
    }
}

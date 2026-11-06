//! Far Ray Tracer desktop app: opens a scene and shows the render as it happens.
//!
//! The engine writes to a shared image and this layer sends a copy of it to the
//! window about 30 times per second. Each message has a 24-byte header (kind,
//! tiles done, total tiles, milliseconds, current pass, total passes) followed by
//! the RGBA pixels.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
const HEADER_SIZE: usize = 24;
const MENU_OPEN: &str = "open";
const MENU_SAVE: &str = "save";
const MENU_RENDER: &str = "render";
const MENU_EXAMPLE: &str = "example:";

struct OpenScene {
    scene: Scene,
    kd: OnceLock<KdTree>,
}

struct FinishedImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Default)]
struct AppState {
    scene: Mutex<Option<Arc<OpenScene>>>,
    progress: Arc<Progress>,
    rendering: AtomicBool,
    last_image: Mutex<Option<FinishedImage>>,
}

#[derive(Serialize)]
struct SceneInfo {
    name: String,
    path: String,
    primitives: usize,
    lights: usize,
    warnings: Vec<String>,
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
    mode: String,
    width: u32,
    height: u32,
    depth: i32,
    threads: usize,
    use_kd: bool,
    samples: u32,
    indirect: bool,
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
    let info = SceneInfo {
        name: path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path: path.to_string_lossy().into_owned(),
        primitives: scene.prims.len(),
        lights: scene.lights.len(),
        warnings: scene.warnings.clone(),
    };
    *state.scene.lock().unwrap() = Some(Arc::new(OpenScene { scene, kd: OnceLock::new() }));
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
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("frt") || e.eq_ignore_ascii_case("far")) {
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
    let mut dialog = app.dialog().file().add_filter("Far Ray Tracer scenes", &["far", "frt"]);
    if let Some(folder) = scenes_folder(&app) {
        dialog = dialog.set_directory(folder);
    }
    let Some(file) = dialog.blocking_pick_file() else { return Ok(None) };
    let path = file.into_path().map_err(|e| e.to_string())?;
    open_scene(&path, &state).map(Some)
}

#[tauri::command(async)]
fn load_scene(path: String, state: State<'_, AppState>) -> Result<SceneInfo, String> {
    open_scene(Path::new(&path), &state)
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

    let mut kd_ms = None;
    let kd = request.use_kd.then(|| {
        open.kd.get_or_init(|| {
            let _ = channel.send(message([MESSAGE_BUILDING_KD, 0, 0, 0, 0, 0], None));
            let start = Instant::now();
            let tree = KdTree::build(&open.scene);
            kd_ms = Some(start.elapsed().as_millis() as u64);
            tree
        })
    });

    let mode = match request.mode.as_str() {
        "classic" => Mode::Classic,
        "fixed" => Mode::Fixed,
        "enhanced" => Mode::Enhanced,
        other => return Err(format!("Unknown mode: {other}")),
    };
    let options = Options {
        mode,
        samples: request.samples,
        indirect: request.indirect,
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
    let finished = AtomicBool::new(false);
    let start = Instant::now();
    let send_frame = || {
        let done = progress.tiles_done.load(Relaxed) as u32;
        let total = progress.tiles_total.load(Relaxed) as u32;
        let pass = progress.pass_index.load(Relaxed) as u32;
        let ms = start.elapsed().as_millis() as u32;
        channel.send(message([MESSAGE_FRAME, done, total, ms, pass, passes], Some(&image)))
    };

    let frame_interval = FRAME_INTERVAL * (image.width * image.height / PIXELS_PER_FRAME_INTERVAL).max(1);
    let mut render_ms = 0;
    std::thread::scope(|s| {
        s.spawn(|| {
            let begin = Instant::now();
            render(&open.scene, kd, &options, &image, progress);
            render_ms = begin.elapsed().as_millis() as u64;
            finished.store(true, Relaxed);
        });
        while !finished.load(Relaxed) {
            std::thread::sleep(frame_interval);
            let _ = send_frame();
        }
    });
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
            load_scene,
            cancel,
            save_image,
            render_scene
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Far Ray Tracer");
}

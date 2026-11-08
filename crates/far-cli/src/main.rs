//! Far Ray Tracer command line.

use far_core::{render, Image, KdTree, Mode, Options, Progress, Scene};
use std::path::Path;
use std::process::exit;
use std::sync::atomic::Ordering;
use std::time::Instant;

const HELP: &str = "\
usage:
  far render <scene.far> -o <output.png|bmp> [options]
      --width N       image width (default 800)
      --height N      image height (default 600)
      --depth N       recursion depth (default 3)
      --threads N     thread count (default: all cores)
      --no-kd         do not use the KD-Tree
      --exact         1 thread and the same random sequence as the original
      --fixed         keep the original algorithm and only remove its bugs
      --enhanced      also use distributed ray tracing and indirect light
      --samples N     samples per pixel in enhanced mode (default 16)
      --no-indirect   turn off indirect light in enhanced mode
      --indirect-strength N  multiplier of each rebound; 1 is physically correct
                      (default: 1, or 0.45 with a 2008 light)
      --bounces N     diffuse rebounds per path, at most 64 (default: 64 with
                      Russian roulette, or 2 with a 2008 light)
      --indirect-clamp N     most light one rebound brings back; 0 for no limit
                      (default: 10, or 1.5 with a 2008 light)
      --denoise       filter the finished image in enhanced mode
      --bloom         spread very bright light into a soft halo in enhanced mode
      --exposure N    brightness multiplier in enhanced mode (default 1)
      --aperture N    lens diameter in scene units; 0 disables depth of field
      --focus-distance N   focus-plane distance from camera in scene units
      --camera X,Y,Z  camera position in enhanced mode
      --target X,Y,Z  point the camera looks at in enhanced mode
      --zoom N        lens magnification in enhanced mode (default 1)
      --caustics      trace photons so glass and mirrors focus light
      --photons N     photons for caustics (default 1000000)
      --caustic-radius N    photon lookup radius, smaller is sharper (default 0.12)
      --caustic-strength N  multiplier of the caustic light (default 1)
      --caustics-only show only the caustic light
  far material-ball <scene.far> <material index> -o <output.png> [--size N] [--samples N]
      render one of the scene's materials on the app's preview ball
  far geometry <scene.far> -o <output.json>
      write what the app's real-time camera view shows
  far compare <a> <b>
      compare two images, ignoring the first row and first column,
      which the original program never writes";

fn fail(message: &str) -> ! {
    eprintln!("error: {message}");
    exit(1)
}

fn number<T: std::str::FromStr>(args: &mut impl Iterator<Item = String>, name: &str) -> T {
    args.next()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| fail(&format!("{name} needs a number")))
}

fn point(args: &mut impl Iterator<Item = String>, name: &str) -> [f32; 3] {
    let text = args.next().unwrap_or_default();
    let values: Vec<f32> = text.split(',').filter_map(|v| v.parse().ok()).collect();
    values.try_into().unwrap_or_else(|_| fail(&format!("{name} needs X,Y,Z")))
}

fn render_command(mut args: impl Iterator<Item = String>) {
    let mut options = Options::default();
    let (mut scene, mut output) = (None, None);
    let (mut position, mut target, mut zoom) = (None, None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => output = args.next(),
            "--width" => options.width = number(&mut args, &a),
            "--height" => options.height = number(&mut args, &a),
            "--depth" => options.max_depth = number(&mut args, &a),
            "--threads" => options.threads = number(&mut args, &a),
            "--no-kd" => options.use_kd = false,
            "--fixed" => options.mode = Mode::Fixed,
            "--enhanced" => options.mode = Mode::Enhanced,
            "--samples" => options.samples = number(&mut args, &a),
            "--no-indirect" => options.indirect = false,
            "--indirect-strength" => options.indirect_strength = Some(number(&mut args, &a)),
            "--bounces" => options.max_bounces = Some(number(&mut args, &a)),
            "--indirect-clamp" => {
                let limit: f32 = number(&mut args, &a);
                options.indirect_clamp = Some(if limit > 0.0 { limit } else { f32::INFINITY });
            }
            "--denoise" => options.denoise = true,
            "--bloom" => options.bloom = true,
            "--aperture" => options.aperture = Some(number(&mut args, &a)),
            "--focus-distance" => options.focus_distance = Some(number(&mut args, &a)),
            "--camera" => position = Some(point(&mut args, &a)),
            "--target" => target = Some(point(&mut args, &a)),
            "--zoom" => zoom = Some(number(&mut args, &a)),
            "--exposure" => options.exposure = number(&mut args, &a),
            "--caustics" => options.caustics = true,
            "--photons" => options.photons = number(&mut args, &a),
            "--caustic-radius" => options.caustic_radius = number(&mut args, &a),
            "--caustic-strength" => options.caustic_strength = number(&mut args, &a),
            "--caustics-only" => options.caustics_only = true,
            "--exact" => {
                options.threads = 1;
                options.continuous_rng = true;
            }
            _ if scene.is_none() => scene = Some(a),
            _ => fail(&format!("unknown option: {a}")),
        }
    }
    let (Some(scene), Some(output)) = (scene, output) else {
        eprintln!("{HELP}");
        exit(2);
    };

    let t0 = Instant::now();
    let scene = Scene::open(Path::new(&scene)).unwrap_or_else(|e| fail(&e));
    let camera = far_core::Camera {
        aperture: options.aperture.unwrap_or(scene.camera.aperture),
        focus_distance: options.focus_distance.unwrap_or(scene.camera.focus_distance),
        position: position.unwrap_or(scene.camera.position),
        target: target.unwrap_or(scene.camera.target),
        zoom: zoom.unwrap_or(scene.camera.zoom),
    };
    camera.validate().unwrap_or_else(|e| fail(&e));
    options.pose = Some(camera.pose());
    for warning in &scene.warnings {
        eprintln!("warning: {warning}");
    }
    let t1 = Instant::now();
    let kd = options.use_kd.then(|| KdTree::for_mode(&scene, options.mode));
    let t2 = Instant::now();
    let image = Image::new(options.width, options.height);
    let progress = Progress::default();
    render(&scene, kd.as_ref(), &options, &image, &progress);
    let t3 = Instant::now();

    println!(
        "primitives={} lights={} load={}ms kd={}ms render={}ms rays={}",
        scene.prims.len(),
        scene.lights.len() + scene.rect_lights.len() + scene.bulbs.len(),
        (t1 - t0).as_millis(),
        (t2 - t1).as_millis(),
        (t3 - t2).as_millis(),
        progress.rays.load(Ordering::Relaxed)
    );
    if let Some(stats) = *progress.photon_stats.lock().unwrap() {
        let lookups = progress.photon_lookups.load(Ordering::Relaxed);
        let gathered = progress.photons_gathered.load(Ordering::Relaxed);
        println!(
            "photons emitted={} stored={} trace={}ms memory={}MB lookups={lookups} per_lookup={:.1}",
            stats.emitted,
            stats.stored,
            stats.trace_ms,
            stats.memory_bytes / 1_000_000,
            gathered as f64 / lookups.max(1) as f64,
        );
    }
    let rgb: Vec<u8> = image.rgba().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    image::RgbImage::from_raw(options.width, options.height, rgb)
        .expect("image size")
        .save(&output)
        .unwrap_or_else(|e| fail(&format!("{output}: {e}")));
}

fn material_ball(mut args: impl Iterator<Item = String>) {
    let (Some(scene_path), Some(index)) = (args.next(), args.next()) else {
        fail("material-ball needs <scene.far> <material index> -o <output.png>");
    };
    let (mut output, mut size, mut samples, mut bounces, mut threads, mut no_env) = (None, 384, 24, None, None, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => output = args.next(),
            "--size" => size = number(&mut args, &a),
            "--samples" => samples = number(&mut args, &a),
            "--bounces" => bounces = Some(number(&mut args, &a)),
            "--threads" => threads = Some(number(&mut args, &a)),
            "--no-environment" => no_env = true,
            _ => fail(&format!("unknown option: {a}")),
        }
    }
    let output = output.unwrap_or_else(|| fail("material-ball needs -o <output.png>"));
    let index: usize = index.parse().unwrap_or_else(|_| fail("the material index must be a number"));
    let scene = Scene::open(Path::new(&scene_path)).unwrap_or_else(|e| fail(&e));
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes");
    let studio = far_core::environment::EnvironmentConfig {
        path: data.join("../environments/studio.hdr").to_string_lossy().into_owned(),
        rotation: 140.0,
        ..Default::default()
    };
    let environment = if no_env { None } else { far_core::environment::Environment::load(studio).ok() };
    let ball = far_core::preview::material_ball(&scene, index, None, environment, &data).unwrap_or_else(|e| fail(&e));
    let options = Options { mode: Mode::Enhanced, width: size, height: size, samples, denoise: true, max_depth: 6, use_kd: false, max_bounces: bounces, ..Options::default() };
    let kd = (ball.prims.len() >= far_core::preview::BALL_TREE_FROM).then(|| KdTree::build_corrected(&ball));
    let options = Options { use_kd: kd.is_some(), ..options };
    let options = Options { threads: threads.unwrap_or(options.threads), ..options };
    let image = Image::new(size, size);
    let t0 = Instant::now();
    render(&ball, kd.as_ref(), &options, &image, &Progress::default());
    println!("primitives={} render={}ms", ball.prims.len(), t0.elapsed().as_millis());
    let rgb: Vec<u8> = image.rgba().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    image::RgbImage::from_raw(size, size, rgb).expect("image size").save(&output)
        .unwrap_or_else(|e| fail(&format!("{output}: {e}")));
}

fn geometry(mut args: impl Iterator<Item = String>) {
    let (Some(scene), Some(_), Some(output)) = (args.next(), args.next(), args.next()) else {
        fail("geometry needs <scene.far> -o <output.json>");
    };
    let scene = Scene::open(Path::new(&scene)).unwrap_or_else(|e| fail(&e));
    let text = serde_json::to_string(&far_core::preview::describe(&scene)).expect("geometry is serializable");
    std::fs::write(&output, text).unwrap_or_else(|e| fail(&format!("{output}: {e}")));
}

fn compare(a: &str, b: &str) {
    let open = |c: &str| image::open(c).unwrap_or_else(|e| fail(&format!("{c}: {e}"))).to_rgb8();
    let (a, b) = (open(a), open(b));
    if a.dimensions() != b.dimensions() {
        fail("the images have different sizes");
    }
    let (w, h) = a.dimensions();
    let (mut different, mut visible, mut largest) = (0u64, 0u64, 0i32);
    for y in 1..h {
        for x in 1..w {
            let (p, q) = (a.get_pixel(x, y), b.get_pixel(x, y));
            let d = (0..3).map(|i| (p[i] as i32 - q[i] as i32).abs()).max().unwrap_or(0);
            different += (d > 0) as u64;
            visible += (d > 16) as u64;
            largest = largest.max(d);
        }
    }
    let total = ((w - 1) * (h - 1)) as f64;
    println!(
        "different={different} ({:.3}%) above_16={visible} ({:.3}%) largest_difference={largest}",
        100.0 * different as f64 / total,
        100.0 * visible as f64 / total
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("render") => render_command(args),
        Some("geometry") => geometry(args),
        Some("material-ball") => material_ball(args),
        Some("compare") => match (args.next(), args.next()) {
            (Some(a), Some(b)) => compare(&a, &b),
            _ => fail("compare needs two images"),
        },
        _ => {
            eprintln!("{HELP}");
            exit(2);
        }
    }
}

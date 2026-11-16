//! Far Ray Tracer command line.

use far_core::{render, Image, KdTree, Mode, Options, Progress, Scene};
use std::path::Path;
use std::process::exit;
use std::sync::atomic::Ordering;
use std::time::Instant;

const HELP: &str = "\
usage:
  far render <scene.far> -o <output.png|bmp|exr> [options]
      an .exr file holds the linear light before tone mapping, in enhanced mode,
      with --denoise also the denoised light, albedo, normal and depth
      --width N       image width (default 800)
      --height N      image height (default 600)
      --depth N       recursion depth (default 3)
      --glossy-depth N        in enhanced mode, most mirror-like reflections in a path
      --transmission-depth N  in enhanced mode, most glass crossings in a path; both stay
                      within --depth, so --depth 12 --glossy-depth 3 lets glass go deep
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
      --noise-threshold N   adaptive sampling: a pixel stops once its error is below N
                      (0.01 is a good start); --samples is then the most it takes
      --bloom         spread very bright light into a soft halo in enhanced mode
      --gpu           render on the graphics card (a first cut: see the README)
      --exposure N    brightness multiplier in enhanced mode (default 1)
      --tone-mapping standard|agx|agx-punchy|aces   how light becomes screen colors (default standard)
      --iso N         the camera's ISO: twice the ISO is twice the light, and more grain
                      (default 100)
      --shutter T     shutter time in seconds, such as 1/60 or 0.5: twice the time is twice
                      the light (default 1/125); ISO 100 at 1/125 is exposure 1
      --white-balance K    color temperature in kelvin that comes out white, 1500 to 15000;
                      lower suits warmer light (default 6500, which changes nothing)
      --tint N        white balance from -1 (green) to 1 (magenta) (default 0)
      --grain N       film grain, 0 to 1, stronger at high ISO (default 0)
      --halation N    reddish glow around bright light, as on film, 0 to 1 (default 0)
      --vignette N    darkens the corners as a lens does, 0 to 1 (default 0)
      --chromatic-aberration N   red and blue fringes toward the corners, 0 to 1 (default 0)
      --lut FILE      grades the finished picture with a .cube LUT, as a color look; the
                      program's own are in assets/luts
      --lut-strength N   how much of the LUT's look to show, 0 to 1 (default 1)
      --aperture N    lens diameter in scene units; 0 disables depth of field
      --f-number N    the lens opening as a camera's f-number, which sets the depth of field
                      and, as on a camera, the light: f/8 leaves it as rendered, and each
                      stop wider (f/5.6, f/4) doubles it
      --focus-distance N   focus-plane distance from camera in scene units
      --blades N      diaphragm blades, the shape of out-of-focus highlights; 0 is round
      --blade-rotation N   turns that shape, in degrees
      --camera X,Y,Z  camera position in enhanced mode
      --target X,Y,Z  point the camera looks at in enhanced mode
      --zoom N        lens magnification in enhanced mode (default 1)
      --focal-length MM    the lens's focal length, in place of --zoom: 22.5 is zoom 1 on
                      a full-frame sensor
      --sensor-width MM    the sensor's width (default the scene's, or 36, full frame)
      --scene-unit M  how long a scene unit is, in metres (default the scene's, or 0.1)
      --caustics      trace photons so glass and mirrors focus light
      --photons N     photons for caustics (default 1000000)
      --caustic-radius N    photon lookup radius, smaller is sharper (default 0.12)
      --caustic-strength N  multiplier of the caustic light (default 1)
      --caustics-only show only the caustic light
      --region X0,Y0,X1,Y1  render only this rectangle of pixels; the rest stays black
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

/// A number above 0.
fn positive(args: &mut impl Iterator<Item = String>, name: &str) -> f32 {
    let value: f32 = number(args, name);
    if value.is_nan() || value <= 0.0 || value.is_infinite() {
        fail(&format!("{name} needs a number above 0"));
    }
    value
}

/// A number from 0 to 1.
fn fraction(args: &mut impl Iterator<Item = String>, name: &str) -> f32 {
    let value: f32 = number(args, name);
    if !(0.0..=1.0).contains(&value) {
        fail(&format!("{name} is 0 to 1"));
    }
    value
}

fn point(args: &mut impl Iterator<Item = String>, name: &str) -> [f32; 3] {
    let text = args.next().unwrap_or_default();
    let values: Vec<f32> = text.split(',').filter_map(|v| v.parse().ok()).collect();
    values.try_into().unwrap_or_else(|_| fail(&format!("{name} needs X,Y,Z")))
}

fn render_command(mut args: impl Iterator<Item = String>) {
    let mut options = Options::default();
    let mut gpu = false;
    let (mut scene, mut output) = (None, None);
    let (mut position, mut target, mut zoom) = (None, None, None);
    let (mut iso, mut shutter, mut grain) = (far_core::lens::REFERENCE_ISO, far_core::lens::REFERENCE_SHUTTER, 0.0);
    let (mut focal_length, mut sensor_width, mut scene_unit, mut f_number) = (None, None, None, None::<f32>);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => output = args.next(),
            "--width" => options.width = number(&mut args, &a),
            "--height" => options.height = number(&mut args, &a),
            "--depth" => options.max_depth = number(&mut args, &a),
            "--glossy-depth" => options.glossy_depth = Some(number(&mut args, &a)),
            "--transmission-depth" => options.transmission_depth = Some(number(&mut args, &a)),
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
            "--noise-threshold" => options.noise_threshold = number(&mut args, &a),
            "--bloom" => options.bloom = true,
            "--gpu" => gpu = true,
            "--aperture" => options.aperture = Some(number(&mut args, &a)),
            "--focus-distance" => options.focus_distance = Some(number(&mut args, &a)),
            "--blades" => options.blades = Some(number(&mut args, &a)),
            "--blade-rotation" => options.blade_rotation = Some(number(&mut args, &a)),
            "--tone-mapping" => {
                let name = args.next().unwrap_or_default();
                options.tone_mapping = far_core::ToneMapping::parse(&name)
                    .unwrap_or_else(|| fail("--tone-mapping is standard, agx, agx-punchy or aces"));
            }
            "--camera" => position = Some(point(&mut args, &a)),
            "--target" => target = Some(point(&mut args, &a)),
            "--zoom" => zoom = Some(number(&mut args, &a)),
            "--focal-length" => focal_length = Some(positive(&mut args, &a)),
            "--sensor-width" => sensor_width = Some(positive(&mut args, &a)),
            "--scene-unit" => scene_unit = Some(positive(&mut args, &a)),
            "--f-number" => f_number = Some(positive(&mut args, &a)),
            "--exposure" => options.exposure = number(&mut args, &a),
            "--white-balance" => {
                options.white_balance = number(&mut args, &a);
                if !(far_core::lens::MIN_KELVIN..=far_core::lens::MAX_KELVIN).contains(&options.white_balance) {
                    fail("--white-balance is 1500 to 15000 kelvin");
                }
            }
            "--vignette" => options.vignette = fraction(&mut args, &a),
            "--iso" => {
                iso = number(&mut args, &a);
                if iso.is_nan() || iso <= 0.0 || iso > 1e6 {
                    fail("--iso needs a positive number");
                }
            }
            "--shutter" => {
                let text = args.next().unwrap_or_default();
                shutter = match text.split_once('/') {
                    Some((top, bottom)) => top.trim().parse::<f32>().ok().zip(bottom.trim().parse::<f32>().ok()).map(|(t, b)| t / b),
                    None => text.trim().parse().ok(),
                }
                .filter(|t: &f32| t.is_finite() && *t > 0.0)
                .unwrap_or_else(|| fail("--shutter needs a time in seconds, such as 1/60 or 0.5"));
            }
            "--tint" => {
                options.tint = number(&mut args, &a);
                if !(-1.0..=1.0).contains(&options.tint) {
                    fail("--tint is -1 to 1");
                }
            }
            "--grain" => grain = fraction(&mut args, &a),
            "--halation" => options.halation = fraction(&mut args, &a),
            "--chromatic-aberration" => options.chromatic_aberration = fraction(&mut args, &a),
            "--lut" => {
                let path = args.next().unwrap_or_else(|| fail("--lut needs a .cube file"));
                let lut = far_core::lut::Lut::open(std::path::Path::new(&path)).unwrap_or_else(|e| fail(&e));
                options.lut = Some(std::sync::Arc::new(lut));
            }
            "--lut-strength" => options.lut_strength = fraction(&mut args, &a),
            "--caustics" => options.caustics = true,
            "--photons" => options.photons = number(&mut args, &a),
            "--caustic-radius" => options.caustic_radius = number(&mut args, &a),
            "--caustic-strength" => options.caustic_strength = number(&mut args, &a),
            "--caustics-only" => options.caustics_only = true,
            "--region" => {
                let text = args.next().unwrap_or_default();
                let values: Vec<u32> = text.split(',').filter_map(|v| v.parse().ok()).collect();
                let [x0, y0, x1, y1] = values[..] else { fail("--region needs X0,Y0,X1,Y1") };
                options.region = Some(far_core::engine::Region { x0, y0, x1, y1 });
            }
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
    if zoom.is_some() && focal_length.is_some() {
        fail("--zoom and --focal-length both set the field of view; give one");
    }
    if options.aperture.is_some() && f_number.is_some() {
        fail("--aperture and --f-number both set the lens opening; give one");
    }
    // The camera's ISO, shutter and f-number join --exposure, and the ISO sets the grain's
    // strength.
    options.exposure *= far_core::lens::exposure(iso, shutter, f_number.unwrap_or(far_core::lens::REFERENCE_F_NUMBER));
    options.grain = far_core::lens::grain_at_iso(grain, iso);

    let t0 = Instant::now();
    let scene = Scene::open(Path::new(&scene)).unwrap_or_else(|e| fail(&e));
    let sensor_width = sensor_width.unwrap_or(scene.camera.sensor_width);
    let scene_unit = scene_unit.unwrap_or(scene.camera.scene_unit);
    let zoom = zoom.or(focal_length.map(|f| far_core::camera::zoom_for(f, sensor_width)));
    if let Some(f_number) = f_number {
        let focal = far_core::camera::focal_length(zoom.unwrap_or(scene.camera.zoom), sensor_width);
        options.aperture = Some(far_core::camera::aperture_for(focal, f_number, scene_unit));
    }
    let camera = far_core::Camera {
        sensor_width,
        scene_unit,
        aperture: options.aperture.unwrap_or(scene.camera.aperture),
        focus_distance: options.focus_distance.unwrap_or(scene.camera.focus_distance),
        blades: options.blades.unwrap_or(scene.camera.blades),
        blade_rotation: options.blade_rotation.unwrap_or(scene.camera.blade_rotation),
        position: position.unwrap_or(scene.camera.position),
        target: target.unwrap_or(scene.camera.target),
        zoom: zoom.unwrap_or(scene.camera.zoom),
    };
    camera.validate().unwrap_or_else(|e| fail(&e));
    options.pose = Some(camera.pose());
    for warning in &scene.warnings {
        eprintln!("warning: {warning}");
    }
    if options.denoise {
        load_denoiser();
    }
    let exr = is_exr(&output);
    if exr && options.mode != Mode::Enhanced {
        fail("an EXR file needs --enhanced, whose light is linear");
    }
    if gpu {
        return render_on_gpu(&scene, &options, &output, t0);
    }
    let t1 = Instant::now();
    let kd = options.use_kd.then(|| KdTree::for_mode(&scene, options.mode));
    let t2 = Instant::now();
    let image = Image::new(options.width, options.height);
    let progress = Progress::default();
    let accumulation = if exr {
        far_core::enhanced::render_continuing(&scene, kd.as_ref(), &options, &image, &progress, None)
    } else {
        render(&scene, kd.as_ref(), &options, &image, &progress);
        None
    };
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
    if let Some(denoiser) = *progress.denoiser.lock().unwrap() {
        println!("denoiser={}", if denoiser == far_core::engine::Denoiser::Oidn { "oidn" } else { "builtin" });
    }
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
    save(&image, accumulation.as_ref(), &options, &output);
}

fn is_exr(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("exr"))
}

/// Writes the image, or for an .exr file the light and passes `accumulation` holds.
fn save(image: &Image, accumulation: Option<&far_core::enhanced::Accumulation>, options: &Options, output: &str) {
    if is_exr(output) {
        let accumulation = accumulation.unwrap_or_else(|| fail("the render kept no light to save as EXR"));
        return accumulation.layers(options).write_exr(Path::new(output)).unwrap_or_else(|e| fail(&e));
    }
    let rgb: Vec<u8> = image.rgba().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    image::RgbImage::from_raw(options.width, options.height, rgb)
        .expect("image size")
        .save(output)
        .unwrap_or_else(|e| fail(&format!("{output}: {e}")));
}

/// Renders with the GPU path tracer and shows the result through the CPU's tone mapping.
fn render_on_gpu(scene: &Scene, options: &Options, output: &str, t0: Instant) {
    let t1 = Instant::now();
    let bvh = far_core::bvh::Bvh::build(scene);
    let t2 = Instant::now();
    let rendered = far_gpu::render(scene, &bvh, options, &|| false).unwrap_or_else(|e| fail(&e));
    let t3 = Instant::now();
    println!(
        "primitives={} lights={} load={}ms bvh={}ms render={}ms gpu={}",
        scene.prims.len(),
        scene.lights.len() + scene.rect_lights.len() + scene.bulbs.len(),
        (t1 - t0).as_millis(),
        (t2 - t1).as_millis(),
        (t3 - t2).as_millis(),
        rendered.adapter
    );
    for note in &rendered.notes {
        println!("note: the GPU renderer leaves out {note}");
    }
    // Shown as the CPU shows its own light: tone mapping, then the denoiser and bloom. Outside
    // a region the image stays black, as on the CPU.
    let image = far_core::Image::new(options.width, options.height);
    let progress = far_core::Progress::default();
    let accumulation = rendered.sums.accumulation(options);
    far_core::enhanced::redisplay(&accumulation, scene, options, &image, &progress);
    if let Some(denoiser) = *progress.denoiser.lock().unwrap() {
        println!("denoiser={}", if denoiser == far_core::engine::Denoiser::Oidn { "oidn" } else { "builtin" });
    }
    save(&image, Some(&accumulation), options, output);
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

/// Loads Intel Open Image Denoise from `FAR_OIDN_DIR`, or from the folder `tools/fetch_oidn.sh`
/// fills in a checkout. Without it, the engine's own filter denoises.
fn load_denoiser() {
    let folder = std::env::var_os("FAR_OIDN_DIR").map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/oidn/lib"));
    if let Err(e) = far_core::oidn::load(&folder) {
        eprintln!("note: denoising without Open Image Denoise ({e})");
    }
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
    let (mut different, mut visible, mut largest, mut squares) = (0u64, 0u64, 0i32, 0f64);
    for y in 1..h {
        for x in 1..w {
            let (p, q) = (a.get_pixel(x, y), b.get_pixel(x, y));
            let d = (0..3).map(|i| (p[i] as i32 - q[i] as i32).abs()).max().unwrap_or(0);
            different += (d > 0) as u64;
            squares += (0..3).map(|i| (p[i] as f64 - q[i] as f64).powi(2)).sum::<f64>() / 3.0;
            visible += (d > 16) as u64;
            largest = largest.max(d);
        }
    }
    let total = ((w - 1) * (h - 1)) as f64;
    println!(
        "different={different} ({:.3}%) above_16={visible} ({:.3}%) largest_difference={largest} rmse={:.3}",
        100.0 * different as f64 / total,
        100.0 * visible as f64 / total,
        (squares / total).sqrt()
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

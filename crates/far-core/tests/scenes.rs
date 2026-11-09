//! Opens every bundled scene and renders it small in every mode.

use far_core::{render, Image, KdTree, Mode, Options, Progress, Scene};
use std::path::{Path, PathBuf};

fn scene_files(folder: &Path, files: &mut Vec<PathBuf>) {
    for path in std::fs::read_dir(folder).unwrap().flatten().map(|e| e.path()) {
        if path.is_dir() {
            scene_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "far") {
            files.push(path);
        }
    }
}

fn all_scenes() -> Vec<PathBuf> {
    let mut files = Vec::new();
    scene_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes"), &mut files);
    assert!(files.len() >= 20, "bundled scenes are missing");
    files
}

#[test]
fn every_scene_opens_without_warnings() {
    for path in all_scenes() {
        let scene = Scene::open(&path).unwrap();
        assert!(scene.warnings.is_empty(), "{}: {:?}", path.display(), scene.warnings);
    }
}

#[test]
fn every_scene_renders_something_in_every_mode() {
    for path in all_scenes() {
        let scene = Scene::open(&path).unwrap();
        let kd = KdTree::build(&scene);
        for mode in [Mode::Classic, Mode::Fixed, Mode::Enhanced] {
            let options = Options { mode, width: 64, height: 48, samples: 1, ..Options::default() };
            let image = Image::new(options.width, options.height);
            render(&scene, Some(&kd), &options, &image, &Progress::default());
            let lit = image.rgba().chunks_exact(4).any(|p| p[0] > 0 || p[1] > 0 || p[2] > 0);
            let can_be_black = (scene.lights.is_empty() && scene.rect_lights.is_empty()) || scene.prims.is_empty();
            assert!(lit || can_be_black, "{} is all black in {mode:?}", path.display());
        }
    }
}

#[test]
fn enhanced_render_does_not_depend_on_thread_count() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes/2008/cube.far");
    let scene = Scene::open(&path).unwrap();
    let kd = KdTree::build(&scene);
    let pixels = |threads| {
        let options = Options {
            mode: Mode::Enhanced,
            width: 64,
            height: 48,
            samples: 4,
            threads,
            ..Options::default()
        };
        let image = Image::new(options.width, options.height);
        render(&scene, Some(&kd), &options, &image, &Progress::default());
        image.rgba()
    };
    assert_eq!(pixels(1), pixels(8));
}

#[test]
fn enhanced_mode_has_no_black_patches_on_meshes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes/2008/rhino.far");
    let scene = Scene::open(&path).unwrap();
    assert!(scene.corner_normals.len() == scene.prims.len());
    assert!(scene.corner_normals.iter().flatten().all(|n| n.x.is_finite() && n.y.is_finite() && n.z.is_finite()));
}

/// A scene by its file name, wherever it sits under `assets/scenes`: the examples are sorted
/// into folders, and a test should not break when they move.
fn scene_path(name: &str) -> std::path::PathBuf {
    fn find(folder: &std::path::Path, file: &str) -> Option<std::path::PathBuf> {
        let mut entries: Vec<_> = std::fs::read_dir(folder).ok()?.flatten().map(|e| e.path()).collect();
        entries.sort();
        entries.iter().find(|p| p.file_name().is_some_and(|n| n == file)).cloned()
            .or_else(|| entries.iter().filter(|p| p.is_dir()).find_map(|p| find(p, file)))
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes");
    let file = name.rsplit('/').next().unwrap_or(name);
    find(&root, file).unwrap_or_else(|| root.join(name))
}

fn render_small(scene: &Scene, kd: Option<&KdTree>, mode: Mode) -> Vec<u8> {
    let options = Options { mode, width: 200, height: 150, ..Options::default() };
    let image = Image::new(options.width, options.height);
    render(scene, kd, &options, &image, &Progress::default());
    image.rgba()
}

fn largest_difference(a: &[u8], b: &[u8]) -> u8 {
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

/// No bug touches these scenes, so Fixed must not change them. This is what shows
/// that Fixed keeps the 2008 algorithm instead of being a different renderer.
#[test]
fn fixed_matches_classic_where_no_bug_applies() {
    for name in ["simple-sphere.far", "plane-only.far", "test-2.far", "test-123.far"] {
        let scene = Scene::open(&scene_path(name)).unwrap();
        let kd = KdTree::build(&scene);
        let classic = render_small(&scene, Some(&kd), Mode::Classic);
        let fixed = render_small(&scene, Some(&kd), Mode::Fixed);
        assert!(largest_difference(&classic, &fixed) <= 1, "{name} changed in Fixed mode");
    }
}

#[test]
fn fixed_changes_scenes_with_mesh_bugs() {
    let scene = Scene::open(&scene_path("tests-01/scene-b.far")).unwrap();
    let kd = KdTree::build(&scene);
    let classic = render_small(&scene, Some(&kd), Mode::Classic);
    let fixed = render_small(&scene, Some(&kd), Mode::Fixed);
    assert!(largest_difference(&classic, &fixed) > 16);
}

/// In Classic mode the two paths shade differently. That is a bug, so in Fixed mode
/// the KD-Tree may only change the speed.
#[test]
fn fixed_image_does_not_depend_on_the_kd_tree() {
    let scene = Scene::open(&scene_path("tests-01/scene-b.far")).unwrap();
    let kd = KdTree::build(&scene);
    let with_kd = render_small(&scene, Some(&kd), Mode::Fixed);
    let without_kd = render_small(&scene, None, Mode::Fixed);
    let different = with_kd.iter().zip(&without_kd).filter(|(a, b)| a.abs_diff(**b) > 1).count();
    assert!(different * 1000 < with_kd.len(), "{different} values differ");
}

#[test]
fn every_mode_handles_sizes_that_tiles_do_not_divide() {
    let scene = Scene::open(&scene_path("cube.far")).unwrap();
    let kd = KdTree::build(&scene);
    for mode in [Mode::Classic, Mode::Fixed, Mode::Enhanced] {
        for (width, height) in [(101, 67), (64, 200), (333, 201)] {
            let options = Options { mode, width, height, samples: 1, ..Options::default() };
            let image = Image::new(width, height);
            render(&scene, Some(&kd), &options, &image, &Progress::default());
            let lit = image.rgba().chunks_exact(4).filter(|p| p[0] > 0 || p[1] > 0 || p[2] > 0).count();
            assert!(lit * 2 > (width * height) as usize, "{mode:?} at {width}x{height}");
        }
    }
}

#[test]
fn enhanced_mode_uses_the_exact_sample_count() {
    let scene = Scene::open(&scene_path("simple-sphere.far")).unwrap();
    let kd = KdTree::build(&scene);
    for samples in [1, 2, 5, 50] {
        let options = Options { mode: Mode::Enhanced, width: 64, height: 64, samples, ..Options::default() };
        let (image, progress) = (Image::new(64, 64), Progress::default());
        render(&scene, Some(&kd), &options, &image, &progress);
        let passes = progress.pass_index.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(passes, samples as usize);
    }
}

#[test]
fn caustics_do_not_depend_on_thread_count() {
    let scene = Scene::open(&scene_path("caustics.far")).unwrap();
    let kd = KdTree::build(&scene);
    let pixels = |threads| {
        let options = Options {
            mode: Mode::Enhanced,
            width: 64,
            height: 48,
            samples: 4,
            threads,
            caustics: true,
            photons: 50_000,
            ..Options::default()
        };
        let image = Image::new(options.width, options.height);
        render(&scene, Some(&kd), &options, &image, &Progress::default());
        image.rgba()
    };
    assert_eq!(pixels(1), pixels(8));
}

/// Without caustics, glass lets light through evenly. With them it must cast a real
/// shadow and also focus light, so some pixels get darker and others much brighter.
#[test]
fn caustics_cast_a_shadow_and_focus_light() {
    let scene = Scene::open(&scene_path("caustics.far")).unwrap();
    let kd = KdTree::build(&scene);
    let pixels = |caustics| {
        let options = Options {
            mode: Mode::Enhanced,
            width: 160,
            height: 120,
            samples: 4,
            indirect: false,
            caustics,
            photons: 500_000,
            ..Options::default()
        };
        let image = Image::new(options.width, options.height);
        render(&scene, Some(&kd), &options, &image, &Progress::default());
        image.rgba()
    };
    let (off, on) = (pixels(false), pixels(true));
    let darker = off.iter().zip(&on).filter(|(a, b)| **a as i32 > **b as i32 + 30).count();
    let brighter = off.iter().zip(&on).filter(|(a, b)| **b as i32 > **a as i32 + 30).count();
    assert!(darker > 100, "no shadow: {darker}");
    assert!(brighter > 20, "no focused light: {brighter}");
}

#[test]
fn a_far_scene_holds_every_primitive() {
    let scene = Scene::open(&scene_path("primitives.far")).unwrap();
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    assert_eq!(scene.lights.len(), 1);
    assert_eq!(scene.prims.len(), 4 + 5 + 1);
    assert_eq!(scene.corner_normals.len(), scene.prims.len());
}

/// Solids are placed in KD-Tree cells by their bounding box, so a mistake there would
/// lose hits and only show up as a difference from testing every primitive.
#[test]
fn solids_look_the_same_with_and_without_the_kd_tree() {
    let scene = Scene::open(&scene_path("primitives.far")).unwrap();
    let kd = KdTree::build(&scene);
    let with_kd = render_small(&scene, Some(&kd), Mode::Fixed);
    let without_kd = render_small(&scene, None, Mode::Fixed);
    let different = with_kd.iter().zip(&without_kd).filter(|(a, b)| a.abs_diff(**b) > 1).count();
    assert!(different * 1000 < with_kd.len(), "{different} values differ");
}

#[test]
fn a_bad_far_file_gives_a_clear_error() {
    let folder = std::env::temp_dir().join(format!("far-test-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let cases = [
        (r#"{"lights": [], "objects": [{"type": "pyramid"}]}"#, "pyramid"),
        (r#"{"lights": [], "objects": [{"type": "box", "center": [0,0,0], "size": [1,-1,1]}]}"#, "positive"),
        (r#"{"lights": [], "objects": [{"type": "tube", "center": [0,0,0], "radius": 1, "inner_radius": 2, "height": 1}]}"#, "inner_radius"),
    ];
    for (i, (text, expected)) in cases.iter().enumerate() {
        let path = folder.join(format!("{i}.far"));
        std::fs::write(&path, text).unwrap();
        let error = Scene::open(&path).err().expect("the scene must be rejected");
        assert!(error.contains(expected), "{error}");
    }
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A ball sealed inside a glass sphere, with the light outside. Fixed mode has opaque
/// shadows, so no light can reach the ball and the whole image must be black. The test
/// in the original missed rays leaving a sphere away from its center, which let light in.
#[test]
fn rays_leaving_a_sphere_away_from_its_center_still_hit_it() {
    let folder = std::env::temp_dir().join(format!("far-sealed-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("sealed.far");
    std::fs::write(
        &path,
        r#"{
            "lights": [{ "position": [9, 0, 8], "radius": 0.1 }],
            "objects": [
                { "type": "sphere", "center": [0, 0, 8], "radius": 3,
                  "material": { "diffuse": 0, "specular": 0, "refraction": 1 } },
                { "type": "sphere", "center": [1.2, 0, 8], "radius": 0.8 }
            ]
        }"#,
    )
    .unwrap();
    let scene = Scene::open(&path).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    let kd = KdTree::build(&scene);
    let lit = |kd: Option<&KdTree>, mode| {
        render_small(&scene, kd, mode).chunks_exact(4).filter(|p| p[0] > 0).count()
    };
    assert_eq!(lit(Some(&kd), Mode::Fixed), 0, "light got through with the KD-Tree");
    assert_eq!(lit(None, Mode::Fixed), 0, "light got through without the KD-Tree");
    assert!(lit(Some(&kd), Mode::Classic) > 0, "the 2008 stage must keep the original bug");
}

#[test]
fn photon_stats_describe_the_pass() {
    let scene = Scene::open(&scene_path("caustics.far")).unwrap();
    let kd = KdTree::build(&scene);
    let options = Options {
        mode: Mode::Enhanced,
        width: 64,
        height: 48,
        samples: 2,
        caustics: true,
        photons: 100_000,
        ..Options::default()
    };
    let (image, progress) = (Image::new(64, 48), Progress::default());
    render(&scene, Some(&kd), &options, &image, &progress);
    let stats = progress.photon_stats.lock().unwrap().expect("stats");
    assert_eq!(stats.emitted, 100_000);
    assert!(stats.stored > 1000 && stats.stored < 200_000, "{}", stats.stored);
    assert!(stats.memory_bytes >= stats.stored * 36);
    let lookups = progress.photon_lookups.load(std::sync::atomic::Ordering::Relaxed);
    assert!(lookups > 1000);
}

#[test]
fn photon_preview_does_not_change_the_render() {
    use std::sync::atomic::Ordering::Relaxed;
    let scene = Scene::open(&scene_path("caustics.far")).unwrap();
    let kd = KdTree::build(&scene);
    let mut options = Options {
        mode: Mode::Enhanced, width: 64, height: 48, samples: 1,
        caustics: true, photons: 20_000, ..Options::default()
    };
    let progress = Progress::default();
    let image = Image::new(64, 48);
    render(&scene, Some(&kd), &options, &image, &progress);
    let baseline = image.rgba();
    assert!(progress.photon_preview.lock().unwrap().is_none());
    progress.capture_photon_preview.store(true, Relaxed);
    render(&scene, Some(&kd), &options, &image, &progress);
    assert_eq!(baseline, image.rgba());
    {
        let preview = progress.photon_preview.lock().unwrap();
        let preview = preview.as_ref().expect("photon preview");
        assert_eq!((preview.width, preview.height), (64, 48));
        assert!(preview.rgba().chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
    }
    options.caustics = false;
    render(&scene, Some(&kd), &options, &image, &progress);
    assert!(progress.photon_preview.lock().unwrap().is_none(), "old map must not survive a new render");
}

#[test]
fn caustics_only_shows_nothing_but_the_focused_light() {
    let scene = Scene::open(&scene_path("caustics.far")).unwrap();
    let kd = KdTree::build(&scene);
    let lit = |caustics_only: bool| {
        let options = Options {
            mode: Mode::Enhanced,
            width: 160,
            height: 120,
            samples: 2,
            caustics: true,
            caustics_only,
            photons: 300_000,
            ..Options::default()
        };
        let image = Image::new(160, 120);
        render(&scene, Some(&kd), &options, &image, &Progress::default());
        image.rgba().chunks_exact(4).filter(|p| p[0] > 8).count()
    };
    let (full, only) = (lit(false), lit(true));
    assert!(only > 0, "the caustic disappeared");
    assert!(only * 4 < full, "direct light is still there: {only} of {full}");
}

#[test]
fn rectangular_lights_emit_caustic_photons_deterministically() {
    use std::sync::atomic::Ordering::Relaxed;
    let scene = Scene::open(&scene_path("rectangular-lights.far")).unwrap();
    assert_eq!(scene.rect_lights.len(), 2);
    let kd = KdTree::build(&scene);
    let render_with = |threads, tree| {
        let options = Options { mode: Mode::Enhanced, width: 64, height: 48, samples: 4,
            caustics: true, photons: 50_000, threads, ..Options::default() };
        let image = Image::new(64, 48);
        let progress = Progress::default();
        progress.capture_photon_preview.store(true, Relaxed);
        render(&scene, tree, &options, &image, &progress);
        let stored = progress.photon_stats.lock().unwrap().unwrap().stored;
        assert!(stored > 100, "rectangle emitted no useful caustics: {stored}");
        assert!(progress.photon_preview.lock().unwrap().is_some());
        (image.rgba(), stored)
    };
    let serial = render_with(1, Some(&kd));
    assert_eq!(serial, render_with(4, Some(&kd)));
    let without_kd = render_with(1, None);
    assert_eq!(serial.1, without_kd.1);
    assert!(serial.0.iter().zip(without_kd.0).all(|(a, b)| a.abs_diff(b) <= 1));
}

#[test]
fn depth_of_field_is_deterministic_and_only_affects_enhanced() {
    let scene = Scene::open(&scene_path("depth-of-field.far")).unwrap();
    let kd = KdTree::build(&scene);
    let pixels = |mode, aperture, focus_distance, threads| {
        let options = Options { mode, aperture, focus_distance, threads,
            width: 96, height: 72, samples: 64, ..Options::default() };
        let image = Image::new(96, 72);
        render(&scene, Some(&kd), &options, &image, &Progress::default());
        image.rgba()
    };
    let lens = pixels(Mode::Enhanced, None, None, 1);
    assert_eq!(lens, pixels(Mode::Enhanced, None, None, 4));
    let pinhole = pixels(Mode::Enhanced, Some(0.0), None, 1);
    assert_ne!(lens, pinhole);
    assert_eq!(pinhole, pixels(Mode::Enhanced, Some(0.0), Some(30.0), 1));
    assert_ne!(lens, pixels(Mode::Enhanced, None, Some(8.0), 1));
    for mode in [Mode::Classic, Mode::Fixed] {
        assert_eq!(pixels(mode, None, None, 1), pixels(mode, Some(0.0), Some(30.0), 1));
    }
}

#[test]
fn large_spherical_photon_pass_does_not_select_a_nonexistent_rectangle() {
    let scene = Scene::open(&scene_path("caustics.far")).unwrap();
    assert!(scene.rect_lights.is_empty());
    let kd = KdTree::build(&scene);
    let options = Options { mode: Mode::Enhanced, width: 16, height: 16, samples: 1,
        caustics: true, photons: 16_000_000, ..Options::default() };
    let image = Image::new(16, 16);
    let progress = Progress::default();
    render(&scene, Some(&kd), &options, &image, &progress);
    assert!(progress.photon_stats.lock().unwrap().unwrap().stored > 0);
}

#[test]
fn photon_pass_honors_cancellation() {
    use std::sync::atomic::Ordering::Relaxed;
    let scene = Scene::open(&scene_path("caustics.far")).unwrap();
    let options = Options { mode: Mode::Enhanced, width: 16, height: 16, samples: 1,
        caustics: true, photons: 16_000_000, ..Options::default() };
    let progress = Progress::default();
    let image = Image::new(16, 16);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| render(&scene, None, &options, &image, &progress));
        while progress.tiles_total.load(Relaxed) == 0 { std::thread::yield_now(); }
        let start = std::time::Instant::now();
        progress.cancel.store(true, Relaxed);
        worker.join().unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(20));
    });
    assert_eq!(progress.photon_stats.lock().unwrap().unwrap().stored, 0);
    assert_eq!(progress.pass_index.load(Relaxed), 0);
}

#[test]
fn ggx_overrides_are_isolated_and_render_deterministically() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes/materials/ggx-materials.far");
    let original = Scene::open(&path).unwrap();
    let mut edited = original.clone();
    let id = edited.prims.last().unwrap().material();
    let surface: far_core::surface::Surface = serde_json::from_str(
        r#"{"preset":"plastic","roughness":0.2,"color":[0.1,0.5,0.8]}"#
    ).unwrap();
    surface.apply(&mut edited.materials[id]);
    assert_eq!(original.materials[id].ggx.unwrap().metallic, 1.0);
    assert_eq!(edited.materials[id].ggx.unwrap().metallic, 0.0);
    let kd = KdTree::build(&original);
    let pixels = |scene: &Scene, threads| {
        let options = Options { mode: Mode::Enhanced, width: 64, height: 48,
            samples: 16, threads, ..Options::default() };
        let image = Image::new(64, 48);
        render(scene, Some(&kd), &options, &image, &Progress::default());
        image.rgba()
    };
    assert_eq!(pixels(&edited, 1), pixels(&edited, 4));
    assert_ne!(pixels(&original, 1), pixels(&edited, 1));
}

#[test]
fn hdri_render_controls_are_deterministic_and_preserve_historical_modes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes/techniques/hdri-studio.far");
    let scene = Scene::open(&path).unwrap();
    let kd = KdTree::build(&scene);
    let pixels = |scene: &Scene, mode, threads| {
        let options = Options { mode, width: 64, height: 48, samples: 32,
            threads, ..Options::default() };
        let image = Image::new(64, 48);
        render(scene, Some(&kd), &options, &image, &Progress::default());
        image.rgba()
    };
    let hidden = pixels(&scene, Mode::Enhanced, 1);
    assert_eq!(hidden, pixels(&scene, Mode::Enhanced, 4));
    let mut visible = scene.clone();
    visible.environment.as_mut().unwrap().config.visible = true;
    let shown = pixels(&visible, Mode::Enhanced, 1);
    // Top center is background; center is an opaque sphere.
    let pixel = |x: usize, y: usize| (y * 64 + x) * 4;
    let sky = pixel(32, 2);
    let object = pixel(32, 26);
    assert_eq!(&hidden[sky..sky+3], &[0, 0, 0]);
    assert_ne!(&shown[sky..sky+3], &[0, 0, 0]);
    assert_eq!(&hidden[object..object+4], &shown[object..object+4]);
    let mut rotated = scene.clone();
    rotated.environment.as_mut().unwrap().config.rotation = 90.;
    assert_ne!(hidden, pixels(&rotated, Mode::Enhanced, 1));
    let mut disabled = scene.clone();
    disabled.environment.as_mut().unwrap().config.enabled = false;
    let dark = pixels(&disabled, Mode::Enhanced, 1);
    assert!(dark.chunks_exact(4).all(|p| p[0..3] == [0, 0, 0]));
    let mut absent = scene.clone();
    absent.environment = None;
    for mode in [Mode::Classic, Mode::Fixed] {
        assert_eq!(pixels(&visible, mode, 1), pixels(&absent, mode, 1));
    }
}

#[test]
fn denoiser_is_deterministic_and_only_touches_enhanced() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes/techniques/hdri-studio.far");
    let scene = Scene::open(&path).unwrap();
    let kd = KdTree::build(&scene);
    let pixels = |mode, samples, denoise, threads| {
        let options = Options { mode, samples, denoise, threads, width: 64, height: 48, ..Options::default() };
        let image = Image::new(64, 48);
        render(&scene, Some(&kd), &options, &image, &Progress::default());
        image.rgba()
    };
    let filtered = pixels(Mode::Enhanced, 16, true, 1);
    assert_eq!(filtered, pixels(Mode::Enhanced, 16, true, 4));
    assert_ne!(filtered, pixels(Mode::Enhanced, 16, false, 1));
    // Too few passes to measure noise: the image is left alone.
    assert_eq!(pixels(Mode::Enhanced, 2, true, 1), pixels(Mode::Enhanced, 2, false, 1));
    for mode in [Mode::Classic, Mode::Fixed] {
        assert_eq!(pixels(mode, 16, true, 1), pixels(mode, 16, false, 1));
    }
}

#[test]
fn a_moved_camera_sees_the_same_with_and_without_the_kd_tree() {
    use far_core::Pose;
    let scene = Scene::open(&scene_path("primitives.far")).unwrap();
    let kd = KdTree::build(&scene);
    let pixels = |tree, pose: Pose| {
        let options = Options { mode: Mode::Enhanced, width: 96, height: 72, samples: 2, indirect: false,
            pose: Some(pose), ..Options::default() };
        let image = Image::new(96, 72);
        render(&scene, tree, &options, &image, &Progress::default());
        image.rgba()
    };
    // Inside the scene, outside it on each side, above and from far away.
    for position in [[3., 2., -4.], [40., 5., 5.], [-40., 2., 8.], [0., 30., 6.], [5., 3., 80.], [200., 100., -300.]] {
        let pose = Pose { position, target: [0., 0., 6.], zoom: 1.5 };
        let (with, without) = (pixels(Some(&kd), pose), pixels(None, pose));
        let different = with.iter().zip(&without).filter(|(a, b)| a.abs_diff(**b) > 2).count();
        assert!(different * 500 < with.len(), "{position:?}: {different} of {} differ", with.len());
    }
}

#[test]
fn only_enhanced_follows_a_moved_camera() {
    let scene = Scene::open(&scene_path("simple-sphere.far")).unwrap();
    let kd = KdTree::build(&scene);
    let pose = far_core::Pose { position: [4., 3., -6.], target: [0., 0., 5.], zoom: 1. };
    let pixels = |mode, pose| {
        let options = Options { mode, pose, width: 64, height: 48, samples: 4, ..Options::default() };
        let image = Image::new(64, 48);
        render(&scene, Some(&kd), &options, &image, &Progress::default());
        image.rgba()
    };
    assert_ne!(pixels(Mode::Enhanced, None), pixels(Mode::Enhanced, Some(pose)));
    assert_eq!(pixels(Mode::Enhanced, None), pixels(Mode::Enhanced, Some(far_core::Pose::default())));
    for mode in [Mode::Classic, Mode::Fixed] {
        assert_eq!(pixels(mode, None), pixels(mode, Some(pose)));
    }
}

/// A region renders exactly the pixels a whole render has there, denoised or not, so
/// switching between them shows no seam; outside it, the image is left alone.
#[test]
fn a_region_renders_the_same_pixels_as_the_whole_image() {
    let scene = Scene::open(&scene_path("cornell-box.far")).unwrap();
    let region = far_core::engine::Region { x0: 37, y0: 21, x1: 101, y1: 70 };
    for (mode, denoise) in [(Mode::Enhanced, false), (Mode::Enhanced, true), (Mode::Classic, false)] {
        let kd = KdTree::for_mode(&scene, mode);
        let whole_options = Options { mode, width: 320, height: 240, samples: 8, denoise, threads: 3, ..Options::default() };
        let whole = Image::new(320, 240);
        render(&scene, Some(&kd), &whole_options, &whole, &Progress::default());
        let part = Image::new(320, 240);
        let options = Options { region: Some(region), threads: 2, ..whole_options };
        let progress = Progress::default();
        render(&scene, Some(&kd), &options, &part, &progress);
        let (a, b) = (whole.rgba(), part.rgba());
        for y in 0..240usize {
            for x in 0..320usize {
                let p = (y * 320 + x) * 4;
                let inside = (37..101).contains(&x) && (21..70).contains(&y);
                if inside {
                    assert_eq!(a[p..p + 4], b[p..p + 4], "{mode:?}, denoise {denoise}: pixel {x},{y} differs");
                }
            }
        }
        // Far from the region nothing was traced.
        assert_eq!(b[(239 * 320 + 319) * 4..][..3], [0, 0, 0], "{mode:?}: the far corner stayed black");
        let tiles = progress.tiles_total.load(std::sync::atomic::Ordering::Relaxed);
        let whole_tiles = if mode == Mode::Classic { 100 } else { 80 * 8 };
        assert!(tiles * 3 < whole_tiles, "{mode:?}: {tiles} of {whole_tiles} tiles for a small region");
    }
}

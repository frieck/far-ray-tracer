//! Opens every bundled scene and renders it small in every mode.

use far_core::{render, Image, KdTree, Mode, Options, Progress, Scene};
use std::path::{Path, PathBuf};

fn frt_files(folder: &Path, files: &mut Vec<PathBuf>) {
    for path in std::fs::read_dir(folder).unwrap().flatten().map(|e| e.path()) {
        if path.is_dir() {
            frt_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "frt" || e == "far") {
            files.push(path);
        }
    }
}

fn all_scenes() -> Vec<PathBuf> {
    let mut files = Vec::new();
    frt_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes"), &mut files);
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
            let can_be_black = scene.lights.is_empty() || scene.prims.is_empty();
            assert!(lit || can_be_black, "{} is all black in {mode:?}", path.display());
        }
    }
}

#[test]
fn enhanced_render_does_not_depend_on_thread_count() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes/cube.frt");
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
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes/rhino.frt");
    let scene = Scene::open(&path).unwrap();
    assert!(scene.corner_normals.len() == scene.prims.len());
    assert!(scene.corner_normals.iter().flatten().all(|n| n.x.is_finite() && n.y.is_finite() && n.z.is_finite()));
}

fn scene_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes").join(name)
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
    for name in ["simple-sphere.frt", "plane-only.frt", "test-2.frt", "test-123.frt"] {
        let scene = Scene::open(&scene_path(name)).unwrap();
        let kd = KdTree::build(&scene);
        let classic = render_small(&scene, Some(&kd), Mode::Classic);
        let fixed = render_small(&scene, Some(&kd), Mode::Fixed);
        assert!(largest_difference(&classic, &fixed) <= 1, "{name} changed in Fixed mode");
    }
}

#[test]
fn fixed_changes_scenes_with_mesh_bugs() {
    let scene = Scene::open(&scene_path("tests-01/scene-b.frt")).unwrap();
    let kd = KdTree::build(&scene);
    let classic = render_small(&scene, Some(&kd), Mode::Classic);
    let fixed = render_small(&scene, Some(&kd), Mode::Fixed);
    assert!(largest_difference(&classic, &fixed) > 16);
}

/// In Classic mode the two paths shade differently. That is a bug, so in Fixed mode
/// the KD-Tree may only change the speed.
#[test]
fn fixed_image_does_not_depend_on_the_kd_tree() {
    let scene = Scene::open(&scene_path("tests-01/scene-b.frt")).unwrap();
    let kd = KdTree::build(&scene);
    let with_kd = render_small(&scene, Some(&kd), Mode::Fixed);
    let without_kd = render_small(&scene, None, Mode::Fixed);
    let different = with_kd.iter().zip(&without_kd).filter(|(a, b)| a.abs_diff(**b) > 1).count();
    assert!(different * 1000 < with_kd.len(), "{different} values differ");
}

#[test]
fn every_mode_handles_sizes_that_tiles_do_not_divide() {
    let scene = Scene::open(&scene_path("cube.frt")).unwrap();
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
    let scene = Scene::open(&scene_path("simple-sphere.frt")).unwrap();
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
    let scene = Scene::open(&scene_path("caustics.frt")).unwrap();
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
    let scene = Scene::open(&scene_path("caustics.frt")).unwrap();
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
    let scene = Scene::open(&scene_path("caustics.frt")).unwrap();
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
fn caustics_only_shows_nothing_but_the_focused_light() {
    let scene = Scene::open(&scene_path("caustics.frt")).unwrap();
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

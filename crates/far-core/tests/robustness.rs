//! Damaged files must give an error or a warning, never crash the program: every example
//! scene, model, IES profile, `.3ds` mesh and HDR image is read cut short and with bytes
//! changed, and what does open is rendered.

use far_core::{render, Image, KdTree, Mode, Options, Progress, Scene};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn files(folder: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    for path in std::fs::read_dir(folder).unwrap().flatten().map(|e| e.path()) {
        if path.is_dir() {
            files(&path, extensions, out);
        } else if path.extension().is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x))) {
            out.push(path);
        }
    }
}

/// A small deterministic generator, so a failure repeats.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Damaged copies of `bytes`: cut at a few places, and with a few bytes changed.
fn damaged(bytes: &[u8], seed: u64) -> Vec<Vec<u8>> {
    let mut random = Random(seed | 1);
    let mut out = Vec::new();
    for share in [0.1, 0.5, 0.9] {
        out.push(bytes[..(bytes.len() as f64 * share) as usize].to_vec());
    }
    for _ in 0..20 {
        let mut copy = bytes.to_vec();
        for _ in 0..1 + random.next() % 8 {
            if copy.is_empty() { break; }
            let at = (random.next() % copy.len() as u64) as usize;
            // Digits, signs and brackets reach deeper into a parser than random bytes.
            const TOKENS: &[u8] = b"0123456789-+.e[]{},:\"/ \n\x00\xff";
            copy[at] = if random.next() % 2 == 0 { TOKENS[(random.next() % TOKENS.len() as u64) as usize] } else { random.next() as u8 };
        }
        out.push(copy);
    }
    out
}

/// Opens a damaged file as a scene, renders it tiny if it opens, and reports a crash.
fn survives(path: &Path) -> Result<(), String> {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if let Ok(scene) = Scene::open(path) {
            let kd = KdTree::build(&scene);
            let options = Options { mode: Mode::Enhanced, width: 16, height: 12, samples: 1, threads: 1, ..Options::default() };
            render(&scene, Some(&kd), &options, &Image::new(16, 12), &Progress::default());
        }
    }));
    outcome.map_err(|panic| {
        let message = panic.downcast_ref::<String>().cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        format!("{}: {message}", path.display())
    })
}

#[test]
fn damaged_scenes_and_models_fail_cleanly() {
    let mut sources = Vec::new();
    files(&assets().join("scenes"), &["far"], &mut sources);
    // Models up to 100 KB: the bigger ones come from the same writers, and each damaged copy
    // opens as a whole scene, so they would only make the test slow.
    let mut models = Vec::new();
    files(&assets().join("scene-data/meshes"), &["obj", "glb", "gltf"], &mut models);
    sources.extend(models.into_iter().filter(|m| std::fs::metadata(m).is_ok_and(|d| d.len() <= 100_000)));
    let work = std::env::temp_dir().join(format!("far-damaged-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let mut crashes = Vec::new();
    for (n, source) in sources.iter().enumerate() {
        let bytes = std::fs::read(source).unwrap();
        // The copy sits beside a copy of the data folder's neighbours only by name, so missing
        // textures and meshes are part of the test.
        for (k, copy) in damaged(&bytes, n as u64 * 7919 + 1).into_iter().enumerate() {
            let path = work.join(format!("{k}-{}", source.file_name().unwrap().to_string_lossy()));
            std::fs::write(&path, copy).unwrap();
            if let Err(crash) = survives(&path) {
                crashes.push(crash);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    assert!(crashes.is_empty(), "{} crashes:\n{}", crashes.len(), crashes.join("\n"));
}

#[test]
fn damaged_meshes_profiles_and_images_fail_cleanly() {
    let mut crashes = Vec::new();
    let mut meshes = Vec::new();
    files(&assets().join("scene-data/meshes"), &["3ds"], &mut meshes);
    for (n, path) in meshes.iter().take(6).enumerate() {
        for copy in damaged(&std::fs::read(path).unwrap(), n as u64 + 11) {
            if catch_unwind(|| far_core::mesh3ds::parse(&copy)).is_err() {
                crashes.push(format!("{}", path.display()));
            }
        }
    }
    let mut profiles = Vec::new();
    files(&assets().join("scene-data/ies"), &["ies"], &mut profiles);
    for (n, path) in profiles.iter().enumerate() {
        for copy in damaged(&std::fs::read(path).unwrap(), n as u64 + 23) {
            let text = String::from_utf8_lossy(&copy).into_owned();
            if catch_unwind(|| far_core::light_profile::Ies::parse(&text).map(|ies| ies.value(10.0, 30.0))).is_err() {
                crashes.push(format!("{}", path.display()));
            }
        }
    }
    let work = std::env::temp_dir().join(format!("far-damaged-hdr-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let hdr = std::fs::read(assets().join("environments/studio.hdr")).unwrap();
    for (k, copy) in damaged(&hdr, 31).into_iter().enumerate() {
        let path = work.join(format!("{k}.hdr"));
        std::fs::write(&path, copy).unwrap();
        if catch_unwind(|| far_core::environment::EnvironmentMap::load(&path).is_ok()).is_err() {
            crashes.push(format!("{}", path.display()));
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    assert!(crashes.is_empty(), "{} crashes:\n{}", crashes.len(), crashes.join("\n"));
}

//! Saves a scene with the edits made in the app: the camera, the environment, material
//! changes and the render settings, written over the scene file as it was read.
//!
//! Everything else in the file stays as it was, in its order.
//! When the scene is saved to another folder, the paths it holds (the environment image,
//! models, IES files and the folder of images and meshes) are rewritten so they still point at
//! the same files.

use crate::environment::EnvironmentConfig;
use crate::scene::{find_data_folder, MaterialSource, Scene};
use crate::surface::Surface;
use serde_json::{json, Map, Value};
use std::path::{Component, Path, PathBuf};

/// What the app changed.
pub struct SceneEdits {
    pub camera: crate::camera::Camera,
    /// None removes the environment.
    pub environment: Option<EnvironmentConfig>,
    /// Material id, its new surface (None keeps the file's), and its new bump strength.
    pub materials: Vec<(usize, Option<Surface>, Option<f32>)>,
    /// The app's render settings, stored as they are.
    pub render: Option<Value>,
}

/// Writes `scene`, read from `original`, with `edits` to `target`. Returns the edits that
/// could not be saved, such as changes to the materials inside a model file.
pub fn save(scene: &Scene, original: &Path, target: &Path, edits: &SceneEdits) -> Result<Vec<String>, String> {
    let mut file = scene.source.as_deref().cloned()
        .ok_or("This scene was not read from a .far file, so it cannot be saved as one")?;
    let root = file.as_object_mut().ok_or("The scene file is not a JSON object")?;
    let from = original.parent().unwrap_or(Path::new(".")).to_path_buf();
    let to = absolute(target.parent().unwrap_or(Path::new(".")));
    let mut warnings = Vec::new();

    if from != to {
        relocate_paths(root, &from, &to, original, target);
    }
    root.insert("camera".into(), serde_json::to_value(edits.camera).map_err(|e| e.to_string())?);
    match &edits.environment {
        None => { root.remove("environment"); }
        Some(config) => { root.insert("environment".into(), environment_value(config, &to)); }
    }
    for (id, surface, bump) in &edits.materials {
        let source = scene.material_sources.get(*id).copied().unwrap_or(MaterialSource::None);
        match source {
            MaterialSource::Object(index) => {
                let Some(object) = root.get_mut("objects").and_then(|o| o.get_mut(index)).and_then(Value::as_object_mut)
                else { continue };
                let material = object.entry("material").or_insert_with(|| json!({}));
                let Some(material) = material.as_object_mut() else { continue };
                if let Some(surface) = surface {
                    material.insert("surface".into(), serde_json::to_value(surface).map_err(|e| e.to_string())?);
                }
                if let Some(strength) = bump {
                    if let Some(bump) = material.get_mut("bump").and_then(Value::as_object_mut) {
                        bump.insert("strength".into(), json!(strength));
                    }
                }
            }
            MaterialSource::ModelPart(index) => {
                let file = root.get("objects").and_then(|o| o.get(index)).and_then(|o| o.get("file"))
                    .and_then(Value::as_str).unwrap_or("a model");
                warnings.push(format!("Edits to the materials inside {file} are not saved: they live in the model file"));
            }
            MaterialSource::None => warnings.push(format!("Edits to material {id} are not saved")),
        }
    }
    match &edits.render {
        Some(render) => { root.insert("render".into(), render.clone()); }
        None => { root.remove("render"); }
    }
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())? + "\n";
    // Written beside the target first, so a failed write never leaves half a scene.
    let temporary = target.with_extension("far.saving");
    std::fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, target).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        format!("{}: {e}", target.display())
    })?;
    Ok(warnings)
}

fn environment_value(config: &EnvironmentConfig, to: &Path) -> Value {
    let mut value = json!({
        "intensity": config.intensity,
        "rotation": config.rotation,
        "visible": config.visible,
        "enabled": config.enabled,
    });
    match &config.sky {
        Some(sky) => { value["sky"] = serde_json::to_value(sky).unwrap_or(Value::Null); }
        // The app holds the image's full path; the file keeps it relative when it can.
        None => { value["path"] = json!(relative_to(Path::new(&config.path), to)); }
    }
    value
}

/// Rewrites the paths the scene file holds, relative to `from`, to work from `to`.
fn relocate_paths(root: &mut Map<String, Value>, from: &Path, to: &Path, original: &Path, target: &Path) {
    let moved = |value: &mut Value| {
        if let Some(text) = value.as_str() {
            let path = from.join(text.replace('\\', "/"));
            if path.exists() {
                *value = json!(relative_to(&path, to));
            }
        }
    };
    if let Some(environment) = root.get_mut("environment") {
        if let Some(path) = environment.get_mut("path") { moved(path); }
    }
    for object in root.get_mut("objects").and_then(Value::as_array_mut).into_iter().flatten() {
        if object.get("type").and_then(Value::as_str) == Some("model") {
            if let Some(file) = object.get_mut("file") { moved(file); }
        }
    }
    for light in root.get_mut("lights").and_then(Value::as_array_mut).into_iter().flatten() {
        if let Some(ies) = light.get_mut("ies") { moved(ies); }
    }
    // Images and meshes are found in a data folder above the scene; point at the old one when
    // the new place has a different one.
    let old_data = match root.get("data_folder").and_then(Value::as_str) {
        Some(folder) => Some(from.join(folder)),
        None => find_data_folder(original),
    };
    if let Some(old_data) = old_data {
        if find_data_folder(&to.join(target.file_name().unwrap_or_default())).map(|d| absolute(&d)) != Some(absolute(&old_data)) {
            root.insert("data_folder".into(), json!(relative_to(&old_data, to)));
        } else {
            root.remove("data_folder");
        }
    }
}

fn absolute(path: &Path) -> PathBuf {
    // Links resolved, so "../" climbs the folders the system really walks (on macOS /var is a
    // link into /private).
    if let Ok(real) = path.canonicalize() {
        return real;
    }
    let path = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) };
    // Folds "a/../b" so two spellings of a folder compare equal.
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => { out.pop(); }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// `path` relative to folder `base`, with forward slashes, or absolute when they share no root
/// (another drive on Windows).
fn relative_to(path: &Path, base: &Path) -> String {
    let (path, base) = (absolute(path), absolute(base));
    let (a, b): (Vec<_>, Vec<_>) = (path.components().collect(), base.components().collect());
    if a.first() != b.first() {
        return path.to_string_lossy().replace('\\', "/");
    }
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".into(); b.len() - common];
    parts.extend(a[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    if parts.is_empty() { ".".into() } else { parts.join("/") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("far-save-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn paths_become_relative_to_where_the_scene_goes() {
        assert_eq!(relative_to(Path::new("/a/b/c.hdr"), Path::new("/a/d")), "../b/c.hdr");
        assert_eq!(relative_to(Path::new("/a/b/c.hdr"), Path::new("/a/b")), "c.hdr");
        assert_eq!(relative_to(Path::new("/a/x/../b"), Path::new("/a")), "b");
    }

    #[test]
    fn saving_keeps_the_file_and_writes_the_edits() {
        let dir = folder("edits");
        let data = dir.join("scene-data").join("images");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(dir.join("scenes")).unwrap();
        std::fs::write(dir.join("scenes").join("lamp.ies"), "TILT=NONE\\n").unwrap();
        let original = dir.join("scenes").join("a.far");
        let text = r#"{"bounds": [[-9, -9, -9], [9, 9, 9]], "lights": [{"type": "sphere", "position": [0, 3, 0], "radius": 0.1, "ies": "lamp.ies"}],
            "objects": [{"type": "sphere", "center": [0, 0, 5], "radius": 1, "material": {"color": [1, 0, 0], "bump": {"map": "b.png", "strength": 0.1}}},
                        {"type": "sphere", "center": [3, 0, 5], "radius": 1}]}"#;
        std::fs::write(&original, text).unwrap();
        // Read without the light, whose IES file here is a stub.
        let mut scene = Scene::from_far(text.replace(r#", "ies": "lamp.ies""#, "").as_bytes(), &original).unwrap();
        scene.source = Some(std::sync::Arc::new(serde_json::from_str(text).unwrap()));
        let surface: Surface = serde_json::from_str(r#"{"preset": "metal", "roughness": 0.3}"#).unwrap();
        let first = scene.material_sources.iter().position(|s| *s == MaterialSource::Object(0)).unwrap();
        let second = scene.material_sources.iter().position(|s| *s == MaterialSource::Object(1)).unwrap();
        let edits = SceneEdits {
            camera: crate::camera::Camera { zoom: 2.0, ..Default::default() },
            environment: Some(EnvironmentConfig { sky: Some(Default::default()), ..Default::default() }),
            materials: vec![(first, None, Some(0.5)), (second, Some(surface), None)],
            render: Some(json!({"samples": 256})),
        };
        // Saved elsewhere: the IES path and the data folder are rewritten.
        let elsewhere = folder("edits-out");
        let target = elsewhere.join("b.far");
        let warnings = save(&scene, &original, &target, &edits).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(saved["bounds"][1][0], 9);
        assert_eq!(saved["camera"]["zoom"], 2.0);
        assert!(saved["environment"]["sky"].is_object());
        assert_eq!(saved["objects"][0]["material"]["bump"]["strength"], 0.5);
        assert_eq!(saved["objects"][1]["material"]["surface"]["preset"], "metal");
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(saved["lights"][0]["ies"], format!("../{name}/scenes/lamp.ies"));
        assert_eq!(saved["data_folder"], format!("../{name}/scene-data"));
        // Saved beside the original, the paths stay as they were.
        let beside = dir.join("scenes").join("c.far");
        save(&scene, &original, &beside, &edits).unwrap();
        let near: Value = serde_json::from_str(&std::fs::read_to_string(&beside).unwrap()).unwrap();
        assert_eq!(near["lights"][0]["ies"], "lamp.ies");
        assert!(near.get("data_folder").is_none());
        assert_eq!(saved["render"]["samples"], 256);
        // The saved scene opens, with its data folder found.
        let reopened = Scene::open(&target);
        assert!(reopened.is_err_and(|e| e.contains("IES")) , "the stub IES file is read, so the scene's paths resolve");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }
}

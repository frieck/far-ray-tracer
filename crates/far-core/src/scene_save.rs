//! Saves a scene with the edits made in the app: the camera, the environment, material
//! changes and the render settings, written over the scene file as it was read.
//!
//! Everything else in the file stays as it was, in its order.
//! When the scene is saved to another folder, the paths it holds (the environment image,
//! models, IES files and the folder of images and meshes) are rewritten so they still point at
//! the same files.

use crate::environment::EnvironmentConfig;
use crate::scene::{find_data_folder, MaterialSource, Scene};
use serde_json::{json, Map, Value};
use std::path::{Component, Path, PathBuf};

/// Changes to the scene file's objects and lights, by index: each one's fields replace those
/// of the entry, and a null removes the field, as when a light goes back from a temperature to
/// a plain color. Fields that say what an entry is or where its data lives (`type`, `file`,
/// `material`, `ies`, `name`) are left alone.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct EntryEdits {
    #[serde(default)]
    pub objects: Vec<(usize, Map<String, Value>)>,
    #[serde(default)]
    pub lights: Vec<(usize, Map<String, Value>)>,
    /// Whole entries added after the file's own, in order: an added object's index is the
    /// file's count of objects plus its place here, so no index already in use changes.
    #[serde(default)]
    pub added_objects: Vec<Value>,
    #[serde(default)]
    pub added_lights: Vec<Value>,
    /// Entries deleted, by index, the added ones included. While editing they are hidden, as
    /// the outliner hides them, so no index changes; saving takes them out of the file.
    #[serde(default)]
    pub removed_objects: Vec<usize>,
    #[serde(default)]
    pub removed_lights: Vec<usize>,
    /// The scene's fog, as the file writes it, in place of the file's; `no_fog` takes it away.
    #[serde(default)]
    pub fog: Option<Map<String, Value>>,
    #[serde(default)]
    pub no_fog: bool,
}

/// Fields an edit cannot change.
const FIXED_FIELDS: [&str; 5] = ["type", "file", "material", "ies", "name"];

impl EntryEdits {
    /// Whether the scene reads the same with these edits: deletions alone change nothing until
    /// saved, as they are hidden.
    pub fn is_empty(&self) -> bool {
        self.objects.iter().all(|(_, f)| f.is_empty()) && self.lights.iter().all(|(_, f)| f.is_empty())
            && self.added_objects.is_empty() && self.added_lights.is_empty() && self.fog.is_none() && !self.no_fog
    }

    /// Whether saving changes which objects and lights the file has, so their indices.
    pub fn changes_entries(&self) -> bool {
        !(self.added_objects.is_empty() && self.added_lights.is_empty() && self.removed_objects.is_empty() && self.removed_lights.is_empty())
    }

    /// Writes the edits into a scene file's root object: the added entries after the file's,
    /// then each edit's fields. Deleted entries stay, so indices hold; see `remove_deleted`.
    pub fn apply(&self, root: &mut Map<String, Value>) {
        if self.no_fog {
            root.remove("fog");
        } else if let Some(fog) = &self.fog {
            root.insert("fog".into(), Value::Object(fog.clone()));
        }
        for (list, added) in [("objects", &self.added_objects), ("lights", &self.added_lights)] {
            if added.is_empty() {
                continue;
            }
            let entries = root.entry(list).or_insert_with(|| Value::Array(Vec::new()));
            if let Some(entries) = entries.as_array_mut() {
                entries.extend(added.iter().filter(|e| e.is_object()).cloned());
            }
        }
        for (list, edits) in [("objects", &self.objects), ("lights", &self.lights)] {
            let Some(entries) = root.get_mut(list).and_then(Value::as_array_mut) else { continue };
            for (index, fields) in edits {
                let Some(entry) = entries.get_mut(*index).and_then(Value::as_object_mut) else { continue };
                for (key, value) in fields {
                    if FIXED_FIELDS.contains(&key.as_str()) {
                        continue;
                    }
                    if value.is_null() {
                        entry.remove(key);
                    } else {
                        entry.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }

    /// Takes the deleted entries out of a scene file's root object, after `apply`.
    pub fn remove_deleted(&self, root: &mut Map<String, Value>) {
        for (list, removed) in [("objects", &self.removed_objects), ("lights", &self.removed_lights)] {
            let Some(entries) = root.get_mut(list).and_then(Value::as_array_mut) else { continue };
            let mut index = 0;
            entries.retain(|_| {
                let keep = !removed.contains(&index);
                index += 1;
                keep
            });
        }
    }
}

/// What the app changed.
#[derive(Default)]
pub struct SceneEdits {
    pub camera: crate::camera::Camera,
    /// None removes the environment.
    pub environment: Option<EnvironmentConfig>,
    /// Material id and its edit: a new surface, bump strength or glow; what it leaves None
    /// keeps the file's.
    pub materials: Vec<(usize, crate::surface::MaterialEdit)>,
    /// The app's render settings, stored as they are.
    pub render: Option<Value>,
    /// Moved, turned, resized or retuned objects and lights.
    pub entries: EntryEdits,
}

/// Writes `scene`, read from `original`, with `edits` to `target`. Returns the edits that
/// could not be saved.
pub fn save(scene: &Scene, original: &Path, target: &Path, edits: &SceneEdits) -> Result<Vec<String>, String> {
    let mut file = scene.source.as_deref().cloned()
        .ok_or("This scene was not read from a .far file, so it cannot be saved as one")?;
    let root = file.as_object_mut().ok_or("The scene file is not a JSON object")?;
    let from = original.parent().unwrap_or(Path::new(".")).to_path_buf();
    let to = absolute(target.parent().unwrap_or(Path::new(".")));
    let mut warnings = Vec::new();
    // Added objects have materials of their own, numbered as in the scene read with them;
    // those of the file's objects keep their numbers there.
    let with_added = (!edits.entries.added_objects.is_empty()).then(|| scene.edited(original, &edits.entries).ok()).flatten();
    let scene = with_added.as_ref().unwrap_or(scene);
    edits.entries.apply(root);

    if from != to {
        relocate_paths(root, &from, &to, original, target);
    }
    // A model added in the app names its file by its full path; the scene keeps it relative.
    for object in root.get_mut("objects").and_then(Value::as_array_mut).into_iter().flatten() {
        if object.get("type").and_then(Value::as_str) != Some("model") {
            continue;
        }
        if let Some(file) = object.get_mut("file") {
            if let Some(path) = file.as_str().map(PathBuf::from).filter(|p| p.is_absolute() && p.exists()) {
                *file = json!(relative_to(&path, &to));
            }
        }
    }
    root.insert("format".into(), json!(crate::scene_file::FORMAT));
    root.insert("camera".into(), serde_json::to_value(edits.camera).map_err(|e| e.to_string())?);
    match &edits.environment {
        None => { root.remove("environment"); }
        Some(config) => { root.insert("environment".into(), environment_value(config, &to)); }
    }
    for (id, edit) in &edits.materials {
        let (surface, bump) = (&edit.surface, &edit.bump_strength);
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
                // A glow keeps the file's map; a strength of 0 removes it.
                if let Some(glow) = edit.emission {
                    if glow.strength > 0.0 {
                        let entry = material.entry("emission").or_insert_with(|| json!({}));
                        if !entry.is_object() {
                            *entry = json!({});
                        }
                        entry["color"] = json!(glow.color);
                        entry["strength"] = json!(glow.strength);
                    } else {
                        material.remove("emission");
                    }
                }
            }
            // Kept in the scene by the part's name, as `parts`; the model file stays as it is.
            MaterialSource::ModelPart(index) => {
                let part = scene.objects.get(index)
                    .and_then(|o| id.checked_sub(o.materials.start).and_then(|k| o.parts.get(k)));
                let object = root.get_mut("objects").and_then(|o| o.get_mut(index)).and_then(Value::as_object_mut);
                let (Some(part), Some(object)) = (part, object) else {
                    warnings.push(format!("Edits to material {id} are not saved"));
                    continue;
                };
                let parts = object.entry("parts").or_insert_with(|| json!({}));
                if !parts.is_object() {
                    *parts = json!({});
                }
                let Some(entry) = parts.as_object_mut().map(|p| p.entry(part.clone()).or_insert_with(|| json!({}))) else { continue };
                if !entry.is_object() {
                    *entry = json!({});
                }
                if let Some(surface) = surface {
                    entry["surface"] = serde_json::to_value(surface).map_err(|e| e.to_string())?;
                }
                if let Some(strength) = bump {
                    entry["bump_strength"] = json!(strength);
                }
                if let Some(glow) = edit.emission {
                    entry["emission"] = json!({"color": glow.color, "strength": glow.strength});
                }
            }
            MaterialSource::None => warnings.push(format!("Edits to material {id} are not saved")),
        }
    }
    // Last, once material edits have found their objects by index.
    edits.entries.remove_deleted(root);
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
        // Windows resolves folders to "\\?\C:\..."; the file keeps the plain form people read.
        let text = path.to_string_lossy().into_owned();
        return text.strip_prefix(r"\\?\").unwrap_or(&text).replace('\\', "/");
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
    #[cfg(unix)]
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
        let surface: crate::surface::Surface = serde_json::from_str(r#"{"preset": "metal", "roughness": 0.3}"#).unwrap();
        let first = scene.material_sources.iter().position(|s| *s == MaterialSource::Object(0)).unwrap();
        let second = scene.material_sources.iter().position(|s| *s == MaterialSource::Object(1)).unwrap();
        let edits = SceneEdits {
            camera: crate::camera::Camera { zoom: 2.0, ..Default::default() },
            environment: Some(EnvironmentConfig { sky: Some(Default::default()), ..Default::default() }),
            materials: vec![
                (first, crate::surface::MaterialEdit { bump_strength: Some(0.5), ..Default::default() }),
                (second, crate::surface::MaterialEdit {
                    surface: Some(surface),
                    emission: Some(crate::surface::EmissionEdit { color: [1.0, 0.5, 0.0], strength: 3.0 }),
                    ..Default::default()
                }),
            ],
            render: Some(json!({"samples": 256})),
            // The second sphere moved, the light grew brighter; `type` cannot change.
            entries: EntryEdits {
                objects: vec![(1, json!({"center": [3, 1, 5], "type": "box"}).as_object().unwrap().clone())],
                lights: vec![(0, json!({"intensity": 4}).as_object().unwrap().clone()), (7, Map::new())],
                // A box added, and the first sphere deleted after its material was edited.
                added_objects: vec![json!({"type": "box", "name": "Crate", "center": [0, 0, 8], "size": [1, 1, 1]})],
                removed_objects: vec![0],
                ..Default::default()
            },
        };
        // Saved elsewhere: the IES path and the data folder are rewritten.
        let elsewhere = folder("edits-out");
        let target = elsewhere.join("b.far");
        let warnings = save(&scene, &original, &target, &edits).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(saved["bounds"][1][0], 9);
        assert_eq!(saved["camera"]["zoom"], 2.0);
        assert_eq!(saved["camera"]["sensor_width"], 36.0);
        assert!((saved["camera"]["scene_unit"].as_f64().unwrap() - 0.1).abs() < 1e-6);
        assert_eq!(saved["format"], crate::scene_file::FORMAT);
        assert!(saved["environment"]["sky"].is_object());
        // The first sphere is gone, so the second is first and the crate after it.
        assert_eq!(saved["objects"].as_array().unwrap().len(), 2);
        assert_eq!(saved["objects"][0]["material"]["surface"]["preset"], "metal");
        assert_eq!(saved["objects"][0]["material"]["emission"], json!({"color": [1.0, 0.5, 0.0], "strength": 3.0}));
        assert_eq!(saved["objects"][0]["center"], json!([3, 1, 5]));
        assert_eq!(saved["objects"][0]["type"], "sphere");
        assert_eq!(saved["objects"][1]["name"], "Crate");
        assert_eq!(saved["lights"][0]["intensity"], 4);
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

    /// A folder with a model of two materials, Red and Glass, and a triangle without one.
    fn model_folder(name: &str) -> PathBuf {
        let dir = folder(name);
        std::fs::write(dir.join("parts.mtl"), "newmtl Red\nKd 1 0 0\nnewmtl Glass\nKd 1 1 1\n").unwrap();
        std::fs::write(dir.join("parts.obj"), "mtllib parts.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nf 1 3 4\n\
            usemtl Red\nf 1 2 3\nusemtl Glass\nf 1 2 4\n").unwrap();
        dir
    }

    fn metal() -> crate::surface::MaterialEdit {
        crate::surface::MaterialEdit { surface: serde_json::from_str(r#"{"preset": "metal", "roughness": 0.3}"#).unwrap(), ..Default::default() }
    }

    #[test]
    fn edits_to_a_models_parts_are_saved_by_part_name_and_read_back() {
        let dir = model_folder("parts");
        let original = dir.join("a.far");
        std::fs::write(&original, r#"{"lights": [], "objects": [{"type": "sphere", "center": [0, 0, 5], "radius": 1},
            {"type": "model", "file": "parts.obj", "position": [0, 0, 3]}]}"#).unwrap();
        let scene = Scene::open(&original).unwrap();
        let model = &scene.objects[1];
        assert_eq!(model.parts, ["Red", "Glass", "default"]);
        assert_eq!(model.materials.len(), 3);
        let glass = model.materials.start + 1;
        assert_eq!(scene.material_sources[glass], MaterialSource::ModelPart(1));
        let edits = SceneEdits { materials: vec![(glass, metal())], ..Default::default() };
        let warnings = save(&scene, &original, &original, &edits).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&original).unwrap()).unwrap();
        assert_eq!(saved["objects"][1]["parts"]["Glass"]["surface"]["preset"], "metal");
        assert!(saved["objects"][1]["parts"].get("Red").is_none());
        let reopened = Scene::open(&original).unwrap();
        let read = |id: usize| reopened.materials[id].surface.as_ref().map(|s| s.metallic);
        assert_eq!(read(glass), Some(1.0), "the part reads as metal");
        assert_eq!(read(glass - 1), Some(0.0), "the other part is as the file has it");
        assert!(reopened.warnings.is_empty(), "{:?}", reopened.warnings);
        // A name the model does not have is reported, not an error.
        std::fs::write(&original, r#"{"lights": [], "objects": [{"type": "model", "file": "parts.obj", "parts": {"Gold": {"bump_strength": 1}}}]}"#).unwrap();
        assert!(Scene::open(&original).unwrap().warnings.iter().any(|w| w.contains("no part named Gold")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn objects_added_in_the_app_leave_the_others_materials_where_they_were() {
        let dir = model_folder("added");
        let original = dir.join("a.far");
        std::fs::write(&original, r#"{"lights": [], "objects": [{"type": "sphere", "center": [0, 0, 5], "radius": 1, "material": {"color": [1, 0, 0]}},
            {"type": "box", "center": [2, 0, 5], "size": [1, 1, 1], "material": {"color": [0, 1, 0]}}]}"#).unwrap();
        let scene = Scene::open(&original).unwrap();
        // A plane and a model, which are read before spheres and boxes in a file.
        let entries = EntryEdits {
            added_objects: vec![
                json!({"type": "plane", "points": [[-1, 0, -1], [1, 0, -1], [1, 0, 1], [-1, 0, 1]], "material": {"color": [0, 0, 1]}}),
                json!({"type": "model", "file": dir.join("parts.obj").to_string_lossy(), "size": 1}),
            ],
            ..Default::default()
        };
        let edited = scene.edited(&original, &entries).unwrap();
        for object in 0..2 {
            let id = scene.objects[object].materials.start;
            assert_eq!(edited.objects[object].materials.start, id);
            assert_eq!(edited.materials[id].color, scene.materials[id].color);
        }
        // The added model's parts are edited and saved with it, its path made relative.
        let model = &edited.objects[3];
        assert_eq!(model.parts, ["Red", "Glass", "default"]);
        let red = model.materials.start;
        let edits = SceneEdits { materials: vec![(red, metal())], entries, ..Default::default() };
        let warnings = save(&scene, &original, &original, &edits).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&original).unwrap()).unwrap();
        assert_eq!(saved["objects"][3]["file"], "parts.obj");
        assert_eq!(saved["objects"][3]["parts"]["Red"]["surface"]["preset"], "metal");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

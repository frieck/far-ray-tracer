//! Opt-in modern material presets. Legacy materials remain untouched by default.
use serde::{Deserialize, Serialize};
use crate::{ggx::Ggx, scene::Material, vector::Vec3};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Preset { Metal, Plastic, Ceramic, Glass, Custom }

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(try_from = "Input")]
pub struct Surface {
    pub preset: Preset,
    pub roughness: f32,
    pub metallic: f32,
    pub ior: f32,
    pub color: Option<[f32; 3]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    preset: Preset,
    roughness: Option<f32>,
    metallic: Option<f32>,
    ior: Option<f32>,
    color: Option<[f32; 3]>,
}
impl TryFrom<Input> for Surface {
    type Error = String;
    fn try_from(v: Input) -> Result<Self, String> {
        let (roughness, metallic) = match v.preset {
            Preset::Metal => (0.2, 1.0), Preset::Plastic => (0.3, 0.0),
            Preset::Ceramic => (0.15, 0.0), Preset::Glass => (0.0, 0.0), Preset::Custom => (0.4, 0.0),
        };
        let s = Self { preset: v.preset, roughness: v.roughness.unwrap_or(roughness),
            metallic: v.metallic.unwrap_or(metallic), ior: v.ior.unwrap_or(1.5), color: v.color };
        if !(0.0..=1.0).contains(&s.roughness) || !(0.0..=1.0).contains(&s.metallic)
            || !s.ior.is_finite() || !(1.0..=3.0).contains(&s.ior)
            || s.color.is_some_and(|c| c.iter().any(|v| !(0.0..=1.0).contains(v))) {
            return Err("Material needs roughness/metallic/color in [0,1] and IOR in [1,3]".into());
        }
        if s.preset == Preset::Glass && (s.roughness != 0.0 || s.metallic != 0.0) {
            return Err("Glass currently supports clear transmission only (roughness and metallic must be zero)".into());
        }
        Ok(s)
    }
}
impl Surface {
    pub fn apply(&self, m: &mut Material) {
        if let Some([r,g,b]) = self.color { m.color = Vec3::new(r,g,b); }
        m.ior = self.ior;
        m.drefl = 0.0;
        m.spec = 0.0;
        if self.preset == Preset::Glass {
            m.ggx = None; m.diff = 0.0; m.refl = 0.0; m.refr = 1.0;
        } else {
            m.ggx = Some(Ggx { roughness: self.roughness, metallic: self.metallic });
            // Classic and Fixed have no GGX and read these two values directly, so they
            // hold the head-on reflectance. Enhanced computes its own response. A small
            // floor keeps the object on the reflective path even at IOR 1.
            let dielectric = ((self.ior - 1.0) / (self.ior + 1.0)).powi(2);
            m.refl = (self.metallic + (1.0 - self.metallic) * dielectric).max(1e-3);
            m.diff = (1.0 - self.metallic) * (1.0 - dielectric);
            m.refr = 0.0;
        }
        m.surface = Some(self.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_and_roundtrip() {
        let s: Surface = serde_json::from_str(r#"{"preset":"metal"}"#).unwrap();
        assert_eq!(s.metallic, 1.);
        assert_eq!(s.roughness, 0.2);
        let copy: Surface = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(copy.preset, Preset::Metal);
    }
    #[test]
    fn legacy_values_follow_reflectance_instead_of_a_full_mirror() {
        let material = |preset: &str| {
            let text = format!(r#"{{"lights":[],"objects":[{{"type":"sphere","center":[0,0,5],"radius":1,"material":{{"surface":{{"preset":"{preset}"}}}}}}]}}"#);
            let scene = crate::Scene::from_far(text.as_bytes(), std::path::Path::new("test.far")).unwrap();
            (scene.materials[0].refl, scene.materials[0].diff)
        };
        let (refl, diff) = material("plastic");
        assert!((refl - 0.04).abs() < 1e-6 && (diff - 0.96).abs() < 1e-6);
        assert_eq!(material("metal"), (1.0, 0.0));
    }
    #[test]
    fn invalid_parameters_rejected() {
        for json in [
            r#"{"preset":"metal","roughness":-0.1}"#,
            r#"{"preset":"plastic","metallic":2}"#,
            r#"{"preset":"custom","ior":0}"#,
            r#"{"preset":"custom","color":[1,2,0]}"#,
            r#"{"preset":"glass","roughness":0.2}"#,
            r#"{"preset":"metal","rougness":0.2}"#,
        ] { assert!(serde_json::from_str::<Surface>(json).is_err(), "{json}"); }
    }
}

/// An edit from the app's material editor: a new surface, a new bump strength, or both.
/// Changing only the bump keeps a 2008 material as it is.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MaterialEdit {
    pub surface: Option<Surface>,
    pub bump_strength: Option<f32>,
}

impl MaterialEdit {
    pub fn apply(&self, m: &mut crate::scene::Material) -> Result<(), String> {
        if let Some(surface) = &self.surface {
            surface.apply(m);
        }
        if let Some(strength) = self.bump_strength {
            if !strength.is_finite() || strength < 0.0 {
                return Err("The bump strength must be zero or more".into());
            }
            if let Some(bump) = m.bump.as_mut() {
                bump.strength = strength;
            }
        }
        Ok(())
    }
}

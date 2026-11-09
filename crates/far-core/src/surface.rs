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
    /// Glass: distance through which light keeps `color`; deeper glass is darker. None keeps
    /// the light tint of plain glass.
    pub absorption_depth: Option<f32>,
    /// Glass: Abbe number. Lower splits light into wider rainbows; None disperses nothing.
    pub abbe: Option<f32>,
    pub clearcoat: f32,
    pub clearcoat_roughness: f32,
    pub sheen: f32,
    pub sheen_color: [f32; 3],
    pub anisotropy: f32,
    /// Degrees the anisotropy direction turns around the normal.
    pub anisotropy_rotation: f32,
    /// Share of the diffuse light that travels under the surface, from 0 to 1.
    pub subsurface: f32,
    /// Mean distance light travels under the surface between scatterings, in scene units.
    pub subsurface_radius: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    preset: Preset,
    roughness: Option<f32>,
    metallic: Option<f32>,
    ior: Option<f32>,
    color: Option<[f32; 3]>,
    absorption_depth: Option<f32>,
    abbe: Option<f32>,
    clearcoat: Option<f32>,
    clearcoat_roughness: Option<f32>,
    sheen: Option<f32>,
    sheen_color: Option<[f32; 3]>,
    anisotropy: Option<f32>,
    anisotropy_rotation: Option<f32>,
    subsurface: Option<f32>,
    subsurface_radius: Option<f32>,
}

fn unit(v: f32) -> bool {
    (0.0..=1.0).contains(&v)
}

impl TryFrom<Input> for Surface {
    type Error = String;
    fn try_from(v: Input) -> Result<Self, String> {
        let (roughness, metallic) = match v.preset {
            Preset::Metal => (0.2, 1.0), Preset::Plastic => (0.3, 0.0),
            Preset::Ceramic => (0.15, 0.0), Preset::Glass => (0.0, 0.0), Preset::Custom => (0.4, 0.0),
        };
        let s = Self {
            preset: v.preset,
            roughness: v.roughness.unwrap_or(roughness),
            metallic: v.metallic.unwrap_or(metallic),
            ior: v.ior.unwrap_or(1.5),
            color: v.color,
            absorption_depth: v.absorption_depth,
            abbe: v.abbe,
            clearcoat: v.clearcoat.unwrap_or(0.0),
            clearcoat_roughness: v.clearcoat_roughness.unwrap_or(0.03),
            sheen: v.sheen.unwrap_or(0.0),
            sheen_color: v.sheen_color.unwrap_or([1.0; 3]),
            anisotropy: v.anisotropy.unwrap_or(0.0),
            anisotropy_rotation: v.anisotropy_rotation.unwrap_or(0.0),
            subsurface: v.subsurface.unwrap_or(0.0),
            subsurface_radius: v.subsurface_radius.unwrap_or(0.1),
        };
        if !unit(s.roughness) || !unit(s.metallic) || !s.ior.is_finite() || !(1.0..=3.0).contains(&s.ior)
            || s.color.is_some_and(|c| c.iter().any(|v| !unit(*v))) {
            return Err("Material needs roughness/metallic/color in [0,1] and IOR in [1,3]".into());
        }
        if !unit(s.clearcoat) || !unit(s.clearcoat_roughness) || !unit(s.sheen) || !unit(s.subsurface)
            || s.sheen_color.iter().any(|v| !unit(*v)) || !(-1.0..=1.0).contains(&s.anisotropy)
            || !s.anisotropy_rotation.is_finite() || !(s.subsurface_radius > 0.0 && s.subsurface_radius.is_finite()) {
            return Err("Material needs clearcoat, sheen and subsurface in [0,1], anisotropy in [-1,1] and a positive subsurface radius".into());
        }
        if s.absorption_depth.is_some_and(|d| !(d > 0.0 && d.is_finite())) || s.abbe.is_some_and(|a| !(5.0..=100.0).contains(&a)) {
            return Err("Glass needs a positive absorption depth and an Abbe number from 5 to 100".into());
        }
        if s.preset == Preset::Glass {
            if s.metallic != 0.0 || s.clearcoat != 0.0 || s.sheen != 0.0 || s.anisotropy != 0.0 || s.subsurface != 0.0 {
                return Err("Glass takes roughness, IOR, color, absorption depth and Abbe number only".into());
            }
        } else if s.absorption_depth.is_some() || s.abbe.is_some() {
            return Err("Absorption depth and Abbe number apply to glass only".into());
        }
        Ok(s)
    }
}

/// Glass beyond the original's clear one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
    pub roughness: f32,
    pub absorption_depth: Option<f32>,
    pub abbe: Option<f32>,
}

/// Light that travels under the surface before it leaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Subsurface {
    pub amount: f32,
    pub radius: f32,
}

impl Surface {
    pub fn apply(&self, m: &mut Material) {
        if let Some([r,g,b]) = self.color { m.color = Vec3::new(r,g,b); }
        m.ior = self.ior;
        m.drefl = 0.0;
        m.spec = 0.0;
        m.glass = None;
        m.subsurface = None;
        if self.preset == Preset::Glass {
            m.ggx = None; m.diff = 0.0; m.refl = 0.0; m.refr = 1.0;
            m.glass = Some(Glass { roughness: self.roughness, absorption_depth: self.absorption_depth, abbe: self.abbe });
        } else {
            let [r, g, b] = self.sheen_color;
            m.ggx = Some(Ggx {
                anisotropy: self.anisotropy,
                rotation: self.anisotropy_rotation.to_radians(),
                clearcoat: self.clearcoat,
                clearcoat_roughness: self.clearcoat_roughness,
                sheen: self.sheen,
                sheen_color: Vec3::new(r, g, b),
                ..Ggx::new(self.roughness, self.metallic)
            });
            if self.subsurface > 0.0 && self.metallic < 1.0 {
                m.subsurface = Some(Subsurface { amount: self.subsurface, radius: self.subsurface_radius });
            }
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
            r#"{"preset":"glass","metallic":0.2}"#,
            r#"{"preset":"glass","clearcoat":0.5}"#,
            r#"{"preset":"plastic","abbe":30}"#,
            r#"{"preset":"glass","abbe":2}"#,
            r#"{"preset":"glass","absorption_depth":0}"#,
            r#"{"preset":"metal","anisotropy":1.5}"#,
            r#"{"preset":"plastic","subsurface_radius":0}"#,
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

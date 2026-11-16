//! A render's linear light and its passes, before tone mapping, saved as one OpenEXR file that
//! compositors and image editors read: exposure, colors and curves can then change without
//! rendering again.

use exr::prelude::*;

/// Per pixel of the whole image, row by row; pixels a region left out hold zeros.
pub struct Layers {
    pub width: u32,
    pub height: u32,
    /// The mean exposed light of each pixel, as the display tone-maps it.
    pub light: Vec<[f32; 3]>,
    /// The same light through Open Image Denoise, when the render denoised and it is installed.
    pub denoised: Option<Vec<[f32; 3]>>,
    /// The color of what camera rays met first, their normals in world space and the distance
    /// along the ray, when the render denoised, which records them.
    pub albedo: Option<Vec<[f32; 3]>>,
    pub normal: Option<Vec<[f32; 3]>>,
    pub depth: Option<Vec<f32>>,
}

impl Layers {
    /// Writes the layers as 32-bit float channels, compressed without loss: the light as R, G
    /// and B, the noisy light as `noisy.R` (when denoised), then `albedo.R`, `normal.X` and
    /// `depth.Z`, the names Blender and Nuke use.
    pub fn write_exr(&self, path: &std::path::Path) -> std::result::Result<(), String> {
        let pixels = (self.width * self.height) as usize;
        if self.light.len() != pixels {
            return Err("the layers do not fit the image size".into());
        }
        let mut channels: SmallVec<[AnyChannel<FlatSamples>; 4]> = SmallVec::new();
        let mut triple = |prefix: &str, names: [&str; 3], values: &[[f32; 3]]| {
            for (k, name) in names.iter().enumerate() {
                channels.push(AnyChannel::new(format!("{prefix}{name}").as_str(), FlatSamples::F32(values.iter().map(|v| v[k]).collect())));
            }
        };
        match &self.denoised {
            Some(denoised) => {
                triple("", ["R", "G", "B"], denoised);
                triple("noisy.", ["R", "G", "B"], &self.light);
            }
            None => triple("", ["R", "G", "B"], &self.light),
        }
        if let Some(albedo) = &self.albedo {
            triple("albedo.", ["R", "G", "B"], albedo);
        }
        if let Some(normal) = &self.normal {
            triple("normal.", ["X", "Y", "Z"], normal);
        }
        if let Some(depth) = &self.depth {
            channels.push(AnyChannel::new("depth.Z", FlatSamples::F32(depth.clone())));
        }
        let layer = Layer::new(
            (self.width as usize, self.height as usize),
            LayerAttributes { software_name: Some(Text::from("Far Ray Tracer")), ..LayerAttributes::default() },
            Encoding::SMALL_LOSSLESS,
            AnyChannels::sort(channels),
        );
        Image::from_layer(layer).write().to_file(path).map_err(|e| format!("could not write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_and_passes_read_back_from_the_file() {
        let light = vec![[0.5, 2.0, 40.0], [0.0, 0.25, 1.0]];
        let layers = Layers {
            width: 2, height: 1, light: light.clone(), denoised: None,
            albedo: Some(vec![[0.8, 0.1, 0.1]; 2]), normal: Some(vec![[0.0, 1.0, 0.0]; 2]), depth: Some(vec![3.5, 7.0]),
        };
        let path = std::env::temp_dir().join(format!("far-layers-{}.exr", std::process::id()));
        layers.write_exr(&path).unwrap();
        let image = read_all_flat_layers_from_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let channels = &image.layer_data[0].channel_data.list;
        let channel = |name: &str| channels.iter().find(|c| c.name.to_string() == name).unwrap_or_else(|| panic!("no {name}"))
            .sample_data.values_as_f32().collect::<Vec<_>>();
        assert_eq!(channel("R"), [0.5, 0.0]);
        assert_eq!(channel("B"), [40.0, 1.0]);
        assert_eq!(channel("albedo.R"), [0.8, 0.8]);
        assert_eq!(channel("normal.Y"), [1.0, 1.0]);
        assert_eq!(channel("depth.Z"), [3.5, 7.0]);
        assert_eq!(channels.len(), 10);
    }
}

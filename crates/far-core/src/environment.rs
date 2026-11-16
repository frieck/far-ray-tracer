//! Latitude-longitude HDR environment, sampled by luminance and exact texel solid angle.
use crate::vector::{Color, Vec3, PI};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EnvironmentConfig {
    /// A panorama (HDR, EXR, PNG or JPEG); empty when `sky` computes the environment instead.
    pub path: String,
    /// A physical sky and sun in place of an image.
    pub sky: Option<crate::sky::SkyConfig>,
    pub intensity: f32,
    /// Rotation around the vertical (+Y) axis, in degrees.
    pub rotation: f32,
    pub visible: bool,
    pub enabled: bool,
}
impl Default for EnvironmentConfig {
    fn default() -> Self {
        Self { path: String::new(), sky: None, intensity: 1., rotation: 0., visible: true, enabled: true }
    }
}
impl EnvironmentConfig {
    pub fn validate(&self) -> Result<(), String> {
        match &self.sky {
            Some(sky) => sky.validate()?,
            None if self.path.is_empty() => return Err("Choose an environment image".into()),
            None => {}
        }
        if !self.intensity.is_finite() || !(0.0..=100.0).contains(&self.intensity)
            || !self.rotation.is_finite() {
            return Err("Environment needs intensity in [0,100] and a finite rotation".into());
        }
        Ok(())
    }

    /// What the map depends on: two configs with the same key share one map.
    pub fn map_key(&self) -> String {
        match &self.sky {
            Some(sky) => format!("sky:{}", serde_json::to_string(sky).unwrap_or_default()),
            None => self.path.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Environment {
    pub config: EnvironmentConfig,
    pub map: Arc<EnvironmentMap>,
}
impl Environment {
    pub fn load(config: EnvironmentConfig) -> Result<Self, String> {
        config.validate()?;
        let map = Arc::new(match &config.sky {
            Some(sky) => crate::sky::render(sky)?,
            None => EnvironmentMap::load(Path::new(&config.path))?,
        });
        Ok(Self { config, map })
    }
    pub fn active(&self) -> bool { self.config.enabled && self.config.intensity > 0. }
    fn rotate(&self, d: Vec3, sign: f32) -> Vec3 {
        let (s, c) = (sign * self.config.rotation.rem_euclid(360.).to_radians()).sin_cos();
        Vec3::new(c * d.x + s * d.z, d.y, -s * d.x + c * d.z)
    }
    pub fn radiance(&self, direction: Vec3) -> Color {
        if !self.active() { return Color::default(); }
        self.map.radiance(self.rotate(direction, -1.)) * self.config.intensity
    }
    pub fn pdf(&self, direction: Vec3) -> f32 { self.map.pdf(self.rotate(direction, -1.)) }
    pub fn sample(&self, u: f32, v: f32) -> (Vec3, Color, f32) {
        let (local, pdf) = self.map.sample(u, v);
        let world = self.rotate(local, 1.);
        (world, self.map.radiance(local) * self.config.intensity, pdf)
    }
}

/// An sRGB-encoded value from 0 to 1 as linear light.
fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub struct EnvironmentMap {
    width: usize,
    height: usize,
    pixels: Vec<Color>,
    /// Flattened CDF with a uniform-sphere mixture to support bilinear filtering.
    cdf: Vec<f64>,
    rows: Vec<f64>,
}
impl EnvironmentMap {
    /// Reads a latitude-longitude panorama: Radiance HDR or OpenEXR as linear light, or an
    /// 8- or 16-bit PNG, JPEG, TGA or BMP, whose sRGB values become linear light up to 1.
    pub fn load(path: &Path) -> Result<Self, String> {
        let format = Self::format(path)?;
        let read = || -> Result<image::ImageReader<std::io::BufReader<std::fs::File>>, String> {
            let mut reader = image::ImageReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            reader.set_format(format);
            reader.no_limits();
            Ok(reader)
        };
        let invalid = |e: image::ImageError| format!("Invalid environment {}: {e}", path.display());
        let (w, h) = read()?.into_dimensions().map_err(invalid)?;
        if w == 0 || h == 0 || w as u64 * h as u64 > 16_777_216 {
            return Err("Environment images must contain 1 to 16,777,216 pixels".into());
        }
        let image = read()?.decode().map_err(invalid)?;
        let linear = matches!(format, image::ImageFormat::Hdr | image::ImageFormat::OpenExr);
        let pixels = if linear {
            image.into_rgb32f().pixels().map(|p| Color::new(p[0].max(0.), p[1].max(0.), p[2].max(0.))).collect()
        } else {
            let table: Vec<f32> = (0..=u16::MAX).map(|v| srgb_to_linear(v as f32 / u16::MAX as f32)).collect();
            image.into_rgb16().pixels().map(|p| Color::new(table[p[0] as usize], table[p[1] as usize], table[p[2] as usize])).collect()
        };
        Self::new(w as usize, h as usize, pixels)
    }

    /// The format of an environment file, from its first bytes, or from its name for Radiance
    /// files, whose header varies.
    fn format(path: &Path) -> Result<image::ImageFormat, String> {
        use image::ImageFormat::*;
        let guessed = image::ImageReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?
            .with_guessed_format().map_err(|e| format!("{}: {e}", path.display()))?.format();
        match guessed.or_else(|| image::ImageFormat::from_path(path).ok()) {
            Some(format @ (Hdr | OpenExr | Png | Jpeg | Tga | Bmp)) => Ok(format),
            None => Ok(Hdr),
            Some(other) => Err(format!("{}: {other:?} images cannot be environments; use HDR, EXR, PNG or JPEG",
                path.display())),
        }
    }
    pub(crate) fn new(width: usize, height: usize, pixels: Vec<Color>) -> Result<Self, String> {
        if pixels.iter().any(|p| p.array().iter().any(|v| !v.is_finite() || *v < 0.)) {
            return Err("HDR pixels must be finite, non-negative radiance".into());
        }
        let mut map = Self { width, height, pixels, cdf: Vec::with_capacity(width * height + 1), rows: Vec::new() };
        // Match the footprint of bilinear reconstruction. Sampling only raw texel
        // centers underestimates the bright fringe in neighboring dark texels.
        let luminance = |x: isize, y: isize| {
            let p = map.pixels[y.clamp(0, height as isize - 1) as usize * width
                + x.rem_euclid(width as isize) as usize];
            0.2126 * p.x as f64 + 0.7152 * p.y as f64 + 0.0722 * p.z as f64
        };
        let weights = [0.125, 0.75, 0.125];
        let filtered: Vec<f64> = (0..width * height).map(|i| {
            let (x, y) = ((i % width) as isize, (i / width) as isize);
            let mut sum = 0.;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    sum += weights[(dx + 1) as usize] * weights[(dy + 1) as usize] * luminance(x + dx, y + dy);
                }
            }
            sum
        }).collect();
        let total: f64 = filtered.iter().enumerate()
            .map(|(i, value)| value * map.solid_angle(i / width)).sum();
        map.cdf.push(0.);
        let mut sum = 0.;
        for (i, luminance) in filtered.iter().enumerate() {
            let omega = map.solid_angle(i / width);
            let uniform = omega / (4. * std::f64::consts::PI);
            sum += if total > 0. { 0.99 * luminance * omega / total + 0.01 * uniform } else { uniform };
            map.cdf.push(sum);
        }
        for x in &mut map.cdf { *x /= sum; }
        map.rows = (0..=height).map(|y| map.cdf[y * width]).collect();
        Ok(map)
    }
    fn solid_angle(&self, row: usize) -> f64 {
        let pi = std::f64::consts::PI;
        2. * pi / self.width as f64 *
            ((pi * row as f64 / self.height as f64).cos() - (pi * (row + 1) as f64 / self.height as f64).cos())
    }
    fn uv(direction: Vec3) -> (f32, f32) {
        // The panorama's center faces +Z, and +X is to its right, as seen from inside.
        ((direction.x.atan2(direction.z) / (2. * PI) + 0.5).rem_euclid(1.),
         direction.y.clamp(-1., 1.).acos() / PI)
    }
    /// The map's width, height and pixels, row by row from the top, for a GPU texture.
    pub fn pixels(&self) -> (usize, usize, &[Color]) {
        (self.width, self.height, &self.pixels)
    }

    /// The map shrunk to `width` by `width / 2` texels for a preview, row by row from the top,
    /// unrotated: each the mean of the texels it covers, so small bright lamps keep their share.
    pub fn thumbnail(&self, width: usize) -> Vec<Color> {
        let width = width.max(2);
        let height = width / 2;
        let span = |i: usize, n: usize, of: usize| {
            let start = i * of / n;
            start..((i + 1) * of / n).max(start + 1).min(of)
        };
        let mut thumbnail = Vec::with_capacity(width * height);
        for ty in 0..height {
            let rows = span(ty, height, self.height);
            for tx in 0..width {
                let columns = span(tx, width, self.width);
                let mut sum = Color::default();
                for y in rows.clone() {
                    for x in columns.clone() {
                        sum += self.pixels[y * self.width + x];
                    }
                }
                thumbnail.push(sum * (1. / (rows.len() * columns.len()) as f32));
            }
        }
        thumbnail
    }

    /// The sampling tables of `sample` for a GPU, in single precision: the share of light up
    /// to the top of each row (height + 1 values), then for each row the share of that row up
    /// to the left of each texel (width + 1 values per row). Kept per row, they keep their
    /// precision in maps of millions of texels.
    pub fn sampling_tables(&self) -> Vec<f32> {
        let mut tables: Vec<f32> = self.rows.iter().map(|&r| r as f32).collect();
        for y in 0..self.height {
            let (start, mass) = (self.rows[y], (self.rows[y + 1] - self.rows[y]).max(f64::MIN_POSITIVE));
            tables.extend((0..=self.width).map(|x| ((self.cdf[y * self.width + x] - start) / mass) as f32));
        }
        tables
    }

    pub fn radiance(&self, direction: Vec3) -> Color {
        let (u, v) = Self::uv(direction);
        let x = u * self.width as f32 - 0.5;
        let y = v * self.height as f32 - 0.5;
        let (ix, iy) = (x.floor() as isize, y.floor() as isize);
        let (tx, ty) = (x - x.floor(), y - y.floor());
        let pixel = |x: isize, y: isize| self.pixels[
            y.clamp(0, self.height as isize - 1) as usize * self.width
                + x.rem_euclid(self.width as isize) as usize];
        (pixel(ix, iy) * (1. - tx) + pixel(ix + 1, iy) * tx) * (1. - ty)
            + (pixel(ix, iy + 1) * (1. - tx) + pixel(ix + 1, iy + 1) * tx) * ty
    }
    pub fn pdf(&self, direction: Vec3) -> f32 {
        let (u, v) = Self::uv(direction);
        let x = (u * self.width as f32) as usize;
        let y = ((v * self.height as f32) as usize).min(self.height - 1);
        let i = y * self.width + x.min(self.width - 1);
        ((self.cdf[i + 1] - self.cdf[i]) / self.solid_angle(y)) as f32
    }
    pub fn sample(&self, u: f32, v: f32) -> (Vec3, f32) {
        // Use one independent dimension for latitude and one for longitude.
        // Flattening the image into a single CDF wastes the second dimension on
        // sub-texel jitter, preventing stratified samples from covering longitude.
        let unit = |x: f32| (x as f64).clamp(0., 1. - f64::EPSILON);
        let v = unit(v);
        let y = self.rows.partition_point(|&c| c <= v).saturating_sub(1).min(self.height - 1);
        let row_start = self.rows[y];
        let row_mass = self.rows[y + 1] - row_start;
        let latitude = ((v - row_start) / row_mass).clamp(0.000001, 0.999999);
        let u = (row_start + unit(u) * row_mass).min(self.rows[y + 1] - f64::EPSILON * row_mass);
        let row = &self.cdf[y * self.width..=(y + 1) * self.width];
        let x = row.partition_point(|&c| c <= u).saturating_sub(1).min(self.width - 1);
        let i = y * self.width + x;
        let mass = self.cdf[i + 1] - self.cdf[i];
        let longitude = ((u - self.cdf[i]) / mass).clamp(0.000001, 0.999999);
        let pi = std::f64::consts::PI;
        let phi = 2. * pi * (x as f64 + longitude) / self.width as f64;
        let a = (pi * y as f64 / self.height as f64).cos();
        let b = (pi * (y + 1) as f64 / self.height as f64).cos();
        let cos = a + (b - a) * latitude;
        let sin = (1. - cos * cos).max(0.).sqrt();
        let dir = Vec3::new((-sin * phi.sin()) as f32, cos as f32, (-sin * phi.cos()) as f32);
        (dir, (mass / self.solid_angle(y)) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn constant() -> EnvironmentMap {
        EnvironmentMap::new(16, 8, vec![Color::new(2., 2., 2.); 128]).unwrap()
    }
    #[test]
    fn constant_map_is_uniform_in_solid_angle() {
        let map = constant();
        let expected = 1. / (4. * PI);
        let mut average = Vec3::default();
        for i in 0..4096 {
            let (d, pdf) = map.sample((i as f32 + 0.5) / 4096., (i as f32 * 0.618034).fract());
            assert!((d.length() - 1.).abs() < 1e-6);
            assert!((pdf - expected).abs() < 1e-6);
            assert!((map.pdf(d) - pdf).abs() < 1e-6);
            assert!((map.radiance(d) - Color::new(2., 2., 2.)).length() < 1e-5);
            average += d;
        }
        assert!((average * (1. / 4096.)).length() < 0.002);
    }
    #[test]
    fn importance_sampling_targets_bright_pixels_and_keeps_full_support() {
        let mut pixels = vec![Color::default(); 32];
        pixels[10] = Color::new(100., 100., 100.);
        let map = EnvironmentMap::new(8, 4, pixels).unwrap();
        let mut bright = 0;
        let mut integral = 0.;
        for i in 0..32 {
            let mass = map.cdf[i + 1] - map.cdf[i];
            assert!(mass > 0.);
            integral += mass;
        }
        assert!((integral - 1.).abs() < 1e-12);
        for i in 0..10000 {
            let (d, pdf) = map.sample((i as f32 + 0.5) / 10000., 0.37);
            let (u, v) = EnvironmentMap::uv(d);
            if (v * 4.) as usize * 8 + (u * 8.) as usize == 10 { bright += 1; }
            assert!((map.pdf(d) - pdf).abs() < 1e-4);
        }
        assert!(bright > 7000); // Remaining samples cover the filtered bright fringe.
    }
    #[test]
    fn filtered_bright_edges_do_not_create_rare_huge_weights() {
        let mut pixels = vec![Color::default(); 32];
        pixels[10] = Color::new(100., 100., 100.);
        let map = EnvironmentMap::new(8, 4, pixels).unwrap();
        for y in 0..64 {
            for x in 0..128 {
                let theta = PI * (y as f32 + 0.5) / 64.;
                let phi = 2. * PI * (x as f32 + 0.5) / 128.;
                let d = Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin());
                let weight = map.radiance(d).x / map.pdf(d);
                assert!(weight < 4. * PI * 100., "Under-sampled filtered edge: {weight}");
            }
        }
    }

    #[test]
    fn two_dimensional_strata_converge_for_diffuse_irradiance() {
        let map = constant();
        let mut squared_error = 0.;
        let mut independent_error = 0.;
        for seed in 0..32 {
            let mut rng = crate::twister::Twister::new(seed);
            let mut independent = crate::twister::Twister::new(seed ^ 0x9e3779b9);
            let mut estimate = 0.;
            let mut independent_estimate = 0.;
            for y in 0..8 {
                for x in 0..8 {
                    let (direction, pdf) = map.sample((x as f32 + rng.rand()) / 8.,
                        (y as f32 + rng.rand()) / 8.);
                    estimate += direction.x.max(0.) / (PI * pdf * 64.);
                    let (direction, pdf) = map.sample(independent.rand(), independent.rand());
                    independent_estimate += direction.x.max(0.) / (PI * pdf * 64.);
                }
            }
            squared_error += (estimate - 1.).powi(2) / 32.;
            independent_error += (independent_estimate - 1.).powi(2) / 32.;
        }
        assert!(squared_error < independent_error * 0.35,
            "Stratified MSE {squared_error}, independent MSE {independent_error}");
    }

    #[test]
    fn panorama_is_not_mirrored() {
        let u = |x, z| EnvironmentMap::uv(Vec3::new(x, 0., z)).0;
        assert!((u(0., 1.) - 0.5).abs() < 1e-6, "center faces the camera");
        assert!((u(1., 0.) - 0.75).abs() < 1e-6, "image right is view right");
        assert!((u(-1., 0.) - 0.25).abs() < 1e-6, "image left is view left");
        let map = constant();
        for (a, b) in [(0.1, 0.3), (0.6, 0.5), (0.9, 0.8)] {
            let (d, _) = map.sample(a, b);
            let (u, v) = EnvironmentMap::uv(d);
            let (x, y) = ((u * 16.) as usize, (v * 8.) as usize);
            assert_eq!(y * 16 + x, map.cdf.partition_point(|&c| c <= map.rows[y] + a as f64 * (map.rows[y + 1] - map.rows[y])) - 1);
        }
    }

    #[test]
    fn rotation_moves_lighting_and_pdf_together() {
        let mut pixels = vec![Color::default(); 32];
        pixels[10] = Color::new(10., 3., 1.);
        let environment = Environment {
            map: Arc::new(EnvironmentMap::new(8, 4, pixels).unwrap()),
            config: EnvironmentConfig { rotation: 90., intensity: 2., ..EnvironmentConfig::default() },
        };
        let (d, radiance, pdf) = environment.sample(0.5, 0.4);
        assert!((environment.radiance(d) - radiance).length() < 1e-4);
        assert!((environment.pdf(d) - pdf).abs() < 1e-4);
        let original = environment.map.sample(0.5, 0.4).0;
        assert!((d - Vec3::new(original.z, original.y, -original.x)).length() < 1e-6);
    }
    #[test]
    fn black_maps_and_boundary_samples_are_finite() {
        let map = EnvironmentMap::new(8, 4, vec![Color::default(); 32]).unwrap();
        for u in [0., 0.5, 1.] {
            for v in [0., 0.5, 1.] {
                let (d, pdf) = map.sample(u, v);
                assert!(pdf.is_finite() && pdf > 0.);
                assert!(map.pdf(d).is_finite());
                assert_eq!(map.radiance(d), Color::default());
            }
        }
        assert!(EnvironmentMap::new(1, 1, vec![Color::new(f32::INFINITY, 0., 0.)]).is_err());
    }
    #[test]
    fn hdr_loader_preserves_radiance_above_one() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/environments/studio.hdr");
        let map = EnvironmentMap::load(&path).unwrap();
        assert!(map.pixels.iter().any(|p| p.x > 8.));
        assert!(EnvironmentMap::load(&path.with_extension("missing")).is_err());
        let bad = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes/techniques/hdri-studio.far");
        assert!(EnvironmentMap::load(&bad).is_err());
    }
    #[test]
    fn exr_panoramas_load_as_the_same_light_and_png_ones_as_linear_light() {
        let hdr = EnvironmentMap::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/environments/studio.hdr")).unwrap();
        let folder = std::env::temp_dir().join(format!("far-environment-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        // The same panorama as EXR, under a name that hides its format.
        let exr = folder.join("studio.panorama");
        exr::prelude::write_rgb_file(&exr, hdr.width, hdr.height, |x, y| {
            let p = hdr.pixels[y * hdr.width + x];
            (p.x, p.y, p.z)
        }).unwrap();
        let png = folder.join("gray.png");
        image::RgbImage::from_raw(2, 1, vec![255, 128, 0, 0, 0, 0]).unwrap().save(&png).unwrap();
        let gif = folder.join("gray.gif");
        image::RgbaImage::new(2, 1).save(&gif).unwrap();
        let (from_exr, from_png, from_gif) = (EnvironmentMap::load(&exr), EnvironmentMap::load(&png), EnvironmentMap::load(&gif));
        let _ = std::fs::remove_dir_all(&folder);
        let from_exr = from_exr.unwrap();
        assert_eq!((from_exr.width, from_exr.height), (hdr.width, hdr.height));
        assert!(from_exr.pixels == hdr.pixels, "EXR keeps every value of the HDR");
        let from_png = from_png.unwrap();
        assert_eq!(from_png.pixels[0].x, 1.0);
        assert!((from_png.pixels[0].y - 0.2158).abs() < 1e-3, "sRGB 128 is 21.6% of the light");
        assert_eq!(from_png.pixels[0].z, 0.0);
        assert!(from_gif.is_err_and(|e| e.contains("cannot be environments")));
    }
    #[test]
    fn the_bundled_studio_maps_hold_the_hdr_light() {
        // Rewritten from studio.hdr by `cargo run -p far-core --example studio_environments`.
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/environments");
        let hdr = EnvironmentMap::load(&folder.join("studio.hdr")).unwrap();
        assert!(EnvironmentMap::load(&folder.join("studio.exr")).unwrap().pixels == hdr.pixels);
        for name in ["studio.png", "studio.jpg"] {
            let ldr = EnvironmentMap::load(&folder.join(name)).unwrap();
            assert_eq!((ldr.width, ldr.height), (hdr.width, hdr.height));
            let close = ldr.pixels.iter().zip(&hdr.pixels)
                .filter(|(l, h)| (0..3).all(|k| (l.array()[k] - h.array()[k].min(1.)).abs() < 0.03)).count();
            assert!(close * 100 > hdr.pixels.len() * 99, "{name}: the HDR's light, clipped at 1");
        }
    }
    #[test]
    fn the_thumbnail_keeps_the_mean_light_and_its_place() {
        let (width, height) = (64, 32);
        let mut pixels = vec![Color::new(0.5, 0.5, 0.5); width * height];
        // A lamp of 2 by 2 texels at the right of the top half.
        for (x, y) in [(48, 8), (49, 8), (48, 9), (49, 9)] {
            pixels[y * width + x] = Color::new(100., 50., 25.);
        }
        let map = EnvironmentMap::new(width, height, pixels).unwrap();
        let small = map.thumbnail(16);
        assert_eq!(small.len(), 16 * 8);
        let mean = |p: &[Color]| p.iter().map(|c| c.x as f64).sum::<f64>() / p.len() as f64;
        assert!((mean(&small) - mean(&map.pixels)).abs() < 1e-4, "the light is kept");
        let brightest = (0..small.len()).max_by(|&a, &b| small[a].x.total_cmp(&small[b].x)).unwrap();
        assert_eq!((brightest % 16, brightest / 16), (12, 2));
        // Larger than the map, each texel repeats.
        assert_eq!(map.thumbnail(128).len(), 128 * 64);
    }
    #[test]
    fn invalid_configuration_is_rejected() {
        let mut config = EnvironmentConfig::default();
        assert!(config.validate().is_err());
        config.path = "studio.hdr".into();
        assert!(config.validate().is_ok());
        config.intensity = -1.;
        assert!(config.validate().is_err());
        config.intensity = 1.;
        config.rotation = f32::NAN;
        assert!(config.validate().is_err());
    }
}

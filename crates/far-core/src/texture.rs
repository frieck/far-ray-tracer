//! Textures.

use crate::vector::Color;
use std::path::Path;

pub struct Texture {
    pub width: i32,
    pub height: i32,
    pub image: Vec<Color>,
}

impl Texture {
    pub fn load(path: &Path) -> Result<Self, String> {
        let img = image::open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .to_rgb8();
        let (width, height) = (img.width() as i32, img.height() as i32);
        let inv256 = 1.0f32 / 256.0;
        let image = img
            .pixels()
            .map(|p| Color::new(p[0] as f32 * inv256, p[1] as f32 * inv256, p[2] as f32 * inv256))
            .collect();
        Ok(Self { width, height, image })
    }

    /// Bilinear lookup.
    ///
    /// The original also uses the width for `fv`, which distorts non-square
    /// textures. It stays that way here because it changes the image.
    #[inline]
    pub fn texel(&self, u: f32, v: f32) -> Color {
        let l = self.width;
        let fu = (u + 1000.5) * l as f32;
        let fv = (v + 1000.0) * l as f32;
        self.bilinear(fu, fv)
    }

    /// Like `texel`, but with the height in `fv`, so non-square textures do not distort.
    #[inline]
    pub fn texel_fixed(&self, u: f32, v: f32) -> Color {
        self.bilinear((u + 1000.5) * self.width as f32, (v + 1000.0) * self.height as f32)
    }

    #[inline]
    fn bilinear(&self, fu: f32, fv: f32) -> Color {
        let (l, a) = (self.width, self.height);
        let u1 = (fu as i32).rem_euclid(l);
        let v1 = (fv as i32).rem_euclid(a);
        let u2 = (u1 + 1) % l;
        let v2 = (v1 + 1) % a;
        let fracu = fu - fu.floor();
        let fracv = fv - fv.floor();
        let w1 = (1.0 - fracu) * (1.0 - fracv);
        let w2 = fracu * (1.0 - fracv);
        let w3 = (1.0 - fracu) * fracv;
        let w4 = fracu * fracv;
        let c = |x: i32, y: i32| self.image[(x + y * l) as usize];
        c(u1, v1) * w1 + c(u2, v1) * w2 + c(u1, v2) * w3 + c(u2, v2) * w4
    }
}

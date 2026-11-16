//! What a camera does to light on its way to the picture, applied as a render is shown, so it
//! changes without rendering again: white balance, tint and vignetting on the light, before
//! tone mapping, and the lateral chromatic aberration of a simple lens and film grain on the
//! finished image.

use crate::engine::{Image, Region};
use std::sync::atomic::Ordering::Relaxed;

/// The ISO, shutter time in seconds and f-number that give the light as rendered: exposure 1.
pub const REFERENCE_ISO: f32 = 100.0;
pub const REFERENCE_SHUTTER: f32 = 1.0 / 125.0;
pub const REFERENCE_F_NUMBER: f32 = 8.0;

/// The exposure multiplier of a camera's ISO, shutter time in seconds and f-number, as a real
/// camera's: twice the ISO or twice the time is twice the light, and so is an f-number smaller
/// by the square root of 2, one stop. ISO 100 at 1/125 s and f/8 leaves it as rendered.
pub fn exposure(iso: f32, shutter: f32, f_number: f32) -> f32 {
    (iso / REFERENCE_ISO) * (shutter / REFERENCE_SHUTTER) * (REFERENCE_F_NUMBER / f_number).powi(2)
}

/// The white balance that leaves colors as rendered: daylight, the white of an sRGB screen.
pub const NEUTRAL_KELVIN: f32 = 6500.0;
/// The range of white balance, from candle light to a blue sky.
pub const MIN_KELVIN: f32 = 1500.0;
pub const MAX_KELVIN: f32 = 15000.0;

/// How far the red and blue images part at the corners with full chromatic aberration, as a
/// share of the distance from the center.
const MAX_ABERRATION: f32 = 0.012;
/// The angle off the lens axis at the corners with full vignetting, whose light falls off as
/// the fourth power of its cosine: at 60 degrees, to a sixteenth.
const MAX_VIGNETTE_ANGLE: f32 = 60.0;
/// Green's change at full tint, in stops.
const MAX_TINT_STOPS: f32 = 0.5;
/// The spread of grain at strength 1, in screen values from 0 to 1.
const GRAIN_SPREAD: f32 = 0.08;
/// The size of a grain as a share of the image's longer side, so a preview and a full render
/// look alike; never under a pixel.
const GRAIN_SIZE: f32 = 1.0 / 1400.0;

/// Per-pixel film settings: the white balance as channel factors, and the vignetting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Film {
    white: [f32; 3],
    /// The tangent of the angle at the corners, 0 for none.
    vignette: f32,
    width: u32,
    height: u32,
}

impl Default for Film {
    fn default() -> Self {
        Self { white: [1.0; 3], vignette: 0.0, width: 1, height: 1 }
    }
}

impl Film {
    /// `kelvin` is the color of the light that should come out white, `tint` from -1 (green) to 1
    /// (magenta), and `vignette` from 0 to 1.
    pub fn new(kelvin: f32, tint: f32, vignette: f32, width: u32, height: u32) -> Self {
        let vignette = if vignette.is_finite() { vignette.clamp(0.0, 1.0) } else { 0.0 } * MAX_VIGNETTE_ANGLE.to_radians().tan();
        Self { white: white_balance(kelvin, tint), vignette, width: width.max(1), height: height.max(1) }
    }

    /// The white balance's factors for each channel of linear sRGB.
    pub fn white(&self) -> [f32; 3] {
        self.white
    }

    /// The light of the pixel at index `p` of the whole image, through the film.
    #[inline]
    pub fn light(&self, mean: [f32; 3], p: usize) -> [f32; 3] {
        let fall = if self.vignette > 0.0 {
            let (x, y) = ((p % self.width as usize) as f32 + 0.5, (p / self.width as usize) as f32 + 0.5);
            self.falloff(x, y)
        } else {
            1.0
        };
        [0, 1, 2].map(|k| mean[k] * self.white[k] * fall)
    }

    /// The share of light that reaches the point (x, y) of the picture, in pixels: the cosine to
    /// the fourth of its angle off the axis, the corners at the angle the strength sets.
    fn falloff(&self, x: f32, y: f32) -> f32 {
        let (cx, cy) = (self.width as f32 / 2.0, self.height as f32 / 2.0);
        let r = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / (cx * cx + cy * cy).sqrt();
        let t = self.vignette * r;
        1.0 / ((1.0 + t * t) * (1.0 + t * t))
    }
}

/// Channel factors that make light of the color temperature `kelvin` come out white, as a
/// camera's white balance does, keeping the brightness of grey. 6500 K changes nothing; lower
/// values suit warmer light and turn the picture bluer. `tint`, from -1 to 1, then moves it
/// along the other axis of white balance: toward magenta, for the green of fluorescent tubes,
/// or toward green.
pub fn white_balance(kelvin: f32, tint: f32) -> [f32; 3] {
    let tint = if tint.is_finite() { tint.clamp(-1.0, 1.0) } else { 0.0 };
    let kelvin = if kelvin.is_finite() { kelvin.clamp(MIN_KELVIN, MAX_KELVIN) } else { NEUTRAL_KELVIN };
    if kelvin == NEUTRAL_KELVIN && tint == 0.0 {
        return [1.0; 3];
    }
    let light = planck_rgb(kelvin);
    let neutral = planck_rgb(NEUTRAL_KELVIN);
    let mut factors = [0, 1, 2].map(|k| neutral[k] / light[k].max(1e-6));
    factors[1] *= (-MAX_TINT_STOPS * tint).exp2();
    let grey = 0.2126 * factors[0] + 0.7152 * factors[1] + 0.0722 * factors[2];
    factors.map(|f| f / grey)
}

/// The range of a light's color temperature: candle flame to clear blue sky, past the fit's ends.
pub const MIN_LIGHT_KELVIN: f32 = 1000.0;
pub const MAX_LIGHT_KELVIN: f32 = 40000.0;

/// The color of light from a black body at `kelvin`, as V-Ray's temperature color mode gives
/// it: linear sRGB of unit luminance, so a light keeps its brightness as its color warms or
/// cools. 6500 K is about white; 2700 K a household bulb, 5500 K midday sun.
pub fn blackbody(kelvin: f32) -> [f32; 3] {
    let kelvin = if kelvin.is_finite() { kelvin.clamp(1667.0, 25000.0) } else { NEUTRAL_KELVIN };
    let rgb = planck_rgb(kelvin);
    let luminance = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    rgb.map(|v| v / luminance.max(1e-6))
}

/// The linear sRGB color of a black body at `kelvin`, of unit luminance: the Planckian locus
/// in CIE xy by the cubic fit of Kim et al. (2002), from 1667 K to 25000 K.
fn planck_rgb(kelvin: f32) -> [f32; 3] {
    let t = kelvin as f64;
    let (t2, t3) = (t * t, t * t * t);
    let x = if t <= 4000.0 {
        -0.2661239e9 / t3 - 0.2343589e6 / t2 + 0.8776956e3 / t + 0.179910
    } else {
        -3.0258469e9 / t3 + 2.1070379e6 / t2 + 0.2226347e3 / t + 0.240390
    };
    let y = if t <= 2222.0 {
        -1.1063814 * x.powi(3) - 1.34811020 * x * x + 2.18555832 * x - 0.20219683
    } else if t <= 4000.0 {
        -0.9549476 * x.powi(3) - 1.37418593 * x * x + 2.09137015 * x - 0.16748867
    } else {
        3.0817580 * x.powi(3) - 5.87338670 * x * x + 3.75112997 * x - 0.37001483
    };
    let (big_x, big_z) = (x / y, (1.0 - x - y) / y);
    [
        3.2404542 * big_x - 1.5371385 - 0.4985314 * big_z,
        -0.9692660 * big_x + 1.8760108 + 0.0415560 * big_z,
        0.0556434 * big_x - 0.2040259 + 1.0572252 * big_z,
    ]
    .map(|v| v.max(0.0) as f32)
}

/// Lateral chromatic aberration: a lens that bends red light less than blue makes the red image
/// a little larger and the blue one smaller, so edges grow colored fringes toward the corners.
/// `strength` from 0 to 1 moves the screen colors of `rendered`, read from within it.
pub fn chromatic_aberration(image: &Image, strength: f32, rendered: Region) {
    if strength.is_nan() || strength <= 0.0 {
        return;
    }
    let strength = strength.min(1.0) * MAX_ABERRATION;
    if rendered.x1 <= rendered.x0 || rendered.y1 <= rendered.y0 {
        return;
    }
    let width = image.width as usize;
    let source: Vec<u32> = image.pixels.iter().map(|p| p.load(Relaxed)).collect();
    let (cx, cy) = (image.width as f32 / 2.0, image.height as f32 / 2.0);
    let (x0, y0) = (rendered.x0 as f32, rendered.y0 as f32);
    let (x1, y1) = (rendered.x1 as f32 - 1.0, rendered.y1 as f32 - 1.0);
    // One channel at a point between pixels, its neighbors weighted by distance.
    let read = |x: f32, y: f32, k: usize| {
        let (x, y) = (x.clamp(x0, x1), y.clamp(y0, y1));
        let (ix, iy) = (x.floor() as usize, y.floor() as usize);
        let (jx, jy) = ((ix + 1).min(x1 as usize), (iy + 1).min(y1 as usize));
        let (fx, fy) = (x - ix as f32, y - iy as f32);
        let at = |px: usize, py: usize| ((source[py * width + px] >> (8 * k)) & 0xff) as f32;
        (at(ix, iy) * (1.0 - fx) + at(jx, iy) * fx) * (1.0 - fy) + (at(ix, jy) * (1.0 - fx) + at(jx, jy) * fx) * fy
    };
    for y in rendered.y0..rendered.y1 {
        for x in rendered.x0..rendered.x1 {
            let p = y as usize * width + x as usize;
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            // Red is larger, so this pixel shows red from nearer the center; blue from farther.
            let red = read(cx + dx * (1.0 - strength) - 0.5, cy + dy * (1.0 - strength) - 0.5, 0);
            let blue = read(cx + dx * (1.0 + strength) - 0.5, cy + dy * (1.0 + strength) - 0.5, 2);
            let byte = |v: f32| (v + 0.5).clamp(0.0, 255.0) as u32;
            image.pixels[p].store((source[p] & 0xff00_ff00) | byte(red) | byte(blue) << 16, Relaxed);
        }
    }
}

/// Film grain: a fixed pattern of fine specks, strongest in the mid-tones, as in a developed
/// film or a sensor at high ISO. `strength` is 1 for a clearly visible grain; the same image
/// always gets the same grain, so it does not crawl when the picture is shown again.
pub fn grain(image: &Image, strength: f32, rendered: Region) {
    if strength.is_nan() || strength <= 0.0 {
        return;
    }
    let spread = strength.min(4.0) * GRAIN_SPREAD;
    let size = (image.width.max(image.height) as f32 * GRAIN_SIZE).max(1.0);
    let width = image.width as usize;
    // Each corner of the grain lattice gets a value from -1 to 1, smoothed between them.
    let corner = |x: i64, y: i64| {
        let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        h ^= h >> 31;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 29;
        // The mean of two uniform values leans toward the middle, as grain does.
        let (a, b) = ((h & 0xffff) as f32 / 65535.0, ((h >> 16) & 0xffff) as f32 / 65535.0);
        a + b - 1.0
    };
    for y in rendered.y0..rendered.y1 {
        for x in rendered.x0..rendered.x1 {
            let (gx, gy) = (x as f32 / size, y as f32 / size);
            let (ix, iy) = (gx.floor() as i64, gy.floor() as i64);
            let (fx, fy) = (gx - gx.floor(), gy - gy.floor());
            let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
            let noise = (corner(ix, iy) * (1.0 - sx) + corner(ix + 1, iy) * sx) * (1.0 - sy)
                + (corner(ix, iy + 1) * (1.0 - sx) + corner(ix + 1, iy + 1) * sx) * sy;
            let p = y as usize * width + x as usize;
            let rgba = image.pixels[p].load(Relaxed);
            let mut out = rgba & 0xff00_0000;
            for k in 0..3 {
                let v = ((rgba >> (8 * k)) & 0xff) as f32 / 255.0;
                // Little in black and in white, most in between.
                let weight = 0.25 + 3.0 * v * (1.0 - v);
                let shown = (v + spread * noise * weight).clamp(0.0, 1.0);
                out |= ((shown * 255.0 + 0.5) as u32).min(255) << (8 * k);
            }
            image.pixels[p].store(out, Relaxed);
        }
    }
}

/// The strength of grain for a `grain` setting from 0 to 1 at an ISO: it grows with the square
/// root of the ISO, as a sensor's noise does, and ISO 100 keeps the setting.
pub fn grain_at_iso(grain: f32, iso: f32) -> f32 {
    if !(grain.is_finite() && iso.is_finite()) || grain <= 0.0 || iso <= 0.0 {
        return 0.0;
    }
    grain.min(1.0) * (iso / 100.0).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_black_body_keeps_its_brightness_and_warms_below_daylight() {
        let luminance = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        for kelvin in [1000.0, 1900.0, 2700.0, 4000.0, 6500.0, 9000.0, 40000.0] {
            assert!((luminance(blackbody(kelvin)) - 1.0).abs() < 1e-4, "{kelvin} K");
        }
        // Daylight is close to the screen's white; a bulb is orange, a blue sky blue.
        assert!(blackbody(6500.0).iter().all(|v| (v - 1.0).abs() < 0.05), "{:?}", blackbody(6500.0));
        let bulb = blackbody(2700.0);
        assert!(bulb[0] > 1.3 && bulb[2] < 0.5, "{bulb:?}");
        let sky = blackbody(12000.0);
        assert!(sky[2] > sky[0], "{sky:?}");
    }

    #[test]
    fn white_balance_is_neutral_at_daylight_and_cools_warm_light() {
        assert_eq!(white_balance(NEUTRAL_KELVIN, 0.0), [1.0; 3]);
        // Close to 6500 K, close to nothing.
        let near = white_balance(6400.0, 0.0);
        assert!(near.iter().all(|f| (f - 1.0).abs() < 0.02), "{near:?}");
        // Balanced for tungsten light, the picture turns blue; for shade, orange.
        let tungsten = white_balance(3200.0, 0.0);
        assert!(tungsten[2] > 1.5 * tungsten[0], "{tungsten:?}");
        let shade = white_balance(9000.0, 0.0);
        assert!(shade[0] > shade[2], "{shade:?}");
        // Grey keeps its brightness.
        let (magenta, green) = (white_balance(NEUTRAL_KELVIN, 1.0), white_balance(NEUTRAL_KELVIN, -1.0));
        assert!(magenta[1] < magenta[0] && magenta[1] < magenta[2] && green[1] > green[0], "{magenta:?} {green:?}");
        for f in [tungsten, shade, magenta, green, white_balance(MIN_KELVIN, 0.0), white_balance(MAX_KELVIN, 0.0)] {
            assert!((0.2126 * f[0] + 0.7152 * f[1] + 0.0722 * f[2] - 1.0).abs() < 1e-5);
            assert!(f.iter().all(|v| v.is_finite() && *v > 0.0));
        }
        // The light of a lamp at a temperature comes out grey when balanced for it.
        let lamp = planck_rgb(3200.0);
        let shown = [0, 1, 2].map(|k| lamp[k] * tungsten[k]);
        assert!((shown[0] - shown[2]).abs() < 1e-3 * shown[1].max(1.0) * 10.0, "{shown:?}");
    }

    #[test]
    fn vignetting_darkens_the_corners_only() {
        let film = Film::new(NEUTRAL_KELVIN, 0.0, 1.0, 101, 51);
        let center = film.light([1.0; 3], 25 * 101 + 50)[0];
        let corner = film.light([1.0; 3], 0)[0];
        assert!((center - 1.0).abs() < 1e-3);
        assert!(corner < 0.08 && corner > 0.05, "{corner}");
        let none = Film::new(NEUTRAL_KELVIN, 0.0, 0.0, 101, 51);
        assert_eq!(none.light([0.5, 0.25, 2.0], 0), [0.5, 0.25, 2.0]);
        // Half the strength, a gentler fall.
        let half = Film::new(NEUTRAL_KELVIN, 0.0, 0.5, 101, 51).light([1.0; 3], 0)[0];
        assert!(half > corner && half < 1.0);
    }

    #[test]
    fn chromatic_aberration_parts_red_and_blue_toward_the_corners() {
        // A white square on black, off center.
        let (w, h) = (64u32, 64u32);
        let image = Image::new(w, h);
        for y in 40..56 {
            for x in 40..56 {
                image.pixels[y * w as usize + x].store(0xffff_ffff, Relaxed);
            }
        }
        let all = Region { x0: 0, y0: 0, x1: w, y1: h };
        let before = image.rgba();
        chromatic_aberration(&image, 0.0, all);
        assert_eq!(image.rgba(), before);
        chromatic_aberration(&image, 1.0, all);
        let pixel = |x: usize, y: usize| image.pixels[y * w as usize + x].load(Relaxed).to_le_bytes();
        // Green stays; just outside the far edge red spills out, and just inside it blue is gone.
        assert!(pixel(56, 48)[0] > 0 && pixel(56, 48)[1] == 0 && pixel(56, 48)[2] == 0, "{:?}", pixel(56, 48));
        assert!(pixel(55, 48)[2] < 255 && pixel(55, 48)[1] == 255, "{:?}", pixel(55, 48));
        // The center does not move.
        assert_eq!(pixel(32, 32), [0, 0, 0, 255]);
    }

    #[test]
    fn exposure_follows_iso_and_shutter() {
        assert_eq!(exposure(REFERENCE_ISO, REFERENCE_SHUTTER, REFERENCE_F_NUMBER), 1.0);
        assert!((exposure(400.0, 1.0 / 250.0, 8.0) - 2.0).abs() < 1e-6);
        assert!((exposure(100.0, 1.0 / 60.0, 8.0) - 125.0 / 60.0).abs() < 1e-6);
        assert!((exposure(100.0, 1.0 / 125.0, 4.0) - 4.0).abs() < 1e-5, "two stops wider, four times the light");
    }

    #[test]
    fn grain_is_fixed_strongest_in_the_mid_tones_and_grows_with_iso() {
        let (w, h) = (128u32, 64u32);
        let grey = |level: u32| {
            let image = Image::new(w, h);
            for p in &image.pixels {
                p.store(0xff00_0000 | (level * 0x0001_0101), Relaxed);
            }
            image
        };
        let all = Region { x0: 0, y0: 0, x1: w, y1: h };
        let spread = |image: &Image, level: u32| image.pixels.iter()
            .map(|p| ((p.load(Relaxed) & 0xff) as i32 - level as i32).unsigned_abs()).sum::<u32>() as f32 / (w * h) as f32;
        let (mid, dark) = (grey(128), grey(8));
        grain(&mid, 1.0, all);
        grain(&dark, 1.0, all);
        assert!(spread(&mid, 128) > 2.0 * spread(&dark, 8), "{} {}", spread(&mid, 128), spread(&dark, 8));
        // Grey grain: the channels move together.
        assert!(mid.pixels.iter().all(|p| { let [r, g, b, _] = p.load(Relaxed).to_le_bytes(); r == g && g == b }));
        // The same image gets the same grain.
        let again = grey(128);
        grain(&again, 1.0, all);
        assert_eq!(again.rgba(), mid.rgba());
        let none = grey(128);
        grain(&none, 0.0, all);
        assert_eq!(none.rgba(), grey(128).rgba());
        assert_eq!(grain_at_iso(0.5, 100.0), 0.5);
        assert_eq!(grain_at_iso(0.5, 400.0), 1.0);
        assert_eq!(grain_at_iso(0.0, 6400.0), 0.0);
    }
}

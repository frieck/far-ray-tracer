//! Light by wavelength, for glass that disperses: a path that crosses it follows one
//! wavelength, weighted by that wavelength's color, so many paths add up to white light split
//! into a spectrum.

use crate::vector::Color;
use std::sync::OnceLock;

/// Visible range, in nanometers.
const SHORTEST: f32 = 380.0;
const LONGEST: f32 = 720.0;

/// Wavelengths at which glass catalogs give the refractive index and the Abbe number.
const D_LINE: f32 = 587.6;
const F_LINE: f32 = 486.1;
const C_LINE: f32 = 656.3;

/// A wavelength across the visible range for a uniform number in [0, 1).
pub fn sample(u: f32) -> f32 {
    SHORTEST + u.clamp(0.0, 1.0) * (LONGEST - SHORTEST)
}

/// One lobe of the color matching fit, steeper on one side than the other.
fn lobe(x: f32, center: f32, below: f32, above: f32) -> f32 {
    let t = (x - center) / if x < center { below } else { above };
    (-0.5 * t * t).exp()
}

/// Linear sRGB of a single wavelength: the CIE 1931 observer, in the multi-lobe fit of
/// Wyman, Sloan and Shirley (2013), with colors outside sRGB clipped.
fn rgb(wavelength: f32) -> Color {
    let l = wavelength;
    let x = 1.056 * lobe(l, 599.8, 37.9, 31.0) + 0.362 * lobe(l, 442.0, 16.0, 26.7) - 0.065 * lobe(l, 501.1, 20.4, 26.2);
    let y = 0.821 * lobe(l, 568.8, 46.9, 40.5) + 0.286 * lobe(l, 530.9, 16.3, 31.1);
    let z = 1.217 * lobe(l, 437.0, 11.8, 36.0) + 0.681 * lobe(l, 459.0, 26.0, 13.8);
    Color::new(
        (3.2406 * x - 1.5372 * y - 0.4986 * z).max(0.0),
        (-0.9689 * x + 1.8758 * y + 0.0415 * z).max(0.0),
        (0.0557 * x - 0.2040 * y + 1.0570 * z).max(0.0),
    )
}

/// The average of `rgb` over the visible range, which `weight` divides by.
fn white() -> Color {
    static WHITE: OnceLock<Color> = OnceLock::new();
    *WHITE.get_or_init(|| {
        const STEPS: usize = 2000;
        let mut sum = Color::default();
        for i in 0..STEPS {
            sum += rgb(sample((i as f32 + 0.5) / STEPS as f32));
        }
        sum * (1.0 / STEPS as f32)
    })
}

/// Color a path of this wavelength carries: averaged over uniformly sampled wavelengths,
/// it is white, so dispersion moves color around without tinting the light.
pub fn weight(wavelength: f32) -> Color {
    let (c, w) = (rgb(wavelength), white());
    Color::new(c.x / w.x, c.y / w.y, c.z / w.z)
}

/// Refractive index at `wavelength` of a glass with index `ior` at the d line and Abbe number
/// `abbe`, by Cauchy's equation n = A + B / λ².
pub fn ior(ior: f32, abbe: f32, wavelength: f32) -> f32 {
    let inverse_square = |nm: f32| 1.0 / (nm * nm * 1e-6);
    let b = (ior - 1.0) / abbe / (inverse_square(F_LINE) - inverse_square(C_LINE));
    let a = ior - b * inverse_square(D_LINE);
    a + b * inverse_square(wavelength)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wavelengths_average_to_white_and_blue_bends_more() {
        let mut sum = Color::default();
        for i in 0..500 {
            sum += weight(sample((i as f32 + 0.5) / 500.0));
        }
        let mean = sum * (1.0 / 500.0);
        assert!((mean - Color::new(1.0, 1.0, 1.0)).length() < 0.01, "{:?}", mean.array());
        assert!(weight(450.0).z > weight(450.0).x && weight(650.0).x > weight(650.0).z);
        // Crown glass: index 1.5168, Abbe 64.17.
        assert!((ior(1.5168, 64.17, D_LINE) - 1.5168).abs() < 1e-4);
        let spread = ior(1.5168, 64.17, F_LINE) - ior(1.5168, 64.17, C_LINE);
        assert!((spread - 0.5168 / 64.17).abs() < 1e-4);
        assert!(ior(1.5, 30.0, 420.0) > ior(1.5, 30.0, 680.0));
    }
}

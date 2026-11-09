//! Film tone mapping: how linear light becomes the colors of the screen.
//!
//! Standard is the program's own curve: linear up to 0.8 and a soft shoulder above it, with no
//! display encoding, as the 2008 images were made. The film curves compress a much wider range
//! of light and encode the result for an sRGB screen:
//!
//! - AgX, Blender's default since 4.0: bright light turns toward white instead of a saturated,
//!   flat color, as on film. This is the fit used by three.js and Godot. Its base look is soft;
//!   AgX Punchy is Blender's look with more contrast and saturation.
//! - ACES, the film industry's curve, in Stephen Hill's fit: more contrast and saturation.
//!
//! Both are scaled so a surface at half brightness shows as in Standard, so a scene keeps its
//! overall level when the curve changes. Shadows then come out a little lighter and highlights
//! softer.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToneMapping {
    #[default]
    Standard,
    Agx,
    AgxPunchy,
    Aces,
}

impl ToneMapping {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "standard" => Some(Self::Standard),
            "agx" => Some(Self::Agx),
            "agx-punchy" | "agx_punchy" => Some(Self::AgxPunchy),
            "aces" => Some(Self::Aces),
            _ => None,
        }
    }
}

type Matrix = [[f32; 3]; 3];

fn mul(m: &Matrix, v: [f32; 3]) -> [f32; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

/// Exposure that makes linear 0.5 show as 0.5, as it does in Standard.
const AGX_MATCH: f32 = 0.3589;
const AGX_PUNCHY_MATCH: f32 = 0.6218;
const ACES_MATCH: f32 = 0.3613;

const SRGB_TO_REC2020: Matrix = [[0.6274, 0.3293, 0.0433], [0.0691, 0.9195, 0.0113], [0.0164, 0.0880, 0.8956]];
const REC2020_TO_SRGB: Matrix = [[1.6605, -0.5876, -0.0728], [-0.1246, 1.1329, -0.0083], [-0.0182, -0.1006, 1.1187]];
/// Moves colors slightly toward white before the curve, so bright saturated light desaturates.
const AGX_INSET: Matrix = [
    [0.856627153315983, 0.0951212405381588, 0.0482516061458583],
    [0.137318972929847, 0.761241990602591, 0.101439036467562],
    [0.11189821299995, 0.0767994186031903, 0.811302368396859],
];
const AGX_OUTSET: Matrix = [
    [1.1271005818144368, -0.11060664309660323, -0.016493938717834573],
    [-0.1413297634984383, 1.157823702216272, -0.016493938717834257],
    [-0.14132976349843826, -0.11060664309660294, 1.2519364065950405],
];
/// The range of stops AgX keeps, around middle grey.
const AGX_MIN_EV: f32 = -12.47393;
const AGX_MAX_EV: f32 = 4.026069;

const ACES_INPUT: Matrix = [[0.59719, 0.35458, 0.04823], [0.07600, 0.90834, 0.01566], [0.02840, 0.13383, 0.83777]];
const ACES_OUTPUT: Matrix = [[1.60475, -0.53108, -0.07367], [-0.10208, 1.10813, -0.00605], [-0.00327, -0.07276, 1.07602]];

/// Linear light to the encoding of an sRGB screen.
fn srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

fn clean(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 })
}

/// The AgX sigmoid on log-encoded values, a polynomial fit.
fn agx_contrast(x: f32) -> f32 {
    let x2 = x * x;
    let x4 = x2 * x2;
    15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x - 0.00232
}

fn agx(color: [f32; 3], punchy: bool) -> [f32; 3] {
    let scale = if punchy { AGX_PUNCHY_MATCH } else { AGX_MATCH };
    let c = mul(&AGX_INSET, mul(&SRGB_TO_REC2020, clean(color).map(|v| v * scale)));
    let c = c.map(|v| ((v.max(1e-10).log2() - AGX_MIN_EV) / (AGX_MAX_EV - AGX_MIN_EV)).clamp(0.0, 1.0));
    let mut c = c.map(agx_contrast);
    if punchy {
        // Blender's Punchy look: power 1.35, then saturation 1.4 around the luminance.
        c = c.map(|v| v.max(0.0).powf(1.35));
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        c = c.map(|v| luma + 1.4 * (v - luma));
    }
    let c = mul(&AGX_OUTSET, c).map(|v| v.max(0.0).powf(2.2));
    mul(&REC2020_TO_SRGB, c).map(srgb)
}

fn aces(color: [f32; 3]) -> [f32; 3] {
    let fit = |v: f32| (v * (v + 0.0245786) - 0.000090537) / (v * (0.983729 * v + 0.4329510) + 0.238081);
    // Hill's fit expects light already scaled by 1 / 0.6.
    let c = mul(&ACES_INPUT, clean(color).map(|v| v * ACES_MATCH / 0.6));
    mul(&ACES_OUTPUT, c.map(fit)).map(srgb)
}

/// The program's own curve: unchanged below the knee, then a shoulder that never reaches 1.
pub(crate) fn shoulder(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    if x.is_nan() || x <= 0.0 {
        0.0
    } else if x < KNEE {
        x
    } else {
        let rest = 1.0 - KNEE;
        KNEE + rest * (1.0 - (-(x - KNEE) / rest).exp())
    }
}

/// Exposed linear light to screen values in [0, 1].
pub fn display(mode: ToneMapping, color: [f32; 3]) -> [f32; 3] {
    match mode {
        ToneMapping::Standard => color.map(shoulder),
        ToneMapping::Agx => agx(color, false),
        ToneMapping::AgxPunchy => agx(color, true),
        ToneMapping::Aces => aces(color),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ToneMapping; 4] = [ToneMapping::Standard, ToneMapping::Agx, ToneMapping::AgxPunchy, ToneMapping::Aces];

    #[test]
    fn grey_stays_grey_and_rises_with_light() {
        for mode in ALL {
            let mut last = -1.0;
            for step in 0..60 {
                let x = 0.001 * 1.25f32.powi(step);
                let [r, g, b] = display(mode, [x; 3]);
                assert!((r - g).abs() < 2e-3 && (g - b).abs() < 2e-3, "{mode:?} tints grey {x}: {r} {g} {b}");
                assert!(g >= last - 1e-5 && (0.0..=1.0).contains(&g), "{mode:?} at {x}: {g}");
                last = g;
            }
            assert!(display(mode, [0.0; 3])[1] < 0.01);
        }
    }

    #[test]
    fn film_curves_keep_half_brightness_and_soften_highlights() {
        for mode in [ToneMapping::Agx, ToneMapping::AgxPunchy, ToneMapping::Aces] {
            assert!((display(mode, [0.5; 3])[1] - 0.5).abs() < 0.01, "{mode:?}");
            // Far brighter light still has room before white.
            assert!(display(mode, [4.0; 3])[1] < 0.97, "{mode:?}");
        }
    }

    #[test]
    fn agx_turns_bright_saturated_light_toward_white() {
        let dim = display(ToneMapping::Agx, [0.5, 0.05, 0.02]);
        let bright = display(ToneMapping::Agx, [40.0, 4.0, 1.6]);
        let spread = |c: [f32; 3]| c[0] - c[2];
        assert!(spread(bright) < spread(dim) * 0.5, "{dim:?} {bright:?}");
    }

    #[test]
    fn bad_values_show_black() {
        for mode in ALL {
            assert_eq!(display(mode, [f32::NAN, -1.0, f32::NEG_INFINITY]).map(|v| v < 0.01), [true; 3]);
        }
    }
}

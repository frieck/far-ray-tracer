//! How much light a lamp sends in each direction: a spotlight's cone, or the measured
//! distribution of a real fixture in an IES file (IESNA LM-63, photometry type C).
//!
//! A profile scales the lamp's own color by a factor from 0 to 1 for each direction leaving it,
//! so the brightest direction keeps the lamp's set intensity.

use crate::vector::{Vec3, PI};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Profile {
    /// Where the lamp points: the spot's axis, or the IES file's 0° vertical angle (straight
    /// down for a ceiling fixture).
    pub axis: Vec3,
    /// The IES file's 0° horizontal angle, perpendicular to `axis`.
    pub across: Vec3,
    /// Cosines of the cone's edge and of where the edge's soft blend begins.
    pub cone: Option<(f32, f32)>,
    pub ies: Option<Arc<Ies>>,
}

impl Profile {
    /// A profile pointing along `axis`; `angle` is the cone's full opening in degrees and
    /// `blend` the share of it that fades out, from 0 (a hard edge) to 1.
    pub fn new(axis: Vec3, angle: Option<f32>, blend: f32, ies: Option<Arc<Ies>>) -> Result<Self, String> {
        let length = axis.length();
        if !(length > 1e-6 && length.is_finite()) {
            return Err("A light's direction must be a nonzero vector".into());
        }
        let axis = axis * (1.0 / length);
        let helper = if axis.y.abs() > 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
        let mut across = helper.cross(axis);
        across.normalize();
        let cone = match angle {
            None => None,
            Some(angle) if angle > 0.0 && angle <= 180.0 && (0.0..=1.0).contains(&blend) => {
                let half = (0.5 * angle).to_radians();
                let inner = half * (1.0 - blend);
                Some((half.cos(), inner.cos()))
            }
            Some(_) => return Err("A spot angle must be from 0 to 180 degrees, and its blend from 0 to 1".into()),
        };
        Ok(Self { axis, across, cone, ies })
    }

    /// The share of the lamp's light that leaves along unit `direction`.
    pub fn factor(&self, direction: Vec3) -> f32 {
        let cos = direction.dot(self.axis).clamp(-1.0, 1.0);
        let mut factor = 1.0;
        if let Some((outer, inner)) = self.cone {
            if cos <= outer {
                return 0.0;
            }
            if cos < inner {
                let t = (cos - outer) / (inner - outer);
                factor = t * t * (3.0 - 2.0 * t);
            }
        }
        if let Some(ies) = &self.ies {
            let side = self.axis.cross(self.across);
            let flat = direction - self.axis * cos;
            let horizontal = flat.dot(side).atan2(flat.dot(self.across)).to_degrees();
            factor *= ies.value(cos.acos().to_degrees(), horizontal);
        }
        factor
    }
}

/// The candela table of an IES file, scaled so its brightest value is 1.
#[derive(Debug)]
pub struct Ies {
    vertical: Vec<f32>,
    horizontal: Vec<f32>,
    /// One row of vertical values per horizontal angle.
    values: Vec<Vec<f32>>,
}

impl Ies {
    pub fn parse(text: &str) -> Result<Self, String> {
        let tilt = text.lines().position(|line| line.trim_start().to_ascii_uppercase().starts_with("TILT="))
            .ok_or("Not an IES file: no TILT line")?;
        let tilt_line = text.lines().nth(tilt).unwrap_or_default().trim().to_ascii_uppercase();
        let mut numbers = text.lines().skip(tilt + 1)
            .flat_map(|line| line.split(|c: char| c.is_whitespace() || c == ','))
            .filter(|word| !word.is_empty())
            .map(|word| word.parse::<f32>().map_err(|_| format!("IES file: {word} is not a number")));
        let mut next = || numbers.next().unwrap_or(Err("IES file ends early".into()));
        if tilt_line == "TILT=INCLUDE" {
            // Lamp-to-luminaire geometry, then angles and multipliers this program does not use.
            next()?;
            let count = next()? as usize;
            for _ in 0..2 * count {
                next()?;
            }
        }
        let _lamps = next()?;
        let _lumens = next()?;
        let _multiplier = next()?;
        let vertical_count = next()? as usize;
        let horizontal_count = next()? as usize;
        let photometry = next()?;
        for _ in 0..4 {
            next()?; // Units, width, length and height.
        }
        for _ in 0..3 {
            next()?; // Ballast factor, a reserved value and input watts.
        }
        if photometry != 1.0 {
            return Err("Only IES files with type C photometry are supported".into());
        }
        if vertical_count < 2 || horizontal_count < 1 || vertical_count * horizontal_count > 1_000_000 {
            return Err("An IES file needs at least two vertical angles and one horizontal angle".into());
        }
        let vertical: Vec<f32> = (0..vertical_count).map(|_| next()).collect::<Result<_, _>>()?;
        let horizontal: Vec<f32> = (0..horizontal_count).map(|_| next()).collect::<Result<_, _>>()?;
        let mut values = Vec::with_capacity(horizontal_count);
        for _ in 0..horizontal_count {
            values.push((0..vertical_count).map(|_| next()).collect::<Result<Vec<f32>, _>>()?);
        }
        let ascending = |angles: &[f32]| angles.windows(2).all(|w| w[0] < w[1]);
        if !ascending(&vertical) || !ascending(&horizontal) {
            return Err("IES angles must increase".into());
        }
        let peak = values.iter().flatten().fold(0.0f32, |a, &b| a.max(b));
        if !(peak > 0.0 && peak.is_finite()) || values.iter().flatten().any(|v| *v < 0.0) {
            return Err("IES candela values must be zero or more, and not all zero".into());
        }
        for row in &mut values {
            for v in row.iter_mut() {
                *v /= peak;
            }
        }
        Ok(Self { vertical, horizontal, values })
    }

    /// The scaled intensity at a vertical angle (0 along the axis) and a horizontal angle, in
    /// degrees, between the table's angles.
    pub fn value(&self, vertical: f32, horizontal: f32) -> f32 {
        let last = *self.horizontal.last().unwrap_or(&0.0);
        // The file lists only part of the circle when the fixture is symmetric.
        let mut h = horizontal.rem_euclid(360.0);
        if last <= 0.0 {
            h = 0.0;
        } else if last <= 90.0 {
            h %= 180.0;
            if h > 90.0 { h = 180.0 - h; }
        } else if last <= 180.0 && h > 180.0 {
            h = 360.0 - h;
        }
        let row = |index: usize| interpolate(&self.vertical, &self.values[index], vertical);
        if self.horizontal.len() == 1 {
            return row(0);
        }
        // Between the listed horizontal angles, wrapping around a full circle.
        let full = last >= 360.0 - 1e-3;
        let after = self.horizontal.partition_point(|&a| a <= h);
        if after == 0 {
            return row(0);
        }
        if after >= self.horizontal.len() {
            return if full { row(0) } else { row(self.horizontal.len() - 1) };
        }
        let (a, b) = (self.horizontal[after - 1], self.horizontal[after]);
        let t = (h - a) / (b - a);
        row(after - 1) * (1.0 - t) + row(after) * t
    }
}

fn interpolate(angles: &[f32], values: &[f32], at: f32) -> f32 {
    if at < angles[0] || at > angles[angles.len() - 1] {
        return 0.0;
    }
    let after = angles.partition_point(|&a| a <= at).min(angles.len() - 1).max(1);
    let (a, b) = (angles[after - 1], angles[after]);
    let t = ((at - a) / (b - a)).clamp(0.0, 1.0);
    values[after - 1] * (1.0 - t) + values[after] * t
}

/// The mean factor over the sphere, how much of a bare lamp's light a profile lets out.
pub fn average(profile: &Profile, samples: usize) -> f32 {
    let mut sum = 0.0;
    for i in 0..samples {
        let z = 1.0 - 2.0 * (i as f32 + 0.5) / samples as f32;
        let r = (1.0 - z * z).max(0.0).sqrt();
        let angle = i as f32 * PI * (3.0 - 5f32.sqrt());
        sum += profile.factor(Vec3::new(r * angle.cos(), z, r * angle.sin()));
    }
    sum / samples as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOWN: Vec3 = Vec3 { x: 0.0, y: -1.0, z: 0.0 };

    #[test]
    fn a_cone_lights_inside_and_fades_at_its_edge() {
        let spot = Profile::new(DOWN, Some(60.0), 0.2, None).unwrap();
        let at = |degrees: f32| {
            let a = degrees.to_radians();
            spot.factor(Vec3::new(a.sin(), -a.cos(), 0.0))
        };
        assert_eq!(at(0.0), 1.0);
        assert_eq!(at(23.0), 1.0);
        assert!(at(27.0) > 0.0 && at(27.0) < 1.0);
        assert_eq!(at(31.0), 0.0);
        assert_eq!(at(120.0), 0.0);
        // About the cone's share of the sphere: (1 - cos 30°) / 2, less the faded edge.
        let share = average(&spot, 20000);
        assert!(share > 0.055 && share < 0.067, "{share}");
        assert!(Profile::new(DOWN, Some(200.0), 0.1, None).is_err());
        assert!(Profile::new(Vec3::default(), None, 0.1, None).is_err());
    }

    const SAMPLE: &str = "IESNA:LM-63-2002\n[TEST] downlight\nTILT=NONE\n1 1000 1 3 2 1 2 0.1 0.1 0\n1 1 50\n0 45 90\n0 90\n200 100 0\n100 50 0\n";

    #[test]
    fn ies_tables_scale_to_their_peak_and_fill_in_symmetry() {
        let ies = Arc::new(Ies::parse(SAMPLE).unwrap());
        assert_eq!(ies.value(0.0, 0.0), 1.0);
        assert!((ies.value(22.5, 0.0) - 0.75).abs() < 1e-6);
        assert_eq!(ies.value(90.0, 0.0), 0.0);
        assert_eq!(ies.value(120.0, 0.0), 0.0);
        // Quadrant symmetry: 0 to 90° mirrors around the circle.
        assert!((ies.value(0.0, 90.0) - 0.5).abs() < 1e-6);
        assert!((ies.value(0.0, 45.0) - 0.75).abs() < 1e-6);
        assert!((ies.value(0.0, 270.0) - 0.5).abs() < 1e-6);
        assert!((ies.value(0.0, 180.0) - 1.0).abs() < 1e-6);
        let lamp = Profile::new(DOWN, None, 0.0, Some(ies)).unwrap();
        assert_eq!(lamp.factor(DOWN), 1.0);
        assert_eq!(lamp.factor(Vec3::new(0.0, 1.0, 0.0)), 0.0);
        assert!(Ies::parse("no tilt here").is_err());
        assert!(Ies::parse("TILT=NONE\n1 1000 1 2 1 2 2 0 0 0\n1 1 0\n0 90\n0\n1 0\n").is_err());
    }
}

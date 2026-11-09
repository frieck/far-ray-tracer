//! A physical sky and sun, computed instead of read from an HDR photo.
//!
//! The sky follows Preetham, Shirley and Smits, "A Practical Analytic Model for Daylight"
//! (1999): its brightness and color across the dome from the sun's position and the air's
//! turbidity, the haze (2 is a clear mountain sky, 3 a clear day, 6 a hazy one, 10 smog). The
//! sun is a disk of its true size, whose light reddens and dims through the air it crosses,
//! more so near the horizon.
//!
//! The result is drawn into an environment map, so it lights the scene, shows in the
//! background and is sampled by importance exactly like an HDR image. The sun's disk is
//! scaled so its light matches the sun's whatever the map's resolution.

use crate::environment::EnvironmentMap;
use crate::vector::{Color, Vec3, PI};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SkyConfig {
    /// The sun's height above the horizon, in degrees.
    pub elevation: f32,
    /// The direction the sun is in, in degrees: 0 toward +Z, 90 toward +X.
    pub azimuth: f32,
    /// Haze, from 2 (very clear) to 10 (smog).
    pub turbidity: f32,
    /// The sun's apparent diameter in degrees; 0.53 is the real sun. Larger softens shadows.
    pub sun_size: f32,
    /// Multiplies the sun's light, and leaves the sky as it is.
    pub sun_strength: f32,
    /// When set, the sun's position comes from the time of day instead of `elevation` and
    /// `azimuth`.
    pub time: Option<SunTime>,
}

impl Default for SkyConfig {
    fn default() -> Self {
        Self { elevation: 35.0, azimuth: 140.0, turbidity: 3.0, sun_size: 0.53, sun_strength: 1.0, time: None }
    }
}

/// A time and place, for the sun's position. North is +Z; the sun rises toward +X.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SunTime {
    /// Local solar time, in hours from midnight: 12 is noon.
    pub hour: f32,
    /// Degrees north of the equator; negative is south.
    pub latitude: f32,
    /// Day of the year, from 1 (January 1) to 365.
    pub day: f32,
}

impl Default for SunTime {
    fn default() -> Self {
        Self { hour: 15.0, latitude: -27.0, day: 80.0 }
    }
}

impl SkyConfig {
    pub fn validate(&self) -> Result<(), String> {
        let finite = [self.elevation, self.azimuth, self.turbidity, self.sun_size, self.sun_strength]
            .iter().all(|v| v.is_finite());
        if !finite || !(-90.0..=90.0).contains(&self.elevation) || !(1.7..=12.0).contains(&self.turbidity)
            || !(0.05..=20.0).contains(&self.sun_size) || !(0.0..=100.0).contains(&self.sun_strength) {
            return Err("A sky needs an elevation from -90 to 90, a turbidity from 1.7 to 12, a sun size from 0.05 to 20 degrees and a sun strength from 0 to 100".into());
        }
        if let Some(time) = self.time {
            if !(0.0..=24.0).contains(&time.hour) || !(-90.0..=90.0).contains(&time.latitude) || !(1.0..=366.0).contains(&time.day) {
                return Err("A sun time needs an hour from 0 to 24, a latitude from -90 to 90 and a day from 1 to 366".into());
            }
        }
        Ok(())
    }

    /// The sun's elevation and azimuth in degrees, from the time when it is set.
    pub fn sun_position(&self) -> (f32, f32) {
        let Some(time) = self.time else { return (self.elevation, self.azimuth) };
        let declination = (23.44f32).to_radians() * (2.0 * PI * (284.0 + time.day) / 365.0).sin();
        let hour_angle = (15.0 * (time.hour - 12.0)).to_radians();
        let latitude = time.latitude.to_radians();
        let sin_elevation = latitude.sin() * declination.sin() + latitude.cos() * declination.cos() * hour_angle.cos();
        let elevation = sin_elevation.clamp(-1.0, 1.0).asin();
        // Measured from north, toward east.
        let azimuth = (-hour_angle.sin()).atan2(declination.tan() * latitude.cos() - latitude.sin() * hour_angle.cos());
        // North is +Z and east +X.
        (elevation.to_degrees(), azimuth.to_degrees().rem_euclid(360.0))
    }

    /// The unit direction toward the sun.
    pub fn sun_direction(&self) -> Vec3 {
        let (elevation, azimuth) = self.sun_position();
        let (e, a) = (elevation.to_radians(), azimuth.to_radians());
        Vec3::new(a.sin() * e.cos(), e.sin(), a.cos() * e.cos())
    }
}

/// Sky brightness per thousand candela per square meter of the model, in the program's units:
/// a clear midday zenith comes out near 0.15.
const SKY_SCALE: f32 = 0.05;
/// Light from the sun on a surface facing it, before the air: a white matte surface there
/// shows about 1.
const SUN_IRRADIANCE: f32 = PI;

struct Perez {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
}

impl Perez {
    fn at(&self, theta: f32, gamma: f32) -> f32 {
        let cos_theta = theta.cos().max(0.01);
        (1.0 + self.a * (self.b / cos_theta).exp()) * (1.0 + self.c * (self.d * gamma).exp() + self.e * gamma.cos().powi(2))
    }
}

/// The sky's xyY color in one direction, at angle `theta` from the zenith and `gamma` from the
/// sun, for a sun at `theta_sun` from the zenith.
struct Preetham {
    luminance: (Perez, f32),
    x: (Perez, f32),
    y: (Perez, f32),
    theta_sun: f32,
}

impl Preetham {
    fn new(turbidity: f32, theta_sun: f32) -> Self {
        let t = turbidity;
        let luminance = Perez { a: 0.1787 * t - 1.4630, b: -0.3554 * t + 0.4275, c: -0.0227 * t + 5.3251,
            d: 0.1206 * t - 2.5771, e: -0.0670 * t + 0.3703 };
        let x = Perez { a: -0.0193 * t - 0.2592, b: -0.0665 * t + 0.0008, c: -0.0004 * t + 0.2125,
            d: -0.0641 * t - 0.8989, e: -0.0033 * t + 0.0452 };
        let y = Perez { a: -0.0167 * t - 0.2608, b: -0.0950 * t + 0.0092, c: -0.0079 * t + 0.2102,
            d: -0.0441 * t - 1.6537, e: -0.0109 * t + 0.0529 };
        let chi = (4.0 / 9.0 - t / 120.0) * (PI - 2.0 * theta_sun);
        let zenith_luminance = ((4.0453 * t - 4.9710) * chi.tan() - 0.2155 * t + 2.4192).max(0.0);
        let s = theta_sun;
        let (s2, s3, t2) = (s * s, s * s * s, t * t);
        let zenith_x = t2 * (0.00166 * s3 - 0.00375 * s2 + 0.00209 * s)
            + t * (-0.02903 * s3 + 0.06377 * s2 - 0.03202 * s + 0.00394)
            + (0.11693 * s3 - 0.21196 * s2 + 0.06052 * s + 0.25886);
        let zenith_y = t2 * (0.00275 * s3 - 0.00610 * s2 + 0.00317 * s)
            + t * (-0.04214 * s3 + 0.08970 * s2 - 0.04153 * s + 0.00516)
            + (0.15346 * s3 - 0.26756 * s2 + 0.06670 * s + 0.26688);
        Self { luminance: (luminance, zenith_luminance), x: (x, zenith_x), y: (y, zenith_y), theta_sun }
    }

    fn color(&self, theta: f32, gamma: f32) -> Color {
        let value = |(perez, zenith): &(Perez, f32)| zenith * perez.at(theta, gamma) / perez.at(0.0, self.theta_sun);
        let (big_y, x, y) = (value(&self.luminance), value(&self.x), value(&self.y));
        xyy_to_rgb(x, y, big_y)
    }
}

fn xyy_to_rgb(x: f32, y: f32, big_y: f32) -> Color {
    if y <= 1e-6 || big_y <= 0.0 {
        return Color::default();
    }
    let big_x = x / y * big_y;
    let big_z = (1.0 - x - y) / y * big_y;
    Color::new(
        (3.2406 * big_x - 1.5372 * big_y - 0.4986 * big_z).max(0.0),
        (-0.9689 * big_x + 1.8758 * big_y + 0.0415 * big_z).max(0.0),
        (0.0557 * big_x - 0.2040 * big_y + 1.0570 * big_z).max(0.0),
    )
}

/// Share of sunlight of each color that crosses the air to the ground, for a sun at
/// `elevation` degrees: Rayleigh scattering takes the blue first, and haze dims all colors.
pub fn sun_transmittance(elevation: f32, turbidity: f32) -> Color {
    if elevation <= -1.0 {
        return Color::default();
    }
    let zenith = (90.0 - elevation.max(0.0)).min(90.0);
    // Kasten and Young's relative air mass: 1 overhead, about 38 at the horizon.
    let air_mass = 1.0 / (zenith.to_radians().cos() + 0.50572 * (96.07995 - zenith).powf(-1.6364));
    let haze = 0.04 * (turbidity - 1.0);
    let depth = [0.03 + haze * 0.8, 0.07 + haze, 0.17 + haze * 1.2];
    let t = depth.map(|d| (-air_mass * d).exp());
    Color::new(t[0], t[1], t[2])
}

/// Map width; the height is half. A texel is about a third of a degree, near the sun's size.
const WIDTH: usize = 1024;

/// Draws the sky and the sun into an environment map, in the program's light units.
pub fn render(config: &SkyConfig) -> Result<EnvironmentMap, String> {
    config.validate()?;
    let (elevation, _) = config.sun_position();
    let sun = config.sun_direction();
    // The model holds for a sun above the horizon; below it, the sky dims through twilight.
    let theta_sun = (90.0 - elevation.max(0.5)).to_radians();
    let model = Preetham::new(config.turbidity, theta_sun);
    let twilight = ((elevation + 8.0) / 8.0).clamp(0.0, 1.0);
    let twilight = twilight * twilight * (3.0 - 2.0 * twilight);
    let (width, height) = (WIDTH, WIDTH / 2);
    let direction = |x: f32, y: f32| {
        // The same layout as EnvironmentMap: +Z at the center, +Y at the top.
        let phi = 2.0 * PI * x / width as f32;
        let theta = PI * y / height as f32;
        Vec3::new(-theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos())
    };
    let sky = |d: Vec3| {
        let up = d.y.max(0.0);
        let theta = up.clamp(0.0, 1.0).acos();
        let gamma = d.dot(sun).clamp(-1.0, 1.0).acos();
        model.color(theta.min(PI * 0.5 - 0.001), gamma) * (SKY_SCALE * twilight)
    };
    // The ground below the horizon: a dull gray lit by the sky, so reflections have a floor.
    let horizon = {
        let mut sum = Color::default();
        for i in 0..64 {
            let a = 2.0 * PI * i as f32 / 64.0;
            sum += sky(Vec3::new(a.sin(), 0.02, a.cos()));
        }
        sum * (1.0 / 64.0)
    };
    let ground = horizon * 0.3 + sun_transmittance(elevation, config.turbidity)
        * (SUN_IRRADIANCE * config.sun_strength * elevation.max(0.0).to_radians().sin() * 0.25 / PI);
    let mut pixels = Vec::with_capacity(width * height);
    for row in 0..height {
        for column in 0..width {
            let d = direction(column as f32 + 0.5, row as f32 + 0.5);
            pixels.push(if d.y >= 0.0 {
                sky(d)
            } else {
                // Blends into the ground over a few degrees, so the horizon has no hard seam.
                let t = (-d.y / 0.05).min(1.0);
                sky(Vec3::new(d.x, 0.0, d.z)) * (1.0 - t) + ground * t
            });
        }
    }
    add_sun(&mut pixels, width, height, config, sun, elevation, &direction);
    EnvironmentMap::new(width, height, pixels)
}

/// Paints the sun's disk, with the texels it partly covers weighed by how much, and scales it
/// so the light it sends matches the sun's.
fn add_sun(pixels: &mut [Color], width: usize, height: usize, config: &SkyConfig, sun: Vec3, elevation: f32,
    direction: &dyn Fn(f32, f32) -> Vec3) {
    let light = sun_transmittance(elevation, config.turbidity) * (SUN_IRRADIANCE * config.sun_strength);
    if light.x.max(light.y).max(light.z) <= 0.0 {
        return;
    }
    let radius = (0.5 * config.sun_size).to_radians();
    let cos_radius = radius.cos();
    let (u, v) = (((sun.x.atan2(sun.z) / (2.0 * PI) + 0.5).rem_euclid(1.0)), sun.y.clamp(-1.0, 1.0).acos() / PI);
    let rows = (radius / PI * height as f32).ceil() as isize + 2;
    let center_row = (v * height as f32) as isize;
    const SUB: usize = 4;
    let mut covered = Vec::new();
    let mut total_solid_angle = 0.0f64;
    for row in (center_row - rows).max(0)..=(center_row + rows).min(height as isize - 1) {
        let row = row as usize;
        let theta0 = PI * row as f32 / height as f32;
        let theta1 = PI * (row + 1) as f32 / height as f32;
        let texel_solid_angle = 2.0 * PI / width as f32 * (theta0.cos() - theta1.cos());
        // Near the poles a few degrees span the whole width.
        let sin = (0.5 * (theta0 + theta1)).sin().max(1e-3);
        let columns = ((radius / sin) / (2.0 * PI) * width as f32).ceil() as isize + 2;
        let center_column = (u * width as f32) as isize;
        for column in center_column - columns.min(width as isize / 2)..=center_column + columns.min(width as isize / 2) {
            let wrapped = column.rem_euclid(width as isize) as usize;
            let mut inside = 0;
            for i in 0..SUB * SUB {
                let (sx, sy) = ((i % SUB) as f32 + 0.5, (i / SUB) as f32 + 0.5);
                let d = direction(column as f32 + sx / SUB as f32, row as f32 + sy / SUB as f32);
                if d.dot(sun) >= cos_radius {
                    inside += 1;
                }
            }
            if inside > 0 {
                let coverage = inside as f32 / (SUB * SUB) as f32;
                covered.push((row * width + wrapped, coverage));
                total_solid_angle += (coverage * texel_solid_angle) as f64;
            }
        }
    }
    if total_solid_angle <= 0.0 {
        return;
    }
    let radiance = light * (1.0 / total_solid_angle as f32);
    for (index, coverage) in covered {
        pixels[index] += radiance * coverage;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_follows_the_clock_and_the_season() {
        let at = |hour: f32, latitude: f32, day: f32| {
            SkyConfig { time: Some(SunTime { hour, latitude, day }), ..SkyConfig::default() }.sun_position()
        };
        // At an equinox on the equator, noon is overhead and 6 am is on the horizon, to the east.
        let (noon, _) = at(12.0, 0.0, 80.0);
        assert!(noon > 88.0, "{noon}");
        let (morning, east) = at(6.0, 0.0, 80.0);
        assert!(morning.abs() < 2.0, "{morning}");
        assert!((east - 90.0).abs() < 3.0, "{east}");
        // Afternoon sun is in the west, and lower in the southern winter than in summer.
        let (_, west) = at(16.0, -27.0, 172.0);
        assert!(west > 180.0, "{west}");
        assert!(at(12.0, -27.0, 172.0).0 < at(12.0, -27.0, 355.0).0);
    }

    #[test]
    fn the_sun_in_the_map_lights_as_much_as_the_sun() {
        let config = SkyConfig { elevation: 50.0, azimuth: 30.0, ..SkyConfig::default() };
        let map = render(&config).unwrap();
        let sun = config.sun_direction();
        // Integrate the map's light within two degrees of the sun.
        let samples = 400_000;
        let mut sum = Color::default();
        let cap = 2f32.to_radians().cos();
        for i in 0..samples {
            let z = 1.0 - (1.0 - cap) * (i as f32 + 0.5) / samples as f32;
            let r = (1.0 - z * z).max(0.0).sqrt();
            let a = i as f32 * PI * (3.0 - 5f32.sqrt());
            let local = Vec3::new(r * a.cos(), r * a.sin(), z);
            let helper = if sun.y.abs() > 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
            let mut t = helper.cross(sun);
            t.normalize();
            let b = sun.cross(t);
            sum += map.radiance(t * local.x + b * local.y + sun * local.z);
        }
        let solid_angle = 2.0 * PI * (1.0 - cap);
        let measured = sum * (solid_angle / samples as f32);
        let expected = sun_transmittance(50.0, 3.0) * SUN_IRRADIANCE;
        assert!((measured.y - expected.y).abs() < 0.05 * expected.y, "{measured:?} vs {expected:?}");
        // The sun is far brighter than the sky.
        assert!(map.radiance(sun).y > 1000.0 * map.radiance(Vec3::new(0.0, 1.0, 0.0)).y);
    }

    #[test]
    fn the_sky_is_blue_overhead_and_the_low_sun_is_red() {
        let map = render(&SkyConfig { elevation: 45.0, ..SkyConfig::default() }).unwrap();
        let zenith = map.radiance(Vec3::new(0.0, 1.0, 0.0));
        assert!(zenith.z > zenith.x, "{zenith:?}");
        let low = sun_transmittance(2.0, 3.0);
        let high = sun_transmittance(60.0, 3.0);
        assert!(low.x > low.z * 3.0 && high.x < high.z * 1.5, "{low:?} {high:?}");
        assert_eq!(sun_transmittance(-5.0, 3.0), Color::default());
        // Below the horizon the sun is gone and the sky dims.
        let night = render(&SkyConfig { elevation: -10.0, ..SkyConfig::default() }).unwrap();
        assert!(night.radiance(Vec3::new(0.0, 1.0, 0.0)).y < 1e-6);
        assert!(SkyConfig { turbidity: 20.0, ..SkyConfig::default() }.validate().is_err());
    }
}

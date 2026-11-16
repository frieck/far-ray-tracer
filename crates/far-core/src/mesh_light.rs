//! Mesh lights: triangles and spheres whose material glows, sampled as lights in Enhanced mode.
//! Without them a glowing surface lights the scene only when a bounced ray happens to meet it,
//! so a small bright one, such as a candle flame, shows as scattered dots of light; sampled,
//! each shading point aims a shadow ray at one of them, drawn by the light it gives off.
//!
//! The light's units are those of panels: a triangle of emission `c` sends radiance `c` from
//! both of its sides, as a glowing surface always has in this program, and a sphere from all
//! around. A point on a triangle is drawn evenly over its area; on a sphere, by drawing a
//! direction evenly within the cone the sphere fills as seen from the shaded point, which wastes
//! no sample on its far side.

use crate::scene::{Primitive, Scene};
use crate::vector::{Color, Vec3};

/// One glowing triangle or sphere.
#[derive(Clone, Debug)]
pub struct MeshLight {
    /// Its index in `Scene::prims`.
    pub prim: usize,
    /// A sphere's radius; 0 for a triangle.
    pub radius: f32,
    /// A triangle's corner and the two edges from it; a sphere's center.
    pub p0: Vec3,
    pub e1: Vec3,
    pub e2: Vec3,
    /// The unit normal of a triangle's plane.
    pub normal: Vec3,
    /// A triangle's area; for a sphere, the half of its surface a point sees at most.
    pub area: f32,
    /// The mean radiance it gives off, its map averaged: what drawing it is weighed by.
    pub radiance: Color,
    /// The chance it is drawn, and the chances of it and the lights before it together.
    pub chance: f32,
    pub cumulative: f32,
}

/// Every glowing triangle and sphere of a scene, drawn by the light each gives off: its mean
/// radiance times its area.
#[derive(Clone, Debug, Default)]
pub struct MeshLights {
    pub lights: Vec<MeshLight>,
    /// For each primitive, one more than its index in `lights`; 0 for one that does not glow.
    /// Empty when nothing glows.
    of_prim: Vec<u32>,
}

/// Mesh lights take no part in Fixed mode, and fewer than this share of the brightest
/// triangle's power is left out, so a large dim mesh does not dilute the draw.
const NEGLIGIBLE: f32 = 1e-6;

impl MeshLights {
    pub fn build(scene: &Scene) -> Self {
        let mut map_means: Vec<Option<Color>> = vec![None; scene.textures.len()];
        let mut lights = Vec::new();
        for (index, prim) in scene.prims.iter().enumerate() {
            let material = match prim {
                // A displaced triangle lies off the shape its maps are read from.
                Primitive::Triangle(t) if scene.displaced_face(index).is_none() => t.material,
                Primitive::Sphere(s) if s.radius > 0.0 && s.radius.is_finite() => s.material,
                _ => continue,
            };
            let Some(emission) = scene.materials[material].maps.emission else { continue };
            let mut radiance = emission.color;
            if let Some(map) = emission.map {
                let mean = *map_means[map].get_or_insert_with(|| {
                    let texels = &scene.textures[map].image;
                    let sum = texels.iter().fold(Color::default(), |s, &c| s + c);
                    sum * (1.0 / texels.len().max(1) as f32)
                });
                radiance = radiance * mean;
            }
            if luminance(radiance) <= 0.0 {
                continue;
            }
            let t = match prim {
                Primitive::Sphere(s) => {
                    lights.push(MeshLight {
                        prim: index, radius: s.radius, p0: s.center, e1: Vec3::default(), e2: Vec3::default(),
                        normal: Vec3::default(), area: 2.0 * std::f32::consts::PI * s.radius * s.radius, radiance,
                        chance: 0.0, cumulative: 0.0,
                    });
                    continue;
                }
                Primitive::Triangle(t) => t,
                _ => continue,
            };
            let [a, b, c] = t.vertices.map(|v| scene.vertices[v as usize].pos);
            let (e1, e2) = (b - a, c - a);
            let mut normal = e1.cross(e2);
            let twice_area = normal.length();
            if twice_area <= 0.0 || !twice_area.is_finite() {
                continue;
            }
            normal *= 1.0 / twice_area;
            lights.push(MeshLight {
                prim: index, radius: 0.0, p0: a, e1, e2, normal, area: 0.5 * twice_area, radiance, chance: 0.0, cumulative: 0.0,
            });
        }
        let power = |l: &MeshLight| luminance(l.radiance) * l.area;
        let brightest = lights.iter().map(power).fold(0.0, f32::max);
        lights.retain(|l| power(l) > NEGLIGIBLE * brightest);
        let total: f64 = lights.iter().map(|l| power(l) as f64).sum();
        let mut sum = 0.0f64;
        for light in &mut lights {
            let p = power(light) as f64 / total;
            sum += p;
            light.chance = p as f32;
            light.cumulative = sum as f32;
        }
        if let Some(last) = lights.last_mut() {
            last.cumulative = 1.0;
        }
        let mut of_prim = Vec::new();
        if !lights.is_empty() {
            of_prim = vec![0; scene.prims.len()];
            for (k, light) in lights.iter().enumerate() {
                of_prim[light.prim] = k as u32 + 1;
            }
        }
        Self { lights, of_prim }
    }

    pub fn is_empty(&self) -> bool {
        self.lights.is_empty()
    }

    /// The light drawn by `u` in [0, 1): one of the brighter ones more often.
    pub fn pick(&self, u: f32) -> Option<&MeshLight> {
        let k = self.lights.partition_point(|l| l.cumulative <= u);
        self.lights.get(k.min(self.lights.len().saturating_sub(1)))
    }

    /// The mesh light of primitive `prim`, if it is one.
    pub fn of(&self, prim: usize) -> Option<&MeshLight> {
        match self.of_prim.get(prim) {
            Some(&k) if k > 0 => self.lights.get(k as usize - 1),
            _ => None,
        }
    }
}

impl MeshLight {
    /// A point on the light for `from` to aim at, drawn by `u` and `v` in [0, 1), and the
    /// density of drawing this light and that point, over directions from `from`. None when
    /// the light cannot be seen from there.
    pub fn sample(&self, from: Vec3, u: f32, v: f32) -> Option<(Vec3, f32)> {
        if self.radius > 0.0 {
            let to_center = self.p0 - from;
            let distance = to_center.length();
            let cone = cone(distance, self.radius)?;
            let axis = to_center * (1.0 / distance);
            let cos = 1.0 - u * cone;
            let sin = (1.0 - cos * cos).max(0.0).sqrt();
            let angle = 2.0 * std::f32::consts::PI * v;
            let (t, b) = basis(axis);
            let dir = t * (sin * angle.cos()) + b * (sin * angle.sin()) + axis * cos;
            // The near side, where the drawn direction meets the sphere.
            let along = distance * cos - (self.radius * self.radius - distance * distance * sin * sin).max(0.0).sqrt();
            return Some((from + dir * along.max(0.0), self.chance / (2.0 * std::f32::consts::PI * cone)));
        }
        let root = u.sqrt();
        let target = self.p0 + self.e1 * (1.0 - v) * root + self.e2 * (v * root);
        let delta = target - from;
        let distance = delta.length();
        if distance <= 0.0 {
            return None;
        }
        let pdf = self.pdf_from(from, delta * (1.0 / distance), distance);
        (pdf > 0.0).then_some((target, pdf))
    }

    /// The density `sample` gives the direction `d` from `from`, which meets the light at
    /// `distance`; 0 where it never draws it.
    pub fn pdf_from(&self, from: Vec3, d: Vec3, distance: f32) -> f32 {
        if self.radius > 0.0 {
            return match cone((self.p0 - from).length(), self.radius) {
                Some(cone) => self.chance / (2.0 * std::f32::consts::PI * cone),
                None => 0.0,
            };
        }
        let cosine = self.normal.dot(d).abs();
        if cosine <= 0.0 { 0.0 } else { self.chance * distance * distance / (self.area * cosine) }
    }
}

/// For a sphere of `radius` whose center is `distance` away: one minus the cosine of the half
/// angle of the cone it fills, the share of a hemisphere it covers. None from inside it.
fn cone(distance: f32, radius: f32) -> Option<f32> {
    if distance.is_nan() || distance <= radius {
        return None;
    }
    let sin2 = (radius / distance).powi(2);
    let cos = (1.0 - sin2).max(0.0).sqrt();
    // 1 - cos as sin² / (1 + cos), which keeps its precision for small, far spheres.
    Some((sin2 / (1.0 + cos)).max(1e-12))
}

/// Two unit vectors at right angles to `n` and each other.
fn basis(n: Vec3) -> (Vec3, Vec3) {
    let helper = if n.x.abs() > 0.9 { Vec3::new(0.0, 1.0, 0.0) } else { Vec3::new(1.0, 0.0, 0.0) };
    let mut t = helper.cross(n);
    t *= 1.0 / t.length();
    (t, n.cross(t))
}

/// The radiance primitive `prim` gives off at `pos`, its map read as on a hit.
pub fn emission_at(scene: &Scene, prim: usize, pos: Vec3) -> Color {
    let p = &scene.prims[prim];
    let Some(emission) = scene.materials[p.material()].maps.emission else { return Color::default() };
    match emission.map.zip(scene.texture_uv_on(p, pos)) {
        Some((map, (u, v))) => emission.color * scene.textures[map].texel_fixed(u, v),
        None => emission.color,
    }
}

fn luminance(c: Color) -> f32 {
    (0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z).max(0.0)
}

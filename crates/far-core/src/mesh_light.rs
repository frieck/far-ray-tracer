//! Mesh lights: triangles whose material glows, sampled as lights in Enhanced mode. Without
//! them a glowing surface lights the scene only when a bounced ray happens to meet it, so a
//! small bright one, such as a candle flame, shows as scattered dots of light; sampled, each
//! shading point aims a shadow ray at one of them, drawn by the light it gives off.
//!
//! The light's units are those of panels: a triangle of emission `c` sends radiance `c` from
//! both of its sides, as a glowing surface always has in this program.

use crate::scene::{Primitive, Scene};
use crate::vector::{Color, Vec3};

/// One glowing triangle.
#[derive(Clone, Debug)]
pub struct MeshLight {
    /// Its index in `Scene::prims`.
    pub prim: usize,
    /// A corner and the two edges from it.
    pub p0: Vec3,
    pub e1: Vec3,
    pub e2: Vec3,
    /// The unit normal of its plane.
    pub normal: Vec3,
    pub area: f32,
    /// The mean radiance it gives off, its map averaged: what drawing it is weighed by.
    pub radiance: Color,
    /// The chance it is drawn, and the chances of it and the lights before it together.
    pub chance: f32,
    pub cumulative: f32,
}

/// Every glowing triangle of a scene, drawn by the light each gives off: its mean radiance
/// times its area.
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
            let Primitive::Triangle(t) = prim else { continue };
            // A displaced triangle lies off the shape its maps are read from.
            if scene.displaced_face(index).is_some() {
                continue;
            }
            let Some(emission) = scene.materials[t.material].maps.emission else { continue };
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
            let [a, b, c] = t.vertices.map(|v| scene.vertices[v as usize].pos);
            let (e1, e2) = (b - a, c - a);
            let mut normal = e1.cross(e2);
            let twice_area = normal.length();
            if twice_area <= 0.0 || !twice_area.is_finite() {
                continue;
            }
            normal *= 1.0 / twice_area;
            lights.push(MeshLight {
                prim: index, p0: a, e1, e2, normal, area: 0.5 * twice_area, radiance, chance: 0.0, cumulative: 0.0,
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
    /// A point spread evenly over the triangle.
    pub fn point(&self, u: f32, v: f32) -> Vec3 {
        let root = u.sqrt();
        self.p0 + self.e1 * (1.0 - v) * root + self.e2 * (v * root)
    }

    /// The density, over directions from a point `distance` away, of drawing this light and
    /// then a point on it seen at `cosine` to its plane.
    pub fn pdf(&self, distance: f32, cosine: f32) -> f32 {
        let cosine = cosine.abs();
        if cosine <= 0.0 { 0.0 } else { self.chance * distance * distance / (self.area * cosine) }
    }
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

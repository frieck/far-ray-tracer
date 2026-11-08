//! Bump and normal maps: surface detail that tilts the shading normal without changing the
//! geometry. Enhanced mode only; Classic and Fixed keep the original's smooth surfaces.
//!
//! The maps follow the same texture coordinates as the material's color texture, on every kind of
//! primitive. Instead of a tangent frame per shape, the direction in which the texture runs over
//! the surface is measured numerically: two small steps in the tangent plane show how the
//! texture coordinates change, and that gives both the height gradient and the tangent axes.

use crate::scene::{Primitive, Scene};
use crate::vector::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BumpKind {
    /// A grayscale height map; white is high. Strength is the depth, in scene units.
    #[default]
    Height,
    /// A tangent-space normal map, OpenGL convention (green points up in the image).
    /// Strength multiplies the tilt; 1 uses the map as drawn.
    Normal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bump {
    pub texture: usize,
    pub kind: BumpKind,
    pub strength: f32,
}

/// Length of the step that measures how texture coordinates run over the surface.
const STEP: f32 = 1e-3;

/// Orthonormal basis around `n` (Duff et al., 2017).
fn base(n: Vec3) -> (Vec3, Vec3) {
    let sign = 1.0f32.copysign(n.z);
    let a = -1.0 / (sign + n.z);
    let b = n.x * n.y * a;
    (
        Vec3::new(1.0 + sign * n.x * n.x * a, sign * b, -sign * n.x),
        Vec3::new(b, sign + n.y * n.y * a, -n.y),
    )
}

/// Change of texture coordinates per scene unit along `dir`. A step that crosses a seam, where
/// the coordinates jump, is measured backward instead.
fn rate(scene: &Scene, p: &Primitive, pos: Vec3, dir: Vec3, at: (f32, f32)) -> Option<(f32, f32)> {
    let forward = scene.texture_uv_on(p, pos + dir * STEP)?;
    let (du, dv) = (forward.0 - at.0, forward.1 - at.1);
    if du.abs() < 0.25 && dv.abs() < 0.25 {
        return Some((du / STEP, dv / STEP));
    }
    let backward = scene.texture_uv_on(p, pos - dir * STEP)?;
    let (du, dv) = (at.0 - backward.0, at.1 - backward.1);
    (du.abs() < 0.25 && dv.abs() < 0.25).then_some((du / STEP, dv / STEP))
}

/// The shading normal at `pos` of `p` with the material's bump or normal map applied. `normal`
/// is the unit normal the primitive gives there; it comes back unchanged where the map does not
/// apply.
pub fn bumped_normal(scene: &Scene, p: &Primitive, pos: Vec3, normal: Vec3) -> Vec3 {
    match scene.materials[p.material()].bump {
        Some(bump) => tilt(scene, p, pos, normal, bump),
        None => normal,
    }
}

/// `normal` tilted by `bump`, read through the texture coordinates of `p` at `pos`. A height map
/// with the displacement's height as strength gives the normal of the displaced surface.
pub fn tilt(scene: &Scene, p: &Primitive, pos: Vec3, normal: Vec3, bump: Bump) -> Vec3 {
    let Some(at) = scene.texture_uv_on(p, pos) else { return normal };
    let texture = &scene.textures[bump.texture];
    let (t1, t2) = base(normal);
    // A ground or box face is flat, so its steps stay on the surface; on curved shapes the step
    // is small enough that leaving the surface by STEP² does not change the coordinates.
    let (Some(d1), Some(d2)) = (rate(scene, p, pos, t1, at), rate(scene, p, pos, t2, at)) else {
        return normal;
    };
    let tilted = match bump.kind {
        BumpKind::Height => {
            // Height slope in texture space, one texel on each side.
            let (su, sv) = (1.0 / texture.width as f32, 1.0 / texture.height as f32);
            let height = |u: f32, v: f32| {
                let c = texture.texel_fixed(u, v);
                (c.x + c.y + c.z) / 3.0
            };
            let hu = (height(at.0 + su, at.1) - height(at.0 - su, at.1)) / (2.0 * su);
            let hv = (height(at.0, at.1 + sv) - height(at.0, at.1 - sv)) / (2.0 * sv);
            // The same slope in scene units along the two tangent directions.
            let g1 = bump.strength * (hu * d1.0 + hv * d1.1);
            let g2 = bump.strength * (hu * d2.0 + hv * d2.1);
            normal - (t1 * g1 + t2 * g2)
        }
        BumpKind::Normal => {
            // The tangent directions in which u and v grow: the inverse of how they change.
            let det = d1.0 * d2.1 - d2.0 * d1.1;
            if det.abs() < 1e-12 {
                return normal;
            }
            let along_u = (t1 * d2.1 - t2 * d1.1) * (1.0 / det);
            let along_v = (t2 * d1.0 - t1 * d2.0) * (1.0 / det);
            let (Some(tangent), Some(down)) = (unit(along_u), unit(along_v)) else { return normal };
            // Image rows grow with v, so up in the image, where green points, is against v.
            let c = texture.texel_fixed(at.0, at.1);
            let (x, y, z) = (2.0 * c.x - 1.0, 2.0 * c.y - 1.0, (2.0 * c.z - 1.0).max(0.0));
            tangent * (x * bump.strength) - down * (y * bump.strength) + normal * z
        }
    };
    let tilted = unit(tilted).unwrap_or(normal);
    // A strong map must not turn the surface away from where it faces.
    if tilted.dot(normal) > 0.05 { tilted } else { normal }
}

fn unit(v: Vec3) -> Option<Vec3> {
    let length = v.length();
    (length > 1e-12 && length.is_finite()).then(|| v * (1.0 / length))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Primitive;
    use crate::texture::Texture;
    use crate::vector::Color;

    /// A ground with one material whose bump map is `image`, `size` texels square.
    fn ground_with(kind: BumpKind, strength: f32, size: i32, image: impl Fn(i32, i32) -> Color) -> (Scene, usize) {
        let text = br#"{"lights":[],"objects":[{"type":"ground","material":{"color":[0.5,0.5,0.5]}}]}"#;
        let mut scene = Scene::from_far(text, std::path::Path::new("bump.far")).unwrap();
        let pixels = (0..size * size).map(|i| image(i % size, i / size)).collect();
        scene.textures.push(Texture { width: size, height: size, image: pixels });
        let prim = scene.prims.iter().position(|p| matches!(p, Primitive::Ground(_))).unwrap();
        let material = scene.prims[prim].material();
        scene.materials[material].bump = Some(Bump { texture: scene.textures.len() - 1, kind, strength });
        (scene, prim)
    }

    fn axes(scene: &Scene, prim: usize) -> (Vec3, Vec3, Vec3) {
        let Primitive::Ground(g) = &scene.prims[prim] else { unreachable!() };
        (g.u_axis, g.v_axis, g.normal)
    }

    #[test]
    fn height_and_normal_maps_lean_the_same_way() {
        let gray = |v: f32| Color::new(v, v, v);
        // Rising along u (image columns), then along v (image rows).
        for along_v in [false, true] {
            let (height, prim) = ground_with(BumpKind::Height, 0.5, 64, |x, y| gray((if along_v { y } else { x }) as f32 / 64.0));
            let (u, v, n) = axes(&height, prim);
            let axis = if along_v { v } else { u };
            let pos = n * 0.0 + u * 3.0 + v * 2.0;
            let bumped = bumped_normal(&height, &height.prims[prim], pos, n);
            assert!(bumped.dot(axis) < -0.01, "a slope rising along the axis leans the normal back: {}", bumped.dot(axis));
            // The same lean drawn as a normal map: against u is red below half; against v,
            // which is up in the image, is green above half.
            let color = if along_v { Color::new(0.5, 0.75, 0.93) } else { Color::new(0.25, 0.5, 0.93) };
            let (normal, prim) = ground_with(BumpKind::Normal, 1.0, 4, |_, _| color);
            let bumped = bumped_normal(&normal, &normal.prims[prim], pos, n);
            assert!(bumped.dot(axis) < -0.1, "the normal map leans the same way: {}", bumped.dot(axis));
            assert!((bumped.length() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn a_flat_map_or_no_map_keeps_the_normal() {
        let (scene, prim) = ground_with(BumpKind::Height, 1.0, 8, |_, _| Color::new(0.3, 0.3, 0.3));
        let (u, v, n) = axes(&scene, prim);
        let pos = u * 1.3 + v * 0.7;
        assert!((bumped_normal(&scene, &scene.prims[prim], pos, n) - n).length() < 1e-5);
        let (scene, prim) = ground_with(BumpKind::Normal, 1.0, 8, |_, _| Color::new(0.5, 0.5, 1.0));
        assert!((bumped_normal(&scene, &scene.prims[prim], pos, n) - n).length() < 1e-3);
    }
}

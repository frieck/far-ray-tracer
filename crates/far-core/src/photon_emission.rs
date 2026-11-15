//! A finite emission aperture for light arriving from an infinite environment.
use crate::{
    scene::{Primitive, Scene},
    vector::{Vec3, PI},
};

pub(crate) struct EnvironmentAperture {
    pub center: Vec3,
    pub radius: f32,
}
impl EnvironmentAperture {
    pub fn for_scene(scene: &Scene) -> Option<Self> {
        scene.environment.as_ref().filter(|e| e.active())?;
        let mut low = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut high = -low;
        let mut reflective = false;
        let mut grow = |p: Vec3| {
            low = Vec3::new(low.x.min(p.x), low.y.min(p.y), low.z.min(p.z));
            high = Vec3::new(high.x.max(p.x), high.y.max(p.y), high.z.max(p.z));
        };
        for primitive in &scene.prims {
            let material = &scene.materials[primitive.material()];
            let specular = material.refl > 0. || material.refr > 0.;
            match primitive {
                // An infinite reflector cannot be enclosed by this aperture.
                Primitive::Ground(_) if specular => return None,
                Primitive::Ground(_) => continue,
                Primitive::Sphere(s) => {
                    let r = Vec3::new(s.radius, s.radius, s.radius);
                    grow(s.center - r);
                    grow(s.center + r);
                }
                Primitive::Solid(s) => {
                    grow(s.low);
                    grow(s.high);
                }
                Primitive::Triangle(t) => {
                    for vertex in t.vertices {
                        grow(scene.vertices[vertex as usize].pos);
                    }
                }
            }
            reflective |= specular;
        }
        let center = (low + high) * 0.5;
        let radius = (high - center).length() * 1.001;
        (reflective && radius.is_finite() && radius > 0.).then_some(Self { center, radius })
    }
    pub fn area(&self) -> f32 {
        PI * self.radius * self.radius
    }
    pub fn origin(&self, toward_light: Vec3, u: f32, v: f32) -> Vec3 {
        let helper = if toward_light.z.abs() < 0.99 {
            Vec3::new(0., 0., 1.)
        } else {
            Vec3::new(1., 0., 0.)
        };
        let mut side = helper.cross(toward_light);
        side.normalize();
        let up = toward_light.cross(side);
        let r = self.radius * u.sqrt();
        let angle = 2. * PI * v;
        self.center
            + toward_light * (self.radius + 0.01)
            + side * (r * angle.cos())
            + up * (r * angle.sin())
    }
}

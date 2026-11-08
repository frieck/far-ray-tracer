//! Isotropic GGX reflection with Smith masking and Schlick Fresnel.
use crate::vector::{Color, Vec3, PI};

#[derive(Clone, Copy, Debug)]
pub struct Ggx { pub roughness: f32, pub metallic: f32 }

impl Ggx {
    fn alpha(self) -> f32 { (self.roughness * self.roughness).max(0.001) }
    pub fn fresnel(self, color: Color, ior: f32, cos: f32) -> Color {
        let dielectric = ((ior - 1.0) / (ior + 1.0)).powi(2);
        let f0 = Color::new(dielectric, dielectric, dielectric) * (1.0 - self.metallic) + color * self.metallic;
        f0 + (Color::new(1.0, 1.0, 1.0) - f0) * (1.0 - cos.clamp(0.0, 1.0)).powi(5)
    }
    fn g1(self, cos: f32) -> f32 {
        if cos <= 0.0 { return 0.0; }
        let a2 = self.alpha().powi(2);
        2.0 * cos / (cos + (a2 + (1.0 - a2) * cos * cos).sqrt())
    }
    fn distribution(self, cos: f32) -> f32 {
        // A shading normal a hair longer than 1 gives a cosine above 1, and the
        // denominator below would reach zero for smooth surfaces.
        let cos = cos.min(1.0);
        let a2 = self.alpha().powi(2);
        let denom = cos * cos * (a2 - 1.0) + 1.0;
        a2 / (PI * denom * denom)
    }
    fn diffuse(self, f: Color, color: Color, cos: f32) -> Color {
        (Color::new(1.0, 1.0, 1.0) - f) * color * ((1.0 - self.metallic) * cos / PI)
    }
    /// Only the diffuse lobe of `evaluate`, for light whose specular bounce photons carry.
    pub fn evaluate_diffuse(self, n: Vec3, wo: Vec3, wi: Vec3, color: Color, ior: f32) -> Color {
        let (no, ni) = (n.dot(wo), n.dot(wi));
        if no <= 0.0 || ni <= 0.0 { return Color::default(); }
        let mut h = wo + wi;
        if h.dot(h) < 1e-12 { return Color::default(); }
        h.normalize();
        self.diffuse(self.fresnel(color, ior, wo.dot(h)), color, ni)
    }
    /// BRDF times incoming cosine, evaluated for an area-light sample.
    pub fn evaluate(self, n: Vec3, wo: Vec3, wi: Vec3, color: Color, ior: f32, light_pdf: Option<f32>) -> Color {
        let (no, ni) = (n.dot(wo), n.dot(wi));
        if no <= 0.0 || ni <= 0.0 { return Color::default(); }
        let mut h = wo + wi;
        if h.dot(h) < 1e-12 { return Color::default(); }
        h.normalize();
        let f = self.fresnel(color, ior, wo.dot(h));
        let diffuse = self.diffuse(f, color, ni);
        let mis = light_pdf.map_or(1.0, |pdf| power_weight(pdf, self.pdf(n, wo, wi)));
        diffuse + f * (mis * self.distribution(n.dot(h).max(0.0)) * self.g1(no) * self.g1(ni) / (4.0 * no))
    }
    /// Density of `sample` in solid angle around `wi`.
    pub fn pdf(self, n: Vec3, wo: Vec3, wi: Vec3) -> f32 {
        let mut h = wo + wi;
        let no = n.dot(wo);
        if no <= 0.0 || n.dot(wi) <= 0.0 || h.dot(h) < 1e-12 { return 0.0; }
        h.normalize();
        self.distribution(n.dot(h).max(0.0)) * self.g1(no) / (4.0 * no)
    }
    /// Samples only the microfacets visible from `wo` (Heitz 2018). The weight stays
    /// at or below Fresnel; sampling all normals gives huge weights near silhouettes.
    pub fn sample(self, n: Vec3, wo: Vec3, u: f32, v: f32, color: Color, ior: f32) -> Option<(Vec3, Color)> {
        let no = n.dot(wo);
        if no <= 0.0 { return None; }
        let helper = if n.z.abs() < 0.999 { Vec3::new(0.0, 0.0, 1.0) } else { Vec3::new(1.0, 0.0, 0.0) };
        let mut t = helper.cross(n); t.normalize();
        let b = n.cross(t);
        let alpha = self.alpha();
        // Stretch the view so the rough surface becomes a hemisphere.
        let mut view = Vec3::new(alpha * wo.dot(t), alpha * wo.dot(b), no);
        view.normalize();
        let side = view.x * view.x + view.y * view.y;
        let t1 = if side > 1e-12 {
            Vec3::new(-view.y, view.x, 0.0) * (1.0 / side.sqrt())
        } else {
            Vec3::new(1.0, 0.0, 0.0)
        };
        let t2 = view.cross(t1);
        // Disk sample, squeezed toward the half that the view can see.
        let radius = u.clamp(0.0, 1.0).sqrt();
        let angle = 2.0 * PI * v;
        let p1 = radius * angle.cos();
        let visible = 0.5 * (1.0 + view.z);
        let p2 = (1.0 - visible) * (1.0 - p1 * p1).max(0.0).sqrt() + visible * radius * angle.sin();
        let normal = t1 * p1 + t2 * p2 + view * (1.0 - p1 * p1 - p2 * p2).max(0.0).sqrt();
        let mut h = t * (alpha * normal.x) + b * (alpha * normal.y) + n * normal.z.max(0.0);
        if h.dot(h) < 1e-12 { return None; }
        h.normalize();
        let oh = wo.dot(h);
        let wi = h * (2.0 * oh) - wo;
        let ni = n.dot(wi);
        if oh <= 0.0 || ni <= 0.0 { return None; }
        Some((wi, self.fresnel(color, ior, oh) * self.g1(ni)))
    }
}

pub fn power_weight(a: f32, b: f32) -> f32 {
    if a <= 0.0 { return 0.0; }
    let ratio = b / a;
    1.0 / (1.0 + ratio * ratio)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sampled_metal_matches_brdf_over_pdf() {
        let n = Vec3::new(0., 0., 1.);
        let mut wo = Vec3::new(0.6, 0., 0.8);
        wo.normalize();
        let color = Color::new(0.9, 0.6, 0.2);
        for roughness in [0.1, 0.3, 0.7, 1.] {
            let g = Ggx { roughness, metallic: 1. };
            for i in 1..100 {
                if let Some((wi, weight)) = g.sample(n, wo, i as f32 / 100., 0.37, color, 1.5) {
                    let expected = g.evaluate(n, wo, wi, color, 1.5, None) * (1. / g.pdf(n, wo, wi));
                    assert!((weight - expected).length() < 0.002);
                    assert!((wi.length() - 1.).abs() < 1e-5);
                }
            }
        }
    }
    #[test]
    fn reciprocal_and_finite_at_grazing_angles() {
        let n = Vec3::new(0., 0., 1.);
        let color = Color::new(0.8, 0.4, 0.1);
        for roughness in [0., 0.2, 1.] {
            for metallic in [0., 1.] {
                let g = Ggx { roughness, metallic };
                let mut wo = Vec3::new(1., 0., 0.0001); wo.normalize();
                let wi = Vec3::new(0., 0., 1.);
                let a = g.evaluate(n, wo, wi, color, 1.5, None);
                let b = g.evaluate(n, wi, wo, color, 1.5, None) * (1. / n.dot(wo));
                assert!(a.array().iter().all(|v| v.is_finite() && *v >= 0.));
                assert!((a-b).length() < 0.001);
                if let Some((_, weight)) = g.sample(n, wo, 1., 0.5, color, 1.5) {
                    assert!(weight.array().iter().all(|v| v.is_finite()));
                }
            }
        }
    }
    #[test]
    fn visible_normal_weights_stay_bounded_at_silhouettes() {
        let n = Vec3::new(0., 0., 1.);
        let white = Color::new(1., 1., 1.);
        for roughness in [0.05, 0.12, 0.35, 0.7] {
            let g = Ggx { roughness, metallic: 1. };
            for cos in [0.005f32, 0.05, 0.3, 1.] {
                let wo = Vec3::new((1. - cos * cos).sqrt(), 0., cos);
                for i in 0..2000 {
                    let (u, v) = ((i as f32 + 0.5) / 2000., (i as f32 * 0.618034).fract());
                    if let Some((_, weight)) = g.sample(n, wo, u, v, white, 1.5) {
                        assert!(weight.x <= 1.0 + 1e-4, "weight {} at cos {cos}", weight.x);
                    }
                }
            }
        }
    }
    #[test]
    fn slightly_long_normals_stay_finite() {
        // Seen in a render: this normal is 1.00055 long, so n·h exceeded 1.
        let n = Vec3::new(0.5596323, 0.27431774, 0.78274155);
        let wo = Vec3::new(0.7345084, 0.21041846, 0.64515245);
        let wi = Vec3::new(0.33455455, 0.3666621, 0.86811996);
        for roughness in [0.05, 0.12, 0.15, 0.5] {
            let g = Ggx { roughness, metallic: 1. };
            assert!(g.pdf(n, wo, wi).is_finite());
            let e = g.evaluate(n, wo, wi, Color::new(1., 1., 1.), 1.5, Some(4e5));
            assert!(e.array().iter().all(|v| v.is_finite()), "roughness {roughness}: {:?}", e.array());
        }
    }
    #[test]
    fn mis_weights_partition_energy() {
        assert!((power_weight(3., 7.) + power_weight(7., 3.) - 1.).abs() < 1e-6);
    }
}

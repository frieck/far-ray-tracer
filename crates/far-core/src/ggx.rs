//! GGX reflection with Smith masking and Schlick Fresnel, and the layers built on it:
//! anisotropic highlights, a clear coat over the base and a sheen at grazing angles.
use crate::vector::{Color, Vec3, PI};

#[derive(Clone, Copy, Debug)]
pub struct Ggx {
    pub roughness: f32,
    pub metallic: f32,
    /// From -1 to 1. Positive stretches highlights across the surface direction, as brushed
    /// metal does across its brushing; negative, along it.
    pub anisotropy: f32,
    /// Radians the surface direction turns around the normal.
    pub rotation: f32,
    /// Strength of a clear varnish layer over the base, from 0 to 1.
    pub clearcoat: f32,
    pub clearcoat_roughness: f32,
    /// Strength of the soft grazing glow of cloth, from 0 to 1.
    pub sheen: f32,
    pub sheen_color: Color,
    /// Surface direction for anisotropy at this point; without it, any direction is used.
    pub tangent: Option<Vec3>,
    /// Share of the diffuse light that travels under the surface instead, handled apart.
    pub subsurface: f32,
}

/// Reflectance of the clear coat at normal incidence: varnish, IOR 1.5.
const COAT_F0: f32 = 0.04;

impl Ggx {
    /// A plain GGX surface, with no anisotropy, coat or sheen.
    pub fn new(roughness: f32, metallic: f32) -> Self {
        Self {
            roughness, metallic, anisotropy: 0.0, rotation: 0.0, clearcoat: 0.0,
            clearcoat_roughness: 0.03, sheen: 0.0, sheen_color: Color::new(1.0, 1.0, 1.0), tangent: None,
            subsurface: 0.0,
        }
    }

    /// Widths of the highlight along the surface direction and across it.
    fn alphas(self) -> (f32, f32) {
        let a = (self.roughness * self.roughness).max(0.001);
        if self.anisotropy == 0.0 {
            return (a, a);
        }
        let aspect = (1.0 - 0.9 * self.anisotropy.abs()).sqrt();
        let (along, across) = ((a / aspect).max(0.001), (a * aspect).max(0.001));
        if self.anisotropy > 0.0 { (along, across) } else { (across, along) }
    }

    /// Tangent and bitangent around `n`, the tangent following the surface direction.
    fn frame(self, n: Vec3) -> (Vec3, Vec3) {
        let mut t = self.tangent.map(|t| t - n * n.dot(t)).unwrap_or_default();
        if t.dot(t) < 1e-8 {
            let helper = if n.z.abs() < 0.999 { Vec3::new(0.0, 0.0, 1.0) } else { Vec3::new(1.0, 0.0, 0.0) };
            t = helper.cross(n);
        }
        t.normalize();
        let b = n.cross(t);
        if self.rotation == 0.0 {
            return (t, b);
        }
        let (sin, cos) = self.rotation.sin_cos();
        let t = t * cos + b * sin;
        (t, n.cross(t))
    }

    pub fn fresnel(self, color: Color, ior: f32, cos: f32) -> Color {
        let dielectric = ((ior - 1.0) / (ior + 1.0)).powi(2);
        let f0 = Color::new(dielectric, dielectric, dielectric) * (1.0 - self.metallic) + color * self.metallic;
        f0 + (Color::new(1.0, 1.0, 1.0) - f0) * (1.0 - cos.clamp(0.0, 1.0)).powi(5)
    }

    fn coat_fresnel(cos: f32) -> f32 {
        COAT_F0 + (1.0 - COAT_F0) * (1.0 - cos.clamp(0.0, 1.0)).powi(5)
    }

    /// Share of light the clear coat leaves for the base, seen from `wo`.
    fn under_coat(self, n: Vec3, wo: Vec3) -> f32 {
        1.0 - self.clearcoat * Self::coat_fresnel(n.dot(wo))
    }

    /// Smith masking of one direction, `v` in the (t, b, n) frame.
    fn g1_local((ax, ay): (f32, f32), v: Vec3) -> f32 {
        if v.z <= 0.0 { return 0.0; }
        let slope = (v.x * v.x * ax * ax + v.y * v.y * ay * ay) / (v.z * v.z);
        2.0 / (1.0 + (1.0 + slope).sqrt())
    }

    /// Microfacet distribution of `h`, given in the (t, b, n) frame.
    fn distribution_local((ax, ay): (f32, f32), h: Vec3) -> f32 {
        // A shading normal a hair longer than 1 gives a cosine above 1, and the
        // denominator below would reach zero for smooth surfaces.
        let z = h.z.min(1.0);
        let d = h.x * h.x / (ax * ax) + h.y * h.y / (ay * ay) + z * z;
        1.0 / (PI * ax * ay * d * d)
    }

    fn local(t: Vec3, b: Vec3, n: Vec3, v: Vec3) -> Vec3 {
        Vec3::new(v.dot(t), v.dot(b), v.dot(n))
    }

    /// Masking of one direction by the base microfacets.
    pub fn g1(self, n: Vec3, v: Vec3) -> f32 {
        let (t, b) = self.frame(n);
        Self::g1_local(self.alphas(), Self::local(t, b, n, v))
    }

    fn diffuse(self, f: Color, color: Color, cos: f32) -> Color {
        (Color::new(1.0, 1.0, 1.0) - f) * color * ((1.0 - self.metallic) * (1.0 - self.subsurface) * cos / PI)
    }

    /// Soft glow at grazing angles, as on velvet, for light arriving along `wi`.
    fn sheen_term(self, wi: Vec3, h: Vec3, cos: f32) -> Color {
        if self.sheen <= 0.0 { return Color::default(); }
        self.sheen_color * (self.sheen * (1.0 - self.metallic) * (1.0 - wi.dot(h).clamp(0.0, 1.0)).powi(5) * cos)
    }

    /// Only the diffuse lobes of `evaluate`, sheen included, for light whose specular bounce
    /// photons carry.
    pub fn evaluate_diffuse(self, n: Vec3, wo: Vec3, wi: Vec3, color: Color, ior: f32) -> Color {
        let (no, ni) = (n.dot(wo), n.dot(wi));
        if no <= 0.0 || ni <= 0.0 { return Color::default(); }
        let mut h = wo + wi;
        if h.dot(h) < 1e-12 { return Color::default(); }
        h.normalize();
        (self.diffuse(self.fresnel(color, ior, wo.dot(h)), color, ni) + self.sheen_term(wi, h, ni)) * self.under_coat(n, wo)
    }

    /// Weight of a cosine-sampled diffuse bounce toward `wi`: the diffuse lobes over the
    /// sampling density.
    pub fn diffuse_weight(self, n: Vec3, wo: Vec3, wi: Vec3, color: Color, ior: f32) -> Color {
        let ni = n.dot(wi);
        if ni <= 0.0 { return Color::default(); }
        self.evaluate_diffuse(n, wo, wi, color, ior) * (PI / ni)
    }

    /// The specular part, base and coat, of BRDF times incoming cosine.
    fn specular(self, n: Vec3, wo: Vec3, wi: Vec3, h: Vec3, color: Color, ior: f32) -> Color {
        let (t, b) = self.frame(n);
        let (lo, li, lh) = (Self::local(t, b, n, wo), Self::local(t, b, n, wi), Self::local(t, b, n, h));
        let alphas = self.alphas();
        let f = self.fresnel(color, ior, wo.dot(h));
        let base = f * (Self::distribution_local(alphas, lh) * Self::g1_local(alphas, lo) * Self::g1_local(alphas, li)
            / (4.0 * lo.z)) * self.under_coat(n, wo);
        if self.clearcoat <= 0.0 {
            return base;
        }
        let coat = self.coat();
        let ca = coat.alphas();
        let coat_term = self.clearcoat * Self::coat_fresnel(wo.dot(h)) * Self::distribution_local(ca, lh)
            * Self::g1_local(ca, lo) * Self::g1_local(ca, li) / (4.0 * lo.z);
        base + Color::new(coat_term, coat_term, coat_term)
    }

    /// The clear coat as a smooth isotropic surface of its own.
    fn coat(self) -> Self {
        Self { roughness: self.clearcoat_roughness, anisotropy: 0.0, ..self }
    }

    /// BRDF times incoming cosine, evaluated for an area-light sample.
    pub fn evaluate(self, n: Vec3, wo: Vec3, wi: Vec3, color: Color, ior: f32, light_pdf: Option<f32>) -> Color {
        let (no, ni) = (n.dot(wo), n.dot(wi));
        if no <= 0.0 || ni <= 0.0 { return Color::default(); }
        let mut h = wo + wi;
        if h.dot(h) < 1e-12 { return Color::default(); }
        h.normalize();
        let diffuse = (self.diffuse(self.fresnel(color, ior, wo.dot(h)), color, ni) + self.sheen_term(wi, h, ni))
            * self.under_coat(n, wo);
        let mis = light_pdf.map_or(1.0, |pdf| power_weight(pdf, self.pdf(n, wo, wi, color, ior)));
        diffuse + self.specular(n, wo, wi, h, color, ior) * mis
    }

    /// Chance that `sample` picks the clear coat rather than the base, seen from `wo`.
    fn coat_chance(self, n: Vec3, wo: Vec3, color: Color, ior: f32) -> f32 {
        if self.clearcoat <= 0.0 { return 0.0; }
        let cos = n.dot(wo);
        let coat = self.clearcoat * Self::coat_fresnel(cos);
        let f = self.fresnel(color, ior, cos);
        let base = f.x.max(f.y).max(f.z) * (1.0 - coat);
        if coat + base <= 0.0 { 0.0 } else { (coat / (coat + base)).clamp(0.05, 0.95) }
    }

    /// Density of visible-normal sampling of one lobe toward `wi`.
    fn lobe_pdf(self, n: Vec3, wo: Vec3, wi: Vec3) -> f32 {
        let mut h = wo + wi;
        let no = n.dot(wo);
        if no <= 0.0 || n.dot(wi) <= 0.0 || h.dot(h) < 1e-12 { return 0.0; }
        h.normalize();
        let (t, b) = self.frame(n);
        let alphas = self.alphas();
        Self::distribution_local(alphas, Self::local(t, b, n, h)) * Self::g1_local(alphas, Self::local(t, b, n, wo)) / (4.0 * no)
    }

    /// Density of `sample` in solid angle around `wi`, coat and base together.
    pub fn pdf(self, n: Vec3, wo: Vec3, wi: Vec3, color: Color, ior: f32) -> f32 {
        self.mixed_pdf(n, wo, wi, self.coat_chance(n, wo, color, ior))
    }

    fn mixed_pdf(self, n: Vec3, wo: Vec3, wi: Vec3, coat_chance: f32) -> f32 {
        let base = self.lobe_pdf(n, wo, wi);
        if coat_chance <= 0.0 { return base; }
        coat_chance * self.coat().lobe_pdf(n, wo, wi) + (1.0 - coat_chance) * base
    }

    /// A microfacet normal visible from `wo` (Heitz 2018), for reflection or refraction.
    pub fn sample_normal(self, n: Vec3, wo: Vec3, u: f32, v: f32) -> Option<Vec3> {
        let no = n.dot(wo);
        if no <= 0.0 { return None; }
        let (t, b) = self.frame(n);
        let (ax, ay) = self.alphas();
        // Stretch the view so the rough surface becomes a hemisphere.
        let mut view = Vec3::new(ax * wo.dot(t), ay * wo.dot(b), no);
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
        let mut h = t * (ax * normal.x) + b * (ay * normal.y) + n * normal.z.max(0.0);
        if h.dot(h) < 1e-12 { return None; }
        h.normalize();
        Some(h)
    }

    /// Samples a specular bounce: the clear coat or the base, picked by `pick`, each by its
    /// visible microfacets. The weight stays at or below Fresnel over the chance of the lobe;
    /// sampling all normals gives huge weights near silhouettes.
    pub fn sample(self, n: Vec3, wo: Vec3, u: f32, v: f32, pick: f32, color: Color, ior: f32) -> Option<(Vec3, Color)> {
        let coat_chance = self.coat_chance(n, wo, color, ior);
        let on_coat = pick < coat_chance;
        let lobe = if on_coat { self.coat() } else { self };
        let h = lobe.sample_normal(n, wo, u, v)?;
        let oh = wo.dot(h);
        let wi = h * (2.0 * oh) - wo;
        if oh <= 0.0 || n.dot(wi) <= 0.0 { return None; }
        let g1 = lobe.g1(n, wi);
        if on_coat {
            let w = self.clearcoat * Self::coat_fresnel(oh) * g1 / coat_chance;
            Some((wi, Color::new(w, w, w)))
        } else {
            Some((wi, self.fresnel(color, ior, oh) * (g1 * self.under_coat(n, wo) / (1.0 - coat_chance))))
        }
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
            let g = Ggx::new(roughness, 1.);
            for i in 1..100 {
                if let Some((wi, weight)) = g.sample(n, wo, i as f32 / 100., 0.37, 0.5, color, 1.5) {
                    let expected = g.evaluate(n, wo, wi, color, 1.5, None) * (1. / g.pdf(n, wo, wi, color, 1.5));
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
                let g = Ggx::new(roughness, metallic);
                let mut wo = Vec3::new(1., 0., 0.0001); wo.normalize();
                let wi = Vec3::new(0., 0., 1.);
                let a = g.evaluate(n, wo, wi, color, 1.5, None);
                let b = g.evaluate(n, wi, wo, color, 1.5, None) * (1. / n.dot(wo));
                assert!(a.array().iter().all(|v| v.is_finite() && *v >= 0.));
                assert!((a-b).length() < 0.001);
                if let Some((_, weight)) = g.sample(n, wo, 1., 0.5, 0.5, color, 1.5) {
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
            let g = Ggx::new(roughness, 1.);
            for cos in [0.005f32, 0.05, 0.3, 1.] {
                let wo = Vec3::new((1. - cos * cos).sqrt(), 0., cos);
                for i in 0..2000 {
                    let (u, v) = ((i as f32 + 0.5) / 2000., (i as f32 * 0.618034).fract());
                    if let Some((_, weight)) = g.sample(n, wo, u, v, 0.5, white, 1.5) {
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
            let g = Ggx::new(roughness, 1.);
            assert!(g.pdf(n, wo, wi, Color::new(1., 1., 1.), 1.5).is_finite());
            let e = g.evaluate(n, wo, wi, Color::new(1., 1., 1.), 1.5, Some(4e5));
            assert!(e.array().iter().all(|v| v.is_finite()), "roughness {roughness}: {:?}", e.array());
        }
    }
    #[test]
    fn anisotropic_samples_match_brdf_over_pdf() {
        let n = Vec3::new(0., 0., 1.);
        let mut wo = Vec3::new(0.5, 0.3, 0.8);
        wo.normalize();
        let color = Color::new(0.9, 0.6, 0.2);
        for anisotropy in [-0.8, 0.5, 1.0] {
            let g = Ggx { anisotropy, rotation: 0.4, tangent: Some(Vec3::new(1., 0., 0.)), ..Ggx::new(0.4, 1.) };
            for i in 1..100 {
                if let Some((wi, weight)) = g.sample(n, wo, i as f32 / 100., 0.61, 0.5, color, 1.5) {
                    let expected = g.evaluate(n, wo, wi, color, 1.5, None) * (1. / g.pdf(n, wo, wi, color, 1.5));
                    assert!((weight - expected).length() < 0.003, "anisotropy {anisotropy}");
                }
            }
        }
    }

    /// Specular light reflected toward `wo`, by sampling and by integrating the BRDF over
    /// the hemisphere: they must agree with a clear coat too.
    #[test]
    fn coated_sampling_returns_the_specular_energy_of_the_brdf() {
        let n = Vec3::new(0., 0., 1.);
        let mut wo = Vec3::new(0.4, 0., 0.9);
        wo.normalize();
        let color = Color::new(0.7, 0.1, 0.1);
        let g = Ggx { clearcoat: 1.0, clearcoat_roughness: 0.3, ..Ggx::new(0.5, 0.) };
        let count = 200_000;
        let mut sampled = 0.0;
        for i in 0..count {
            let u = (i as f32 + 0.5) / count as f32;
            let (v, pick) = ((i as f32 * 0.618034).fract(), (i as f32 * 0.754877).fract());
            if let Some((_, weight)) = g.sample(n, wo, u, v, pick, color, 1.5) {
                sampled += weight.y / count as f32;
            }
        }
        // The specular part is the whole response minus the diffuse lobes.
        let steps = 400;
        let mut integrated = 0.0;
        for i in 0..steps {
            for j in 0..steps {
                let cos = (i as f32 + 0.5) / steps as f32;
                let angle = 2.0 * PI * (j as f32 + 0.5) / steps as f32;
                let sin = (1.0 - cos * cos).sqrt();
                let wi = Vec3::new(sin * angle.cos(), sin * angle.sin(), cos);
                let spec = g.evaluate(n, wo, wi, color, 1.5, None) - g.evaluate_diffuse(n, wo, wi, color, 1.5);
                integrated += spec.y * 2.0 * PI / (steps * steps) as f32;
            }
        }
        assert!((sampled - integrated).abs() < 0.01, "sampled {sampled}, integrated {integrated}");
    }

    #[test]
    fn mis_weights_partition_energy() {
        assert!((power_weight(3., 7.) + power_weight(7., 3.) - 1.).abs() < 1e-6);
    }
}

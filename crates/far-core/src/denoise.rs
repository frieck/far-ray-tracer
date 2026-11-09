//! Removes the remaining Monte Carlo noise from an Enhanced image.
//!
//! The filter is non-local means guided by what the passes measured. Each pixel keeps
//! its own variance, so only differences larger than the noise count as real detail.
//! The first surface each pixel saw (color, normal and depth) stops the filter at
//! object edges and textures, even where the image is too noisy to show them.

use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

/// Fewer passes than this give no usable variance estimate.
pub const MIN_PASSES: u32 = 4;
const WINDOW: i32 = 6;
const PATCH: i32 = 1;
/// How many standard deviations of noise a difference may have and still be averaged.
const STRENGTH: f32 = 0.7;
const COLOR_SIGMA2: f32 = 0.01;
const NORMAL_SIGMA2: f32 = 0.04;
const DEPTH_SIGMA2: f32 = 0.01;
/// Keeps flat areas with no measured variance from dividing by zero.
const MIN_DENOMINATOR: f32 = 1e-5;
/// A pixel is a firefly only if its noise is this many times that of its neighbors.
const FIREFLY_VARIANCE: f32 = 4.0;

const LUMA2: usize = 0;
const ALBEDO: usize = 1;
const NORMAL: usize = 4;
const DEPTH: usize = 7;
const FIELDS: usize = 8;

/// What the first hit of a sample saw. Misses keep the zero default.
#[derive(Clone, Copy, Default)]
pub struct Primary {
    pub albedo: [f32; 3],
    pub normal: [f32; 3],
    pub depth: f32,
}

/// Per-pixel sums over the passes. Each pixel belongs to one tile, and so to one
/// thread at a time, which makes the separate load and store safe.
pub struct Guide {
    sums: Vec<AtomicU32>,
}

impl Guide {
    pub fn new(pixels: usize) -> Self {
        Self { sums: (0..pixels * FIELDS).map(|_| AtomicU32::new(0)).collect() }
    }

    fn add(&self, index: usize, value: f32) {
        let cell = &self.sums[index];
        cell.store((f32::from_bits(cell.load(Relaxed)) + value).to_bits(), Relaxed);
    }

    fn get(&self, index: usize) -> f32 {
        f32::from_bits(self.sums[index].load(Relaxed))
    }

    pub fn record(&self, pixel: usize, luminance: f32, primary: &Primary) {
        let base = pixel * FIELDS;
        self.add(base + LUMA2, luminance * luminance);
        for k in 0..3 {
            self.add(base + ALBEDO + k, primary.albedo[k]);
            self.add(base + NORMAL + k, primary.normal[k]);
        }
        self.add(base + DEPTH, primary.depth);
    }

    /// The sums of a rectangle of an image `width` pixels wide, as a guide of its own.
    pub fn crop(&self, width: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> Self {
        let mut sums = Vec::with_capacity((x1 - x0) * (y1 - y0) * FIELDS);
        for y in y0..y1 {
            for x in x0..x1 {
                let base = (y * width + x) * FIELDS;
                sums.extend((0..FIELDS).map(|k| AtomicU32::new(self.sums[base + k].load(Relaxed))));
            }
        }
        Self { sums }
    }

    /// Mean of the squared luminance over `count` samples.
    pub fn mean_luminance2(&self, pixel: usize, count: u32) -> f32 {
        self.get(pixel * FIELDS + LUMA2) / count as f32
    }

    /// Mean albedo and normal of a pixel's first hits.
    pub fn albedo_normal(&self, pixel: usize, count: u32) -> ([f32; 3], [f32; 3]) {
        let f = self.feature(pixel, count);
        (f.albedo, f.normal)
    }

    fn feature(&self, pixel: usize, count: u32) -> Feature {
        let base = pixel * FIELDS;
        let scale = 1.0 / count as f32;
        let vector = |at: usize| [0, 1, 2].map(|k| self.get(base + at + k) * scale);
        Feature { albedo: vector(ALBEDO), normal: vector(NORMAL), depth: self.get(base + DEPTH) * scale }
    }
}

#[derive(Clone, Copy)]
struct Feature {
    albedo: [f32; 3],
    normal: [f32; 3],
    depth: f32,
}

fn squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum()
}

pub fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn similarity(a: &Feature, b: &Feature) -> f32 {
    let depth = (a.depth - b.depth) / a.depth.max(b.depth).max(1e-3);
    (-squared(a.albedo, b.albedo) / COLOR_SIGMA2
        - squared(a.normal, b.normal) / NORMAL_SIGMA2
        - depth * depth / DEPTH_SIGMA2).exp()
}

/// Replaces isolated pixels that are far brighter than their neighbors and whose
/// brightness came from a few samples. Sharp highlights have low variance and stay.
fn remove_fireflies(color: &mut [[f32; 3]], variance: &[f32], width: usize, height: usize) {
    let source = color.to_vec();
    for y in 1..height.saturating_sub(1) {
        for x in 1..width.saturating_sub(1) {
            let mut around: Vec<(f32, usize, f32)> = (0..9).map(|i| {
                let q = (y + i / 3 - 1) * width + x + i % 3 - 1;
                (luma(source[q]), q, variance[q])
            }).collect();
            around.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (median, q, _) = around[4];
            let mut noise: Vec<f32> = around.iter().map(|v| v.2).collect();
            noise.sort_by(f32::total_cmp);
            let p = y * width + x;
            let spread = 4.0 * noise[4].sqrt() + 0.02;
            if luma(source[p]) > median + spread && variance[p] > FIREFLY_VARIANCE * noise[4] {
                color[p] = source[q];
            }
        }
    }
}

/// Averages each pixel over a 3x3 block, because one pixel's estimate is itself noisy.
fn smooth(values: &[f32], width: usize, height: usize) -> Vec<f32> {
    (0..values.len()).map(|p| {
        let (x, y) = ((p % width) as i32, (p / width) as i32);
        let (mut sum, mut count) = (0.0, 0.0);
        for j in (y - 1).max(0)..=(y + 1).min(height as i32 - 1) {
            for i in (x - 1).max(0)..=(x + 1).min(width as i32 - 1) {
                sum += values[j as usize * width + i as usize];
                count += 1.0;
            }
        }
        sum / count
    }).collect()
}

/// Filters display-referred colors. `variance` is the variance of each pixel's mean.
/// Returns None if `cancel` is set while it runs.
pub fn denoise(
    mut color: Vec<[f32; 3]>,
    variance: Vec<f32>,
    guide: &Guide,
    counts: &[u32],
    width: usize,
    height: usize,
    cancel: &AtomicBool,
) -> Option<Vec<[f32; 3]>> {
    remove_fireflies(&mut color, &variance, width, height);
    let variance = smooth(&variance, width, height);
    let features: Vec<Feature> = (0..color.len()).map(|p| guide.feature(p, counts[p].max(1))).collect();
    let (w, h) = (width as i32, height as i32);
    let at = |x: i32, y: i32| y.clamp(0, h - 1) as usize * width + x.clamp(0, w - 1) as usize;
    let cells = ((2 * PATCH + 1) * (2 * PATCH + 1) * 3) as f32;

    let rows: Vec<Option<Vec<[f32; 3]>>> = (0..h).into_par_iter().map(|y| {
        if cancel.load(Relaxed) { return None; }
        Some((0..w).map(|x| {
            let p = at(x, y);
            let (mut sum, mut total) = ([0.0f32; 3], 0.0f32);
            for dy in -WINDOW..=WINDOW {
                for dx in -WINDOW..=WINDOW {
                    let (qx, qy) = (x + dx, y + dy);
                    if qx < 0 || qy < 0 || qx >= w || qy >= h { continue; }
                    let q = at(qx, qy);
                    let guided = similarity(&features[p], &features[q]);
                    if guided < 1e-4 { continue; }
                    let (vp, vq) = (variance[p], variance[q]);
                    let mut distance = 0.0;
                    for j in -PATCH..=PATCH {
                        for i in -PATCH..=PATCH {
                            let (a, b) = (at(x + i, y + j), at(qx + i, qy + j));
                            let noise = variance[a] + variance[a].min(variance[b]);
                            distance += (0..3)
                                .map(|k| (color[a][k] - color[b][k]).powi(2) - noise)
                                .sum::<f32>();
                        }
                    }
                    let scale = MIN_DENOMINATOR + STRENGTH * STRENGTH * (vp + vq);
                    let weight = guided * (-(distance / cells).max(0.0) / scale).exp();
                    total += weight;
                    for k in 0..3 { sum[k] += weight * color[q][k]; }
                }
            }
            sum.map(|v| v / total.max(1e-12))
        }).collect())
    }).collect();
    let mut result = Vec::with_capacity(color.len());
    for row in rows { result.extend(row?); }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: usize = 48;

    fn flat_guide(split: Option<usize>) -> Guide {
        let guide = Guide::new(SIZE * SIZE);
        for p in 0..SIZE * SIZE {
            let dark = split.is_some_and(|s| p % SIZE >= s);
            let albedo = if dark { [0.1; 3] } else { [0.8; 3] };
            guide.record(p, 0.0, &Primary { albedo, normal: [0.0, 1.0, 0.0], depth: 10.0 });
        }
        guide
    }

    fn noise(p: usize) -> f32 {
        let mut x = (p as u32).wrapping_mul(2654435761).wrapping_add(12345);
        x ^= x >> 15;
        x = x.wrapping_mul(2246822519);
        x ^= x >> 13;
        (x & 0xffff) as f32 / 65535.0 - 0.5
    }

    fn run(color: Vec<[f32; 3]>, variance: f32, guide: &Guide) -> Vec<[f32; 3]> {
        denoise(color, vec![variance; SIZE * SIZE], guide, &[1; SIZE * SIZE], SIZE, SIZE, &AtomicBool::new(false)).unwrap()
    }

    fn rmse(image: &[[f32; 3]], expected: impl Fn(usize) -> f32) -> f32 {
        let sum: f32 = image.iter().enumerate().map(|(p, c)| (c[0] - expected(p)).powi(2)).sum();
        (sum / image.len() as f32).sqrt()
    }

    #[test]
    fn flat_noise_goes_away_and_brightness_stays() {
        let noisy: Vec<_> = (0..SIZE * SIZE).map(|p| [0.5 + 0.2 * noise(p); 3]).collect();
        let before = rmse(&noisy, |_| 0.5);
        let after = run(noisy, 0.04 * 0.083, &flat_guide(None));
        assert!(rmse(&after, |_| 0.5) < before * 0.25);
        let mean = after.iter().map(|c| c[0]).sum::<f32>() / after.len() as f32;
        assert!((mean - 0.5).abs() < 0.01, "mean {mean}");
    }

    #[test]
    fn surface_edges_survive_noise_as_strong_as_the_edge() {
        let truth = |p: usize| if p % SIZE < 24 { 0.6 } else { 0.4 };
        let noisy: Vec<_> = (0..SIZE * SIZE).map(|p| [truth(p) + 0.4 * noise(p); 3]).collect();
        let after = run(noisy, 0.16 * 0.083, &flat_guide(Some(24)));
        for y in 0..SIZE {
            assert!(after[y * SIZE + 23][0] > after[y * SIZE + 24][0]);
        }
        assert!(rmse(&after, truth) < 0.04);
    }

    #[test]
    fn low_noise_detail_with_no_feature_is_kept() {
        // A reflection: no surface change marks it, only the measured variance protects it.
        let image: Vec<_> = (0..SIZE * SIZE).map(|p| [if p % SIZE < 24 { 0.7 } else { 0.3 }; 3]).collect();
        let after = run(image.clone(), 1e-6, &flat_guide(None));
        assert!(rmse(&after, |p| image[p][0]) < 0.002);
    }

    #[test]
    fn fireflies_are_removed_but_sharp_low_noise_highlights_stay() {
        let mut color = vec![[0.3; 3]; SIZE * SIZE];
        let mut variance = vec![1e-4; SIZE * SIZE];
        color[10 * SIZE + 10] = [1.0; 3];
        variance[10 * SIZE + 10] = 0.05;
        color[30 * SIZE + 30] = [1.0; 3];
        remove_fireflies(&mut color, &variance, SIZE, SIZE);
        assert_eq!(color[10 * SIZE + 10], [0.3; 3]);
        assert_eq!(color[30 * SIZE + 30], [1.0; 3]);
    }

    #[test]
    fn cancelling_returns_nothing() {
        let color = vec![[0.5; 3]; SIZE * SIZE];
        let cancel = AtomicBool::new(true);
        assert!(denoise(color, vec![0.0; SIZE * SIZE], &flat_guide(None), &[1; SIZE * SIZE], SIZE, SIZE, &cancel).is_none());
    }
}

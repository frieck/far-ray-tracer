//! Caustic photon map: light that reached a diffuse surface through glass or a mirror.
//!
//! Tracing from the camera cannot find these paths, because a ray would have to pass
//! through the glass and land on the light by chance. So photons are traced from the
//! lights first (Jensen, 1996), and the render reads their density at each surface.

use crate::vector::{Color, Vec3, PI};

#[derive(Clone, Copy)]
pub struct Photon {
    pub pos: Vec3,
    pub normal: Vec3,
    pub power: Color,
}

/// Photons sorted into a hashed grid, so a lookup only reads nearby ones.
pub struct PhotonMap {
    photons: Vec<Photon>,
    /// `bucket_start[b]..bucket_start[b + 1]` are the photons of bucket `b`.
    bucket_start: Vec<u32>,
    radius: f32,
    cell: f32,
}

fn cell_of(v: f32, cell: f32) -> i32 {
    (v / cell).floor() as i32
}

fn bucket_of(x: i32, y: i32, z: i32, buckets: usize) -> usize {
    let hash = (x as u32).wrapping_mul(73_856_093)
        ^ (y as u32).wrapping_mul(19_349_663)
        ^ (z as u32).wrapping_mul(83_492_791);
    hash as usize & (buckets - 1)
}

impl PhotonMap {
    pub fn new(mut photons: Vec<Photon>, radius: f32) -> Self {
        // With cells twice the radius, a lookup touches at most two cells per axis.
        let cell = 2.0 * radius;
        let buckets = photons.len().next_power_of_two().max(1024);
        let bucket = |p: &Photon| {
            let (x, y, z) = (cell_of(p.pos.x, cell), cell_of(p.pos.y, cell), cell_of(p.pos.z, cell));
            bucket_of(x, y, z, buckets)
        };
        photons.sort_by_key(bucket);
        let mut bucket_start = vec![0u32; buckets + 1];
        for p in &photons {
            bucket_start[bucket(p) + 1] += 1;
        }
        for i in 0..buckets {
            bucket_start[i + 1] += bucket_start[i];
        }
        Self { photons, bucket_start, radius, cell }
    }

    pub fn len(&self) -> usize {
        self.photons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.photons.is_empty()
    }

    pub fn memory_bytes(&self) -> usize {
        self.photons.len() * std::mem::size_of::<Photon>() + self.bucket_start.len() * 4
    }

    /// Light arriving at a point, estimated from nearby photons with a cone filter.
    /// Photons on another surface are skipped, so light does not leak around corners.
    /// Also returns how many photons were used: few of them means a noisy estimate.
    pub fn irradiance(&self, pos: Vec3, normal: Vec3) -> (Color, u32) {
        if self.photons.is_empty() {
            return (Color::default(), 0);
        }
        let (r, cell) = (self.radius, self.cell);
        let buckets = self.bucket_start.len() - 1;
        let low = [pos.x - r, pos.y - r, pos.z - r].map(|v| cell_of(v, cell));
        let high = [pos.x + r, pos.y + r, pos.z + r].map(|v| cell_of(v, cell));
        let r2 = r * r;
        let mut sum = Color::default();
        let mut found = 0;
        for x in low[0]..=high[0] {
            for y in low[1]..=high[1] {
                for z in low[2]..=high[2] {
                    let b = bucket_of(x, y, z, buckets);
                    let range = self.bucket_start[b] as usize..self.bucket_start[b + 1] as usize;
                    for p in &self.photons[range] {
                        let offset = p.pos - pos;
                        let d2 = offset.dot(offset);
                        if d2 >= r2 || p.normal.dot(normal) <= 0.7 || offset.dot(normal).abs() >= 0.5 * r {
                            continue;
                        }
                        // Different cells can share a bucket; each photon counts once.
                        // This check is the most expensive one, so it runs last.
                        if cell_of(p.pos.x, cell) == x && cell_of(p.pos.y, cell) == y && cell_of(p.pos.z, cell) == z {
                            sum += p.power * (1.0 - d2 / r2);
                            found += 1;
                        }
                    }
                }
            }
        }
        // The cone integrates to half a disk, hence the factor of 2.
        (sum * (2.0 / (PI * r2)), found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_photons(spacing: f32, power: f32) -> Vec<Photon> {
        let n = (4.0 / spacing) as i32;
        let mut photons = Vec::new();
        for i in 0..n {
            for j in 0..n {
                photons.push(Photon {
                    pos: Vec3::new(i as f32 * spacing, 0.0, j as f32 * spacing),
                    normal: Vec3::new(0.0, 1.0, 0.0),
                    power: Color::new(power, power, power),
                });
            }
        }
        photons
    }

    #[test]
    fn uniform_photons_give_their_density() {
        let (spacing, power) = (0.02, 0.5);
        let map = PhotonMap::new(flat_photons(spacing, power), 0.3);
        let (e, found) = map.irradiance(Vec3::new(2.0, 0.0, 2.0), Vec3::new(0.0, 1.0, 0.0));
        let in_disk = PI * 0.3 * 0.3 / (spacing * spacing);
        assert!((found as f32 / in_disk - 1.0).abs() < 0.02);
        let expected = power / (spacing * spacing);
        assert!((e.x / expected - 1.0).abs() < 0.02, "{} and {expected}", e.x);
    }

    #[test]
    fn photons_on_another_surface_do_not_count() {
        let map = PhotonMap::new(flat_photons(0.02, 1.0), 0.3);
        let (facing_away, _) = map.irradiance(Vec3::new(2.0, 0.0, 2.0), Vec3::new(1.0, 0.0, 0.0));
        let (above, _) = map.irradiance(Vec3::new(2.0, 0.25, 2.0), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(facing_away, Color::default());
        assert_eq!(above, Color::default());
    }

    #[test]
    fn an_empty_map_gives_nothing() {
        let map = PhotonMap::new(Vec::new(), 0.1);
        assert_eq!(map.irradiance(Vec3::default(), Vec3::new(0.0, 1.0, 0.0)).0, Color::default());
    }
}

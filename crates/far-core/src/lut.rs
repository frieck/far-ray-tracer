//! Color looks from a LUT, a lookup table in the `.cube` format that grading programs such as
//! DaVinci Resolve, Premiere, Photoshop and V-Ray's frame buffer read and write.
//!
//! The table maps the screen colors of the finished picture, after tone mapping, to new ones,
//! as a colorist's grade does to a scanned film. It is the last thing a render's display does,
//! after film grain, so it changes without rendering again; a saved EXR keeps the light
//! without it. Most looks are made for such sRGB or Rec. 709 screen values; a LUT made for log
//! footage expects other values and shows washed out or too strong.
//!
//! A 3D table is read between its points by tetrahedral interpolation, as Resolve does, which
//! keeps greys grey; a 1D table changes each channel by its own curve.

use crate::engine::{Image, Region};
use std::sync::atomic::Ordering::Relaxed;

/// The largest tables the format allows.
const MAX_3D_SIZE: usize = 256;
const MAX_1D_SIZE: usize = 65536;

#[derive(Clone, Debug, PartialEq)]
pub struct Lut {
    /// The title the file gives, if any.
    pub title: Option<String>,
    /// Points along each axis.
    size: usize,
    three_d: bool,
    /// Red changes fastest, then green, then blue, as the file lists them.
    table: Vec<[f32; 3]>,
    /// The input values the first and last points stand for, per channel.
    min: [f32; 3],
    max: [f32; 3],
}

impl Lut {
    /// Reads a `.cube` file.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("could not read the LUT {}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Reads the text of a `.cube` file: keywords, then one line of three numbers per point.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut title = None;
        let (mut size_3d, mut size_1d) = (None, None);
        let (mut min, mut max) = ([0.0f32; 3], [1.0f32; 3]);
        let mut table = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let at = |message: &str| format!("line {}: {message}", number + 1);
            let mut words = line.split_whitespace();
            let first = words.next().unwrap_or("");
            let numbers = |words: std::str::SplitWhitespace| -> Result<Vec<f32>, String> {
                words.map(|w| w.parse::<f32>().ok().filter(|v| v.is_finite()).ok_or_else(|| at(&format!("{w} is not a number"))))
                    .collect()
            };
            let size = |words: std::str::SplitWhitespace, largest: usize| -> Result<usize, String> {
                let n = numbers(words)?;
                match n.as_slice() {
                    [n] if n.fract() == 0.0 && *n >= 2.0 && *n <= largest as f32 => Ok(*n as usize),
                    _ => Err(at(&format!("the size must be a whole number from 2 to {largest}"))),
                }
            };
            match first {
                "TITLE" => title = Some(line["TITLE".len()..].trim().trim_matches('"').to_string()),
                "LUT_3D_SIZE" => size_3d = Some(size(words, MAX_3D_SIZE)?),
                "LUT_1D_SIZE" => size_1d = Some(size(words, MAX_1D_SIZE)?),
                "DOMAIN_MIN" | "DOMAIN_MAX" => {
                    let n = numbers(words)?;
                    let n: [f32; 3] = n.try_into().map_err(|_| at("the domain needs three numbers"))?;
                    if first == "DOMAIN_MIN" { min = n } else { max = n }
                }
                // Resolve's older way of giving the domain, the same for all three channels.
                "LUT_1D_INPUT_RANGE" | "LUT_3D_INPUT_RANGE" => {
                    let n = numbers(words)?;
                    let [low, high]: [f32; 2] = n.try_into().map_err(|_| at("the input range needs two numbers"))?;
                    (min, max) = ([low; 3], [high; 3]);
                }
                _ if first.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.')) => {
                    let n = numbers(line.split_whitespace())?;
                    let point: [f32; 3] = n.try_into().map_err(|_| at("each point needs three numbers"))?;
                    table.push(point);
                }
                // Keywords other programs add, such as LUT_IN_VIDEO_RANGE, change nothing here.
                _ => {}
            }
        }
        let (size, three_d) = match (size_3d, size_1d) {
            (Some(n), None) => (n, true),
            (None, Some(n)) => (n, false),
            (Some(_), Some(_)) => return Err("a LUT with both a 1D and a 3D table is not supported".into()),
            (None, None) => return Err("not a .cube LUT: it gives no LUT_3D_SIZE or LUT_1D_SIZE".into()),
        };
        let expected = if three_d { size * size * size } else { size };
        if table.len() != expected {
            return Err(format!("the table has {} points where its size calls for {expected}", table.len()));
        }
        if (0..3).any(|k| max[k] <= min[k]) {
            return Err("each DOMAIN_MAX must be above its DOMAIN_MIN".into());
        }
        Ok(Self { title, size, three_d, table, min, max })
    }

    /// Points along each axis, and whether it is a 3D table.
    pub fn size(&self) -> (usize, bool) {
        (self.size, self.three_d)
    }

    /// The color a screen color turns into, values from 0 to 1.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let last = (self.size - 1) as f32;
        // Where the color falls on each axis, in points.
        let at = [0, 1, 2].map(|k| {
            let v = (rgb[k] - self.min[k]) / (self.max[k] - self.min[k]);
            if v.is_finite() { v.clamp(0.0, 1.0) * last } else { 0.0 }
        });
        if !self.three_d {
            return [0, 1, 2].map(|k| {
                let i = (at[k].floor() as usize).min(self.size - 2);
                let f = at[k] - i as f32;
                self.table[i][k] * (1.0 - f) + self.table[i + 1][k] * f
            });
        }
        let base = at.map(|v| (v.floor() as usize).min(self.size - 2));
        let [fr, fg, fb] = [0, 1, 2].map(|k| at[k] - base[k] as f32);
        let point = |dr: usize, dg: usize, db: usize| {
            self.table[(base[0] + dr) + self.size * ((base[1] + dg) + self.size * (base[2] + db))]
        };
        // The cube between the eight nearest points splits into six tetrahedra along its grey
        // diagonal; the one the color is in, found by ordering its fractions, gives four
        // points and their weights.
        let (c000, c111) = (point(0, 0, 0), point(1, 1, 1));
        let (w, p1, p2) = if fr > fg {
            if fg > fb {
                ([1.0 - fr, fr - fg, fg - fb, fb], point(1, 0, 0), point(1, 1, 0))
            } else if fr > fb {
                ([1.0 - fr, fr - fb, fb - fg, fg], point(1, 0, 0), point(1, 0, 1))
            } else {
                ([1.0 - fb, fb - fr, fr - fg, fg], point(0, 0, 1), point(1, 0, 1))
            }
        } else if fb > fg {
            ([1.0 - fb, fb - fg, fg - fr, fr], point(0, 0, 1), point(0, 1, 1))
        } else if fb > fr {
            ([1.0 - fg, fg - fb, fb - fr, fr], point(0, 1, 0), point(0, 1, 1))
        } else {
            ([1.0 - fg, fg - fr, fr - fb, fb], point(0, 1, 0), point(1, 1, 0))
        };
        [0, 1, 2].map(|k| w[0] * c000[k] + w[1] * p1[k] + w[2] * p2[k] + w[3] * c111[k])
    }
}

/// Grades the screen colors of `rendered` through the LUT. `strength` from 0 to 1 mixes the
/// graded colors over the picture's own, for a lighter look.
pub fn grade(image: &Image, lut: &Lut, strength: f32, rendered: Region) {
    if strength.is_nan() || strength <= 0.0 {
        return;
    }
    let strength = strength.min(1.0);
    let width = image.width as usize;
    let rows: Vec<u32> = (rendered.y0..rendered.y1).collect();
    use rayon::prelude::*;
    rows.par_iter().for_each(|&y| {
        for x in rendered.x0..rendered.x1 {
            let p = y as usize * width + x as usize;
            let rgba = image.pixels[p].load(Relaxed);
            let color = [0, 1, 2].map(|k| ((rgba >> (8 * k)) & 0xff) as f32 / 255.0);
            let graded = lut.apply(color);
            let mut out = rgba & 0xff00_0000;
            for k in 0..3 {
                let v = color[k] + (graded[k] - color[k]) * strength;
                out |= ((v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32).min(255) << (8 * k);
            }
            image.pixels[p].store(out, Relaxed);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3D table of `size` points per axis, each holding `f` of its own color.
    fn table(size: usize, f: impl Fn([f32; 3]) -> [f32; 3]) -> String {
        let mut text = format!("# made for a test\nTITLE \"Test\"\nLUT_3D_SIZE {size}\n\n");
        let step = 1.0 / (size - 1) as f32;
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let [x, y, z] = f([r as f32 * step, g as f32 * step, b as f32 * step]);
                    text += &format!("{x:.6} {y:.6} {z:.6}\n");
                }
            }
        }
        text
    }

    fn close(a: [f32; 3], b: [f32; 3], tolerance: f32) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() <= tolerance)
    }

    #[test]
    fn a_table_reads_between_its_points() {
        // Tetrahedral interpolation follows any linear map of the colors exactly: here one that
        // swaps red and blue and darkens green.
        let lut = Lut::parse(&table(5, |[r, g, b]| [b, 0.5 * g + 0.1, r])).unwrap();
        assert_eq!(lut.title.as_deref(), Some("Test"));
        assert_eq!(lut.size(), (5, true));
        for color in [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.3, 0.71, 0.05], [0.9, 0.2, 0.6], [0.13, 0.13, 0.9]] {
            let [r, g, b] = color;
            assert!(close(lut.apply(color), [b, 0.5 * g + 0.1, r], 1e-5), "{color:?} -> {:?}", lut.apply(color));
        }
        // Values outside the domain hold at its edges.
        assert!(close(lut.apply([1.5, -0.2, 0.5]), [0.5, 0.1, 1.0], 1e-5));
        // Greys stay grey through a table that keeps them.
        let identity = Lut::parse(&table(3, |c| c)).unwrap();
        assert!(close(identity.apply([0.37, 0.37, 0.37]), [0.37; 3], 1e-6));
    }

    #[test]
    fn one_dimensional_tables_and_domains_are_read() {
        let lut = Lut::parse("LUT_1D_SIZE 3\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n0 0 0\n0.5 0.25 1\n1 1 1\n").unwrap();
        assert_eq!(lut.size(), (3, false));
        // Input 0.5 is a quarter of the way along the domain, halfway to the second point.
        assert!(close(lut.apply([0.5, 0.5, 0.5]), [0.25, 0.125, 0.5], 1e-6));
        let range = Lut::parse("LUT_3D_INPUT_RANGE 0 2\nLUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
        assert!(close(range.apply([1.0, 0.5, 2.0]), [0.5, 0.25, 1.0], 1e-6));
    }

    #[test]
    fn broken_tables_say_what_is_wrong() {
        let error = |text: &str| Lut::parse(text).unwrap_err();
        assert!(error("0 0 0\n").contains("LUT_3D_SIZE"));
        assert!(error("LUT_3D_SIZE 2\n0 0 0\n").contains("1 points where its size calls for 8"));
        assert!(error("LUT_3D_SIZE 1\n").contains("from 2 to 256"));
        assert!(error("LUT_1D_SIZE 2\n0 0\n1 1 1\n").contains("line 2"));
        assert!(error("LUT_1D_SIZE 2\n0 0 x\n1 1 1\n").contains("x is not a number"));
        assert!(error("LUT_1D_SIZE 2\nDOMAIN_MAX 0 1 1\n0 0 0\n1 1 1\n").contains("DOMAIN_MAX"));
    }

    #[test]
    fn grading_mixes_by_strength_within_the_rendered_part() {
        let invert = Lut::parse(&table(2, |c| c.map(|v| 1.0 - v))).unwrap();
        let image = Image::new(2, 1);
        for pixel in &image.pixels {
            pixel.store(0xff00_40c0 | 0x10 << 16, Relaxed);
        }
        let shown = |image: &Image, p: usize| image.pixels[p].load(Relaxed);
        grade(&image, &invert, 0.0, Region { x0: 0, y0: 0, x1: 2, y1: 1 });
        assert_eq!(shown(&image, 0), 0xff10_40c0);
        grade(&image, &invert, 1.0, Region { x0: 1, y0: 0, x1: 2, y1: 1 });
        assert_eq!(shown(&image, 0), 0xff10_40c0, "outside the region");
        assert_eq!(shown(&image, 1), 0xffef_bf3f, "inverted, alpha kept");
        grade(&image, &invert, 0.5, Region { x0: 0, y0: 0, x1: 1, y1: 1 });
        let half = shown(&image, 0);
        assert!((0..3).all(|k| ((half >> (8 * k)) & 0xff).abs_diff(0x80) <= 1), "halfway to the inverse is grey: {half:x}");
    }

    #[test]
    fn the_bundled_looks_read() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/luts");
        let mut count = 0;
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "cube") {
                let lut = Lut::open(&path).unwrap();
                assert!(lut.title.is_some(), "{}", path.display());
                // Every look keeps black dark and white light.
                let (black, white) = (lut.apply([0.0; 3]), lut.apply([1.0; 3]));
                assert!(black.iter().all(|v| *v < 0.15) && white.iter().all(|v| *v > 0.8), "{}: {black:?} {white:?}", path.display());
                count += 1;
            }
        }
        assert!(count >= 5, "{count} looks");
    }
}

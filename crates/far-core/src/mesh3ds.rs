//! 3DS mesh reader.

#[derive(Default)]
pub struct Mesh3ds {
    pub vertices: Vec<[f32; 3]>,
    pub faces: Vec<[u16; 3]>,
}

fn word(b: &[u8], i: usize) -> u16 {
    match b.get(i..i + 2) {
        Some(s) => u16::from_le_bytes([s[0], s[1]]),
        None => 0,
    }
}

fn dword(b: &[u8], i: usize) -> u32 {
    match b.get(i..i + 4) {
        Some(s) => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
        None => 0,
    }
}

fn float(b: &[u8], i: usize) -> f32 {
    f32::from_bits(dword(b, i))
}

/// Walks one chunk. As in the original, only the last object of a file survives,
/// because each vertex or face chunk replaces the previous one.
fn chunk(b: &[u8], m: &mut Mesh3ds, base: usize) -> i64 {
    if base + 6 > b.len() {
        return 0;
    }
    let id = word(b, base);
    let length = dword(b, base + 2) as i32 as i64;
    let mut i: usize = 6;
    match id {
        0x4D4D => {
            while base + i < b.len() {
                let w = word(b, base + i);
                if w == 0x3D3D || w == 0xB000 {
                    break;
                }
                i += 2;
            }
        }
        0x4000 => {
            while base + i < b.len() && b[base + i] != 0 {
                i += 1;
            }
            i += 1;
        }
        0x4110 => {
            let n = word(b, base + i) as usize;
            i += 2;
            m.vertices = (0..n)
                .map(|j| {
                    let p = base + i + j * 12;
                    [float(b, p), float(b, p + 4), float(b, p + 8)]
                })
                .collect();
            i += n * 12;
        }
        0x4120 => {
            let n = word(b, base + i) as usize;
            i += 2;
            m.faces = (0..n)
                .map(|j| {
                    let p = base + i + j * 8;
                    [word(b, p), word(b, p + 2), word(b, p + 4)]
                })
                .collect();
            i += n * 8;
            // The original `case` has no `break` and falls through to the texture
            // coordinate chunk, consuming bytes that are not its own. The only practical
            // effect is advancing `i`, so that is all this reproduces.
            i += 2 + word(b, base + i) as usize * 8;
        }
        0x4140 => {
            i += 2 + word(b, base + i) as usize * 8;
        }
        0x3D3D | 0x4100 => {}
        _ => return length,
    }
    while (i as i64) < length {
        let n = chunk(b, m, base + i);
        if n <= 0 {
            break;
        }
        i += n as usize;
    }
    length
}

pub fn parse(bytes: &[u8]) -> Mesh3ds {
    let mut m = Mesh3ds::default();
    chunk(bytes, &mut m, 0);
    m
}

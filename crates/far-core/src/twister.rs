//! Mersenne Twister generator.
//!
//! The original declared its state as `unsigned long`, which is 32 bits on the
//! Windows of 2008 but 64 bits on macOS and Linux. Built for 64 bits, `rand()`
//! goes above 1 and the diffuse reflection sampling loop almost never ends.
//! Explicit `u32` rules out that kind of error.

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;
const TEMPERING_MASK_B: u32 = 0x9d2c_5680;
const TEMPERING_MASK_C: u32 = 0xefc6_0000;

pub const DEFAULT_SEED: u32 = 0xf271_0812;

pub struct Twister {
    mt: [u32; N],
    mti: usize,
}

impl Twister {
    pub fn new(seed: u32) -> Self {
        let mut mt = [0u32; N];
        mt[0] = seed;
        for i in 1..N {
            mt[i] = mt[i - 1].wrapping_mul(69069);
        }
        let mut s: u32 = 373_737;
        for v in mt.iter_mut().skip(1) {
            *v ^= s;
            s = s.wrapping_mul(5531).wrapping_add(81547);
            s ^= (s >> 9) ^ (s << 19);
        }
        Self { mt, mti: N }
    }

    fn generate(&mut self) {
        let twist = |y: u32| if y & 1 == 0 { 0 } else { MATRIX_A };
        let mt = &mut self.mt;
        for k in 0..N - M {
            let y = (mt[k] & UPPER_MASK) | (mt[k + 1] & LOWER_MASK);
            mt[k] = mt[k + M] ^ (y >> 1) ^ twist(y);
        }
        for k in N - M..N - 1 {
            let y = (mt[k] & UPPER_MASK) | (mt[k + 1] & LOWER_MASK);
            mt[k] = mt[k + M - N] ^ (y >> 1) ^ twist(y);
        }
        let y = (mt[N - 1] & UPPER_MASK) | (mt[0] & LOWER_MASK);
        mt[N - 1] = mt[M - 1] ^ (y >> 1) ^ twist(y);
        self.mti = 0;
    }

    /// Real number in [0, 1).
    #[inline]
    pub fn rand(&mut self) -> f32 {
        if self.mti >= N {
            self.generate();
        }
        let mut y = self.mt[self.mti];
        self.mti += 1;
        y ^= y >> 11;
        y ^= (y << 7) & TEMPERING_MASK_B;
        y ^= (y << 15) & TEMPERING_MASK_C;
        y ^= y >> 18;
        y as f32 * 2.328_306_437_080_797_4e-10_f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rand_stays_between_zero_and_one() {
        let mut t = Twister::new(DEFAULT_SEED);
        for _ in 0..100_000 {
            let r = t.rand();
            assert!((0.0..=1.0).contains(&r), "out of range: {r}");
        }
    }

    #[test]
    fn same_seed_gives_same_sequence() {
        let (mut a, mut b) = (Twister::new(7), Twister::new(7));
        for _ in 0..2000 {
            assert_eq!(a.rand().to_bits(), b.rand().to_bits());
        }
    }
}

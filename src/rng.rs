//! Tiny deterministic random generator (xorshift32).
//!
//! Every voice owns one, so the "life" of a patch (random detune, pan, start
//! phase, later drift) is cheap, allocation free and reproducible.

#[derive(Clone, Copy)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        // xorshift must never be seeded with zero
        Self(seed.max(1))
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform value in [0, 1).
    #[inline]
    pub fn unipolar(&mut self) -> f32 {
        // top 24 bits fit exactly in an f32 mantissa
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform value in [-1, 1).
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        self.unipolar() * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_stay_in_range() {
        let mut rng = Rng::new(12345);
        for _ in 0..10_000 {
            let u = rng.unipolar();
            assert!((0.0..1.0).contains(&u));
            let b = rng.bipolar();
            assert!((-1.0..1.0).contains(&b));
        }
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn zero_seed_is_safe() {
        let mut rng = Rng::new(0);
        assert_ne!(rng.next_u32(), 0);
    }
}

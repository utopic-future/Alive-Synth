//! Analog-style drift: slow, smoothed random walks.
//!
//! Each stream is a mean-reverting random walk (an Ornstein-Uhlenbeck style
//! process), not an LFO. It wanders, never repeats, and stays bounded. A voice
//! owns several independent streams, so no two voices ever move in lockstep.

use crate::rng::Rng;

/// Per-block constants shared by every stream and voice.
#[derive(Clone, Copy)]
pub struct DriftStep {
    reversion: f32,
    sigma: f32,
}

impl DriftStep {
    /// `rate` scales the speed: 1.0 means a time constant of about 2 seconds.
    /// `block_len` is the number of samples since the last step, so the
    /// behaviour stays the same however the host splits its buffers.
    pub fn new(rate: f32, block_len: usize, sample_rate: f32) -> Self {
        let tau = 2.0 / rate.max(0.01);
        let a = 1.0 - (-(block_len as f32) / (sample_rate * tau)).exp();
        // Noise size chosen so the long-run spread is about 0.5 (range -1..1).
        let sigma = 0.5 * (3.0 * (2.0 * a - a * a)).sqrt();
        Self {
            reversion: a,
            sigma,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Drift {
    x: f32,
}

impl Drift {
    /// Start each note somewhere different inside the usual range.
    pub fn randomize(&mut self, rng: &mut Rng) {
        self.x = rng.bipolar() * 0.5;
    }

    /// Advance one step and return the new value in -1..1.
    #[inline]
    pub fn step(&mut self, s: &DriftStep, rng: &mut Rng) -> f32 {
        self.x = (self.x * (1.0 - s.reversion) + s.sigma * rng.bipolar()).clamp(-1.0, 1.0);
        self.x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn stays_bounded_with_expected_spread() {
        let step = DriftStep::new(1.0, 64, SR);
        let mut rng = Rng::new(42);
        let mut d = Drift::default();
        let n = 2_000_000;
        let mut sum_sq = 0.0f64;
        for _ in 0..n {
            let v = d.step(&step, &mut rng);
            assert!((-1.0..=1.0).contains(&v));
            sum_sq += (v * v) as f64;
        }
        let std = (sum_sq / n as f64).sqrt();
        assert!((0.3..0.65).contains(&std), "std {std}");
    }

    #[test]
    fn moves_slowly_from_step_to_step() {
        let step = DriftStep::new(1.0, 64, SR);
        let mut rng = Rng::new(9);
        let mut d = Drift::default();
        let mut prev = d.step(&step, &mut rng);
        for _ in 0..10_000 {
            let v = d.step(&step, &mut rng);
            assert!((v - prev).abs() < 0.05, "jump {}", (v - prev).abs());
            prev = v;
        }
    }

    #[test]
    fn streams_with_different_seeds_diverge() {
        let step = DriftStep::new(1.0, 64, SR);
        let (mut ra, mut rb) = (Rng::new(1), Rng::new(2));
        let (mut a, mut b) = (Drift::default(), Drift::default());
        let mut apart = 0.0f32;
        for _ in 0..50_000 {
            apart = apart.max((a.step(&step, &mut ra) - b.step(&step, &mut rb)).abs());
        }
        assert!(apart > 0.2, "streams should not move together, {apart}");
    }

    #[test]
    fn statistics_do_not_depend_on_block_size() {
        // Same wall-clock time, different buffer splits, similar spread.
        let spread = |block: usize| {
            let step = DriftStep::new(1.0, block, SR);
            let mut rng = Rng::new(5);
            let mut d = Drift::default();
            let steps = (SR as usize * 1000) / block;
            let mut sum_sq = 0.0f64;
            for _ in 0..steps {
                let v = d.step(&step, &mut rng);
                sum_sq += (v * v) as f64;
            }
            (sum_sq / steps as f64).sqrt()
        };
        let (small, large) = (spread(8), spread(64));
        assert!((small - large).abs() < 0.12, "{small} vs {large}");
    }
}

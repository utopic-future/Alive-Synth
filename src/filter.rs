//! Stereo-ready state variable filter (TPT form) with a gentle output saturator.

use nih_plug::prelude::Enum;
use std::f32::consts::PI;

#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterType {
    #[name = "Low-pass"]
    LowPass,
    #[name = "Band-pass"]
    BandPass,
    #[name = "High-pass"]
    HighPass,
}

/// Coefficients for one cutoff and resonance. Computed once per block.
#[derive(Clone, Copy)]
pub struct SvfCoefs {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl SvfCoefs {
    /// `resonance` runs 0..1 and maps to a Q between 0.707 and about 15.
    pub fn new(cutoff_hz: f32, resonance: f32, sample_rate: f32) -> Self {
        let cutoff = cutoff_hz.clamp(20.0, sample_rate * 0.45);
        let g = (PI * cutoff / sample_rate).tan();
        let res = resonance.clamp(0.0, 1.0);
        let q = 0.707 * (1.0 + res * res * 20.0);
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self { a1, a2, a3, k }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Svf {
    s1: f32,
    s2: f32,
}

impl Svf {
    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f32, c: &SvfCoefs, kind: FilterType) -> f32 {
        let v3 = x - self.s2;
        let v1 = c.a1 * self.s1 + c.a2 * v3;
        let v2 = self.s2 + c.a2 * self.s1 + c.a3 * v3;
        self.s1 = 2.0 * v1 - self.s1;
        self.s2 = 2.0 * v2 - self.s2;
        match kind {
            FilterType::LowPass => v2,
            // scaled so the peak gain stays near 1 whatever the resonance
            FilterType::BandPass => v1 * c.k,
            FilterType::HighPass => x - c.k * v1 - v2,
        }
    }
}

/// Smooth saturation so a strong resonance cannot run away.
/// Almost linear for small signals, limits at about +-2.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let x = (x * 0.5).clamp(-3.0, 3.0);
    2.0 * x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn rms_through(kind: FilterType, freq: f32, cutoff: f32, res: f32) -> f32 {
        let c = SvfCoefs::new(cutoff, res, SR);
        let mut f = Svf::default();
        let n = 9_600;
        let mut sum = 0.0;
        for i in 0..n {
            let x = (2.0 * PI * freq * i as f32 / SR).sin();
            let y = f.process(x, &c, kind);
            if i >= n / 2 {
                sum += y * y;
            }
        }
        (sum / (n / 2) as f32).sqrt()
    }

    #[test]
    fn low_pass_keeps_lows_and_cuts_highs() {
        let low = rms_through(FilterType::LowPass, 100.0, 1_000.0, 0.0);
        let high = rms_through(FilterType::LowPass, 10_000.0, 1_000.0, 0.0);
        assert!(low > 0.6, "low {low}");
        assert!(high < 0.05, "high {high}");
    }

    #[test]
    fn high_pass_keeps_highs_and_cuts_lows() {
        let low = rms_through(FilterType::HighPass, 100.0, 1_000.0, 0.0);
        let high = rms_through(FilterType::HighPass, 10_000.0, 1_000.0, 0.0);
        assert!(high > 0.6, "high {high}");
        assert!(low < 0.05, "low {low}");
    }

    #[test]
    fn band_pass_peaks_at_cutoff() {
        let at = rms_through(FilterType::BandPass, 1_000.0, 1_000.0, 0.5);
        let away = rms_through(FilterType::BandPass, 100.0, 1_000.0, 0.5);
        assert!(at > away * 3.0, "at {at}, away {away}");
    }

    #[test]
    fn stays_finite_at_extreme_settings() {
        for kind in [FilterType::LowPass, FilterType::BandPass, FilterType::HighPass] {
            let c = SvfCoefs::new(30_000.0, 1.0, SR);
            let mut f = Svf::default();
            for i in 0..48_000 {
                let x = if i % 100 < 50 { 1.0 } else { -1.0 };
                let y = soft_clip(f.process(x, &c, kind));
                assert!(y.is_finite() && y.abs() <= 2.01);
            }
        }
    }

    #[test]
    fn soft_clip_is_nearly_linear_when_small() {
        assert!((soft_clip(0.1) - 0.1).abs() < 0.001);
        assert!(soft_clip(100.0) <= 2.0 + 1e-3);
    }
}

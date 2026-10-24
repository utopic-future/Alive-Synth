//! Built-in grain sources: four evolving textures generated in code.
//!
//! Every source is a seamless loop of `LOOP_SECONDS` made at the host sample
//! rate (so nothing is stored in the plugin file and nothing is licensed).
//! The loops are built from sinusoids that complete a whole number of cycles
//! per loop, so they repeat without a seam.
//!
//! Playing a source faster raises its pitch and would alias, so each source is
//! stored in `SRC_LEVELS` band-limited versions. Level `l` holds only content
//! that stays below Nyquist when read at up to `2^l` times the normal speed,
//! the same idea as the mipmapped wavetables.
//!
//! Building takes a fraction of a second, so it happens in `initialize()`.

use crate::filter::{FilterType, Svf, SvfCoefs};
use crate::rng::Rng;
use crate::wavetable::WavetableBank;
use nih_plug::prelude::Enum;
use std::f32::consts::TAU;

/// Textures, not counting the wavetable option.
pub const NUM_SOURCES: usize = 4;
pub const SRC_LEVELS: usize = 6;
pub const LOOP_SECONDS: f32 = 3.0;
/// Cycles of the root pitch per loop: 384 cycles in 3 seconds is 128 Hz.
const ROOT_CYCLES: u32 = 384;
const ROOT_HZ: f64 = 128.0;
/// Points used to describe slow changes (vowel, shimmer) over one loop.
const CONTROL_POINTS: usize = 512;

/// What the grains read from.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrainSource {
    #[name = "Wavetable"]
    Wavetable,
    #[name = "Choir"]
    Choir,
    #[name = "Glass Bells"]
    Bells,
    #[name = "Breath"]
    Breath,
    #[name = "Saw Swarm"]
    Swarm,
}

impl GrainSource {
    /// 0 for the wavetable, 1 to 4 for the textures.
    pub fn index(self) -> usize {
        self as usize
    }
}

pub struct SourceBank {
    len: usize,
    sample_rate: f32,
    /// Flattened `[source][level][sample]`.
    data: Vec<f32>,
}

impl SourceBank {
    pub fn empty() -> Self {
        Self {
            len: 0,
            sample_rate: 0.0,
            data: Vec::new(),
        }
    }

    pub fn new(sample_rate: f32, bank: &WavetableBank) -> Self {
        let n = (LOOP_SECONDS * sample_rate).round() as usize;
        let mut data = Vec::with_capacity(NUM_SOURCES * SRC_LEVELS * n);

        for source in 0..NUM_SOURCES {
            let mut b = Builder::new(n, sample_rate);
            match source {
                0 => build_choir(&mut b),
                1 => build_bells(&mut b),
                2 => build_breath(&mut b),
                _ => build_swarm(&mut b, bank),
            }
            normalize(&mut b.levels);
            for level in &b.levels {
                data.extend_from_slice(level);
            }
        }

        Self {
            len: n,
            sample_rate,
            data,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Samples per loop.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Pitch of the loop when played at normal speed.
    pub fn root_hz() -> f64 {
        ROOT_HZ
    }

    /// Band-limited version to use when reading at `speed` times normal speed.
    pub fn level_for_speed(speed: f64) -> usize {
        if speed <= 1.0 {
            0
        } else {
            (speed.log2().ceil() as usize).min(SRC_LEVELS - 1)
        }
    }

    /// Read at a fractional sample position (0 up to `len`), linearly interpolated.
    #[inline]
    pub fn read(&self, source: usize, level: usize, pos: f64) -> f32 {
        let i = pos as usize;
        if i >= self.len {
            return 0.0;
        }
        let frac = (pos - i as f64) as f32;
        let start = (source * SRC_LEVELS + level.min(SRC_LEVELS - 1)) * self.len;
        let buf = &self.data[start..start + self.len];
        let next = if i + 1 == self.len { 0 } else { i + 1 };
        buf[i] + (buf[next] - buf[i]) * frac
    }
}

struct Builder {
    n: usize,
    sr: f32,
    levels: Vec<Vec<f32>>,
}

impl Builder {
    fn new(n: usize, sr: f32) -> Self {
        Self {
            n,
            sr,
            levels: vec![vec![0.0; n]; SRC_LEVELS],
        }
    }

    /// Highest level that still contains a partial at `hz`, or none above Nyquist.
    fn top_level(&self, hz: f32) -> Option<usize> {
        let max_hz = self.sr * 0.45;
        if hz > max_hz {
            return None;
        }
        Some(((max_hz / hz).log2().floor() as usize).min(SRC_LEVELS - 1))
    }

    /// Sample a slow curve (`u` runs 0..1 over the loop) at the control points.
    fn controls(f: impl Fn(f32) -> f32) -> Vec<f32> {
        (0..CONTROL_POINTS)
            .map(|k| f(k as f32 / CONTROL_POINTS as f32))
            .collect()
    }

    /// Add one sinusoid that makes `cycles` whole cycles per loop.
    ///
    /// `amps` is a periodic amplitude curve at the control points. `vib` and
    /// `beta` add vibrato: a periodic wobble scaled by `beta` radians.
    fn add_partial(&mut self, cycles: u32, phase0: f32, beta: f32, vib: &[f32], amps: &[f32]) {
        let hz = cycles as f32 / LOOP_SECONDS;
        let Some(top) = self.top_level(hz) else {
            return;
        };
        let n_total = self.n;
        let inv_n = 1.0 / n_total as f32;
        let k = amps.len();
        let k_per_sample = k as f32 / n_total as f32;
        // cycles * n modulo the loop length, kept exact with integer steps
        let mut idx = 0usize;

        for n in 0..n_total {
            let pos = n as f32 * k_per_sample;
            let seg = (pos as usize).min(k - 1);
            let t = pos - seg as f32;
            let a = amps[seg] + (amps[(seg + 1) % k] - amps[seg]) * t;

            let wobble = if beta != 0.0 { beta * vib[n] } else { 0.0 };
            let s = (TAU * idx as f32 * inv_n + phase0 + wobble).sin() * a;
            for level in self.levels[..=top].iter_mut() {
                level[n] += s;
            }

            idx += cycles as usize;
            if idx >= n_total {
                idx -= n_total;
            }
        }
    }
}

fn smoothstep(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

// ---------------------------------------------------------------- Choir

/// First three formant frequencies of a, e, i, o, u.
const VOWELS: [[f32; 3]; 5] = [
    [800.0, 1150.0, 2900.0],
    [400.0, 1600.0, 2700.0],
    [270.0, 2300.0, 3000.0],
    [450.0, 800.0, 2830.0],
    [325.0, 700.0, 2530.0],
];
const FORMANT_GAIN: [f32; 3] = [1.0, 0.55, 0.25];
const FORMANT_WIDTH: [f32; 3] = [110.0, 130.0, 160.0];

/// Strength of a harmonic at `hz` while the vowel is at `u` (0..1 over the loop).
fn formant_gain(hz: f32, u: f32) -> f32 {
    let v = u.rem_euclid(1.0) * VOWELS.len() as f32;
    let i = (v as usize).min(VOWELS.len() - 1);
    let t = smoothstep(v - i as f32);
    let next = (i + 1) % VOWELS.len();

    let mut gain = 0.015;
    for j in 0..3 {
        let center = VOWELS[i][j] + (VOWELS[next][j] - VOWELS[i][j]) * t;
        let d = (hz - center) / FORMANT_WIDTH[j];
        gain += FORMANT_GAIN[j] * (-0.5 * d * d).exp();
    }
    gain
}

/// Two slightly detuned singers gliding through the five vowels, with vibrato.
fn build_choir(b: &mut Builder) {
    let mut rng = Rng::new(0xC401_0001);
    let n = b.n;

    for voice in 0..2u32 {
        // 15 vibrato cycles per 3 s loop is 5 Hz
        let vib: Vec<f32> = (0..n)
            .map(|i| (TAU * (15.0 * i as f32 / n as f32 + 0.31 * voice as f32)).sin())
            .collect();
        let time_offset = [0.0, 0.37][voice as usize];
        let depth = [0.07, 0.055][voice as usize];

        for h in 1..=48u32 {
            let cycles = (ROOT_CYCLES + voice) * h;
            let hz = cycles as f32 / LOOP_SECONDS;
            if hz > b.sr * 0.45 {
                break;
            }
            let amps = Builder::controls(|u| {
                formant_gain(hz, u + time_offset) * 0.5 / (1.0 + hz / 2500.0)
            });
            b.add_partial(cycles, rng.unipolar() * TAU, depth * h as f32, &vib, &amps);
        }
    }
}

// ---------------------------------------------------------------- Bells

/// Inharmonic partials, each doubled with a slight detune so it shimmers.
fn build_bells(b: &mut Builder) {
    let mut rng = Rng::new(0xBE11_0002);
    // (ratio to the root, weight)
    const PARTIALS: [(f32, f32); 9] = [
        (0.5, 0.6),
        (1.0, 1.0),
        (2.0, 0.8),
        (2.76, 0.5),
        (3.0, 0.6),
        (4.07, 0.35),
        (5.4, 0.25),
        (6.78, 0.18),
        (8.93, 0.12),
    ];

    for (ratio, weight) in PARTIALS {
        for detune in [0u32, 2] {
            let cycles = (ROOT_CYCLES as f32 * ratio).round() as u32 + detune;
            // whole shimmer cycles per loop keep the loop seamless
            let m = 1.0 + (rng.next_u32() % 3) as f32;
            let p = rng.unipolar();
            let amps = Builder::controls(|u| {
                weight * 0.18 * (0.55 + 0.45 * (TAU * (m * u + p)).sin())
            });
            b.add_partial(cycles, rng.unipolar() * TAU, 0.0, &[], &amps);
        }
    }
}

// --------------------------------------------------------------- Breath

/// Noise through a band-pass that sweeps up and down once per loop.
fn build_breath(b: &mut Builder) {
    let n = b.n;
    let sr = b.sr;
    let mut rng = Rng::new(0xB2EA_0003);
    let noise: Vec<f32> = (0..n).map(|_| rng.bipolar()).collect();

    // Two passes: the first only warms up the filter, so the end of the loop
    // flows into the start.
    let mut sweep = vec![0.0f32; n];
    let mut filter = Svf::default();
    for pass in 0..2 {
        for i in 0..n {
            let u = i as f32 / n as f32;
            let cutoff = 300.0 * 2f32.powf(3.0 * (0.5 + 0.5 * (TAU * u).sin()));
            let c = SvfCoefs::new(cutoff, 0.55, sr);
            let y = filter.process(noise[i], &c, FilterType::BandPass);
            if pass == 1 {
                sweep[i] = y + 0.15 * noise[i];
            }
        }
    }
    b.levels[0].copy_from_slice(&sweep);

    // Darker copies for fast playback: two cascaded low-passes per level.
    for level in 1..SRC_LEVELS {
        let cutoff = sr * 0.45 / (1u32 << level) as f32;
        let coefs = SvfCoefs::new(cutoff, 0.0, sr);
        let mut buf = sweep.clone();
        for _ in 0..2 {
            let mut f = Svf::default();
            for pass in 0..2 {
                for s in buf.iter_mut() {
                    let y = f.process(*s, &coefs, FilterType::LowPass);
                    if pass == 1 {
                        *s = y;
                    }
                }
            }
        }
        b.levels[level].copy_from_slice(&buf);
    }
}

// ---------------------------------------------------------------- Swarm

/// Seven detuned saws that slowly breathe in and out of each other.
fn build_swarm(b: &mut Builder, bank: &WavetableBank) {
    let n = b.n;
    let inv_n = 1.0 / n as f32;
    let mut rng = Rng::new(0x5A71_0004);

    // (cycles per loop, start phase, shimmer cycles, shimmer phase)
    let saws: Vec<(u32, f32, f32, f32)> = (-3i32..=3)
        .map(|d| {
            (
                (ROOT_CYCLES as i32 + 2 * d) as u32,
                rng.unipolar(),
                1.0 + (rng.next_u32() % 3) as f32,
                rng.unipolar(),
            )
        })
        .collect();

    for level in 0..SRC_LEVELS {
        // Pick wavetable mips as if the sample rate were lower, so each level
        // keeps only the harmonics that survive faster playback.
        let sr_eff = b.sr / (1u32 << level) as f32;
        for &(cycles, phase0, m, p) in &saws {
            let hz = cycles as f32 / LOOP_SECONDS;
            let mip = WavetableBank::level_for_frequency(hz, sr_eff);
            let mut idx = 0usize;
            for i in 0..n {
                let phase = (idx as f32 * inv_n + phase0).fract();
                let u = i as f32 * inv_n;
                let gain = 0.1 * (0.6 + 0.4 * (TAU * (m * u + p)).sin());
                b.levels[level][i] += bank.sample(phase, 2.0 / 7.0, mip) * gain;
                idx += cycles as usize;
                if idx >= n {
                    idx -= n;
                }
            }
        }
    }
}

/// Match loudness between sources, with one gain for all levels of a source.
fn normalize(levels: &mut [Vec<f32>]) {
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;
    for &s in &levels[0] {
        sum_sq += (s * s) as f64;
        peak = peak.max(s.abs());
    }
    let rms = (sum_sq / levels[0].len() as f64).sqrt() as f32;
    let gain = (0.4 / rms.max(1e-9)).min(1.4 / peak.max(1e-9));
    for level in levels.iter_mut() {
        for s in level.iter_mut() {
            *s *= gain;
        }
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use std::sync::OnceLock;

    /// One wavetable bank and source bank at 48 kHz, shared by every test.
    pub fn shared() -> &'static (WavetableBank, SourceBank) {
        static CELL: OnceLock<(WavetableBank, SourceBank)> = OnceLock::new();
        CELL.get_or_init(|| {
            let bank = WavetableBank::new();
            let sources = SourceBank::new(48_000.0, &bank);
            (bank, sources)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::shared;
    use super::*;

    fn level_slice(s: &SourceBank, source: usize, level: usize) -> &[f32] {
        let start = (source * SRC_LEVELS + level) * s.len();
        &s.data[start..start + s.len()]
    }

    fn second_diff_rms(v: &[f32]) -> f32 {
        let sum: f32 = v.windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).powi(2)).sum();
        (sum / v.len() as f32).sqrt()
    }

    #[test]
    fn length_matches_loop_time() {
        let (_, s) = shared();
        assert_eq!(s.len(), 144_000);
        assert_eq!(s.sample_rate(), 48_000.0);
        assert!(!s.is_empty());
    }

    #[test]
    fn all_sources_are_finite_and_loud_enough() {
        let (_, s) = shared();
        for source in 0..NUM_SOURCES {
            for level in 0..SRC_LEVELS {
                let v = level_slice(s, source, level);
                assert!(v.iter().all(|x| x.is_finite()), "source {source} level {level}");
            }
            let v = level_slice(s, source, 0);
            let rms = (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
            let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!((0.1..0.5).contains(&rms), "source {source} rms {rms}");
            assert!(peak <= 1.41, "source {source} peak {peak}");
        }
    }

    #[test]
    fn loops_have_no_seam() {
        let (_, s) = shared();
        for source in 0..NUM_SOURCES {
            let v = level_slice(s, source, 0);
            let mean_step =
                v.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (v.len() - 1) as f32;
            let jump = (v[0] - v[v.len() - 1]).abs();
            assert!(
                jump < 5.0 * mean_step + 1e-4,
                "source {source}: jump {jump}, typical step {mean_step}"
            );
        }
    }

    #[test]
    fn higher_levels_never_get_brighter() {
        let (_, s) = shared();
        for source in 0..NUM_SOURCES {
            let mut previous = f32::MAX;
            for level in 0..SRC_LEVELS {
                let r = second_diff_rms(level_slice(s, source, level));
                assert!(r <= previous * 1.02, "source {source} level {level}: {r} vs {previous}");
                previous = r;
            }
        }
        // sources with plenty of treble must clearly darken
        for source in [0usize, 2, 3] {
            let bright = second_diff_rms(level_slice(s, source, 0));
            let dark = second_diff_rms(level_slice(s, source, 4));
            assert!(dark < bright * 0.7, "source {source}: {dark} vs {bright}");
        }
    }

    #[test]
    fn sources_sound_different() {
        let (_, s) = shared();
        let a = level_slice(s, 0, 0);
        for other in 1..NUM_SOURCES {
            let b = level_slice(s, other, 0);
            let diff: f32 = a.iter().zip(b).take(20_000).map(|(x, y)| (x - y).abs()).sum();
            assert!(diff > 100.0, "source {other} too similar to choir");
        }
    }

    #[test]
    fn read_interpolates_and_wraps() {
        let (_, s) = shared();
        let a = s.read(0, 0, 1000.0);
        let b = s.read(0, 0, 1001.0);
        let mid = s.read(0, 0, 1000.5);
        assert!((mid - (a + b) * 0.5).abs() < 1e-5);

        // between the last and the first sample it must not read past the end
        let last = s.len() as f64 - 0.5;
        let wrapped = s.read(0, 0, last);
        let expected = (s.read(0, 0, (s.len() - 1) as f64) + s.read(0, 0, 0.0)) * 0.5;
        assert!((wrapped - expected).abs() < 1e-5);

        // out of range reads are silent instead of panicking
        assert_eq!(s.read(0, 0, s.len() as f64 + 10.0), 0.0);
    }

    #[test]
    fn level_follows_playback_speed() {
        assert_eq!(SourceBank::level_for_speed(0.5), 0);
        assert_eq!(SourceBank::level_for_speed(1.0), 0);
        assert_eq!(SourceBank::level_for_speed(1.5), 1);
        assert_eq!(SourceBank::level_for_speed(2.0), 1);
        assert_eq!(SourceBank::level_for_speed(3.0), 2);
        assert_eq!(SourceBank::level_for_speed(1000.0), SRC_LEVELS - 1);
    }

    #[test]
    fn root_pitch_is_exact() {
        // 384 whole cycles in a 3 second loop
        assert_eq!(SourceBank::root_hz(), ROOT_CYCLES as f64 / LOOP_SECONDS as f64);
    }
}

#[cfg(test)]
mod build_time {
    use super::*;
    use std::time::Instant;

    /// Run with: cargo test --release build_time -- --ignored --nocapture
    #[test]
    #[ignore]
    fn how_long_does_building_take() {
        let bank = WavetableBank::new();
        for sr in [44_100.0, 48_000.0, 96_000.0] {
            let start = Instant::now();
            let s = SourceBank::new(sr, &bank);
            let mb = (s.data.len() * 4) as f32 / 1_048_576.0;
            println!("{sr} Hz: {:.2} s to build, {mb:.1} MB", start.elapsed().as_secs_f32());
        }
    }
}

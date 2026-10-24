//! Granular engine: a cloud of short, windowed grains.
//!
//! Every grain is a tiny player with its own pitch, position, pan, length and
//! amplitude, all drawn at random around the voice. Spawned grains overlap, so
//! one voice becomes a stereo cloud instead of one line.
//!
//! A grain reads either the built-in wavetable bank (default) or one of the
//! built-in texture sources. Each grain remembers which one it started with,
//! so changing the source never cuts or confuses grains already sounding.

use crate::rng::Rng;
use crate::sources::SourceBank;
use crate::wavetable::WavetableBank;
use std::f32::consts::FRAC_PI_4;

/// Grains alive at once per voice. Spawning pauses while the pool is full.
pub const MAX_GRAINS: usize = 32;
/// Random grain length variation, as a fraction either way.
const SIZE_JITTER: f32 = 0.2;
/// Largest wavetable position offset a grain can get at Scatter = 100%.
const MAX_SCATTER: f32 = 0.4;
/// Same for textures, as a fraction of the loop either way.
const MAX_SCATTER_SOURCE: f32 = 0.5;

/// Per-block grain settings, shared by every voice.
#[derive(Clone, Copy)]
pub struct GrainSettings {
    /// 0 = only the wavetable oscillator, 1 = only the grain cloud.
    pub mix: f32,
    /// 0 reads the wavetable, 1 to 4 read the built-in textures.
    pub source: usize,
    density: f32,
    size_s: f32,
    spread_cents: f32,
    scatter: f32,
    pan: f32,
    /// Fraction of the grain used by each fade, 0.05 (flat) to 0.5 (Hann-like).
    edge: f32,
    /// Average number of grains sounding at once, weighted by the window.
    overlap: f32,
    /// Level compensation for how many grains overlap.
    gain: f32,
}

/// Grains with the same pitch add up like one signal (divide by the overlap).
/// Detuned grains, and textures read from different places, add up like noise
/// (divide by its square root).
fn overlap_gain(overlap: f32, spread_cents: f32, source: usize) -> f32 {
    let coherence = if source == 0 {
        (-spread_cents / 15.0).exp()
    } else {
        0.0
    };
    overlap.powf(-(0.5 + 0.5 * coherence))
}

impl GrainSettings {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mix: f32,
        density: f32,
        size_ms: f32,
        spread_cents: f32,
        scatter: f32,
        pan: f32,
        shape: f32,
    ) -> Self {
        let density = density.max(0.1);
        let size_s = (size_ms * 0.001).max(0.002);
        let edge = 0.5 - 0.45 * shape.clamp(0.0, 1.0);
        let overlap = (density * size_s * (1.0 - edge)).max(1.0);

        Self {
            mix: mix.clamp(0.0, 1.0),
            source: 0,
            density,
            size_s,
            spread_cents,
            scatter,
            pan,
            edge,
            overlap,
            gain: overlap_gain(overlap, spread_cents, 0),
        }
    }

    /// Choose what the grains read: 0 for the wavetable, 1 to 4 for textures.
    pub fn with_source(mut self, source: usize) -> Self {
        self.source = source;
        self.gain = overlap_gain(self.overlap, self.spread_cents, source);
        self
    }

    #[cfg(test)]
    pub fn off() -> Self {
        Self::new(0.0, 30.0, 80.0, 0.0, 0.0, 0.0, 0.0)
    }
}

/// What a grain needs to read sound.
pub struct GrainEnv<'a> {
    pub bank: &'a WavetableBank,
    pub sources: &'a SourceBank,
    pub sample_rate: f32,
}

#[derive(Clone, Copy)]
struct Grain {
    active: bool,
    /// 0 = wavetable, 1 to 4 = texture. Fixed when the grain starts.
    source: u8,
    /// Mip level of the wavetable or of the texture.
    level: usize,
    // wavetable grains
    phase: f32,
    phase_inc: f32,
    pos_offset: f32,
    // texture grains, in samples of the loop (f64 keeps long loops in tune)
    src_pos: f64,
    src_speed: f64,
    /// 0..1 through the grain.
    progress: f32,
    progress_inc: f32,
    amp: f32,
    gain_l: f32,
    gain_r: f32,
}

impl Grain {
    const INACTIVE: Grain = Grain {
        active: false,
        source: 0,
        level: 0,
        phase: 0.0,
        phase_inc: 0.0,
        pos_offset: 0.0,
        src_pos: 0.0,
        src_speed: 0.0,
        progress: 0.0,
        progress_inc: 0.0,
        amp: 0.0,
        gain_l: 0.0,
        gain_r: 0.0,
    };
}

/// Fade in and out with smooth ramps (smoothstep) and a flat middle.
#[inline]
fn window(t: f32, edge: f32) -> f32 {
    let x = if t < edge {
        t / edge
    } else if t > 1.0 - edge {
        (1.0 - t) / edge
    } else {
        return 1.0;
    };
    x * x * (3.0 - 2.0 * x)
}

pub struct GrainCloud {
    grains: [Grain; MAX_GRAINS],
    /// Samples until the next grain starts.
    countdown: f32,
}

impl GrainCloud {
    pub fn new() -> Self {
        Self {
            grains: [Grain::INACTIVE; MAX_GRAINS],
            countdown: 0.0,
        }
    }

    pub fn reset(&mut self) {
        for g in self.grains.iter_mut() {
            g.active = false;
        }
        self.countdown = 0.0;
    }

    #[cfg(test)]
    pub fn active_count(&self) -> usize {
        self.grains.iter().filter(|g| g.active).count()
    }

    /// Produce one stereo sample of the cloud.
    ///
    /// `position` is the position the voice is at right now. Wavetable grains
    /// follow it live, texture grains start from it. `freq` and `phase` are
    /// the voice's current pitch and oscillator phase, so a wavetable cloud
    /// without spread stays in tune and in phase.
    #[inline]
    pub fn next(
        &mut self,
        env: &GrainEnv,
        position: f32,
        settings: &GrainSettings,
        freq: f32,
        phase: f32,
        rng: &mut Rng,
    ) -> (f32, f32) {
        self.countdown -= 1.0;
        if self.countdown <= 0.0 {
            self.spawn(env, settings, position, freq, phase, rng);
            // Slightly uneven timing sounds more natural than a metronome.
            let interval = env.sample_rate / settings.density;
            self.countdown += interval * (1.0 + 0.5 * rng.bipolar());
        }

        let loop_len = env.sources.len() as f64;
        let (mut l, mut r) = (0.0, 0.0);
        for g in self.grains.iter_mut().filter(|g| g.active) {
            let raw = if g.source == 0 {
                let pos = (position + g.pos_offset).clamp(0.0, 1.0);
                let v = env.bank.sample(g.phase, pos, g.level);
                g.phase += g.phase_inc;
                if g.phase >= 1.0 {
                    g.phase -= 1.0;
                }
                v
            } else {
                let v = env.sources.read(g.source as usize - 1, g.level, g.src_pos);
                g.src_pos += g.src_speed;
                if g.src_pos >= loop_len {
                    g.src_pos -= loop_len;
                }
                v
            };

            let x = raw * window(g.progress, settings.edge) * g.amp;
            l += x * g.gain_l;
            r += x * g.gain_r;

            g.progress += g.progress_inc;
            if g.progress >= 1.0 {
                g.active = false;
            }
        }
        (l * settings.gain, r * settings.gain)
    }

    fn spawn(
        &mut self,
        env: &GrainEnv,
        s: &GrainSettings,
        position: f32,
        freq: f32,
        phase: f32,
        rng: &mut Rng,
    ) {
        // Textures need a built source. Before initialize() there is none.
        if s.source != 0 && env.sources.is_empty() {
            return;
        }
        let Some(g) = self.grains.iter_mut().find(|g| !g.active) else {
            return;
        };

        let cents = rng.bipolar() * s.spread_cents;
        let grain_freq = freq * 2f32.powf(cents / 1200.0);
        let length = (s.size_s * (1.0 + SIZE_JITTER * rng.bipolar())).max(0.002) * env.sample_rate;
        let pan = rng.bipolar() * s.pan;
        let angle = (pan + 1.0) * FRAC_PI_4;

        *g = Grain {
            active: true,
            source: s.source as u8,
            progress: 0.0,
            progress_inc: 1.0 / length,
            amp: 0.7 + 0.3 * rng.unipolar(),
            gain_l: angle.cos(),
            gain_r: angle.sin(),
            ..Grain::INACTIVE
        };

        if s.source == 0 {
            g.phase = phase;
            g.phase_inc = grain_freq / env.sample_rate;
            g.level = WavetableBank::level_for_frequency(grain_freq * 1.02, env.sample_rate);
            g.pos_offset = rng.bipolar() * s.scatter * MAX_SCATTER;
        } else {
            // Play the loop faster or slower to reach the grain's pitch.
            let speed = grain_freq as f64 / SourceBank::root_hz();
            let loop_len = env.sources.len() as f64;
            let at = (position + rng.bipolar() * s.scatter * MAX_SCATTER_SOURCE).rem_euclid(1.0);
            let mut start = at as f64 * loop_len;
            if start >= loop_len {
                start -= loop_len;
            }
            g.src_pos = start;
            g.src_speed = speed;
            g.level = SourceBank::level_for_speed(speed * 1.02);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::test_support::shared;

    const SR: f32 = 48_000.0;

    fn run_at(
        settings: &GrainSettings,
        seconds: f32,
        seed: u32,
        position: f32,
    ) -> (Vec<f32>, Vec<f32>, usize) {
        let (bank, sources) = shared();
        let env = GrainEnv {
            bank,
            sources,
            sample_rate: SR,
        };
        let mut cloud = GrainCloud::new();
        let mut rng = Rng::new(seed);
        let freq = 220.0;
        let mut phase = 0.0f32;
        let (mut l, mut r) = (Vec::new(), Vec::new());
        let mut max_active = 0;
        for _ in 0..(SR * seconds) as usize {
            let (a, b) = cloud.next(&env, position, settings, freq, phase, &mut rng);
            l.push(a);
            r.push(b);
            max_active = max_active.max(cloud.active_count());
            phase += freq / SR;
            if phase >= 1.0 {
                phase -= 1.0;
            }
        }
        (l, r, max_active)
    }

    fn run(settings: &GrainSettings, seconds: f32, seed: u32) -> (Vec<f32>, Vec<f32>, usize) {
        run_at(settings, seconds, seed, 0.3)
    }

    #[test]
    fn window_is_smooth_symmetric_and_flat_in_the_middle() {
        assert_eq!(window(0.0, 0.25), 0.0);
        assert!((window(0.5, 0.25) - 1.0).abs() < 1e-6);
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            let w = window(t, 0.25);
            assert!((0.0..=1.0).contains(&w));
            assert!((w - window(1.0 - t, 0.25)).abs() < 1e-5);
        }
        // Hann-like at the widest setting
        assert!((window(0.25, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn denser_clouds_are_turned_down() {
        let sparse = GrainSettings::new(1.0, 10.0, 50.0, 30.0, 0.0, 0.0, 0.0);
        let dense = GrainSettings::new(1.0, 100.0, 400.0, 30.0, 0.0, 0.0, 0.0);
        assert!(sparse.gain <= 1.0);
        assert!(dense.gain < sparse.gain);
        // in tune, grains add up more, so they are turned down more
        let tuned = GrainSettings::new(1.0, 100.0, 400.0, 0.0, 0.0, 0.0, 0.0);
        assert!(tuned.gain < dense.gain);
        // textures never add up coherently, whatever the pitch spread
        assert!(tuned.with_source(1).gain > tuned.gain);
    }

    #[test]
    fn makes_sound_and_stays_bounded() {
        let s = GrainSettings::new(1.0, 30.0, 80.0, 12.0, 0.3, 0.7, 0.0);
        let (l, r, _) = run(&s, 1.0, 1);
        assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 2.0));
        assert!(l.iter().any(|x| x.abs() > 0.05), "cloud should be audible");
    }

    #[test]
    fn pan_spread_makes_the_cloud_stereo() {
        let wide = GrainSettings::new(1.0, 30.0, 80.0, 12.0, 0.3, 1.0, 0.0);
        let (l, r, _) = run(&wide, 1.0, 2);
        let diff: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 10.0, "expected stereo width, got {diff}");

        let centred = GrainSettings::new(1.0, 30.0, 80.0, 12.0, 0.3, 0.0, 0.0);
        let (l, r, _) = run(&centred, 1.0, 2);
        for (a, b) in l.iter().zip(&r) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn grain_pool_never_overflows() {
        let s = GrainSettings::new(1.0, 100.0, 500.0, 50.0, 0.5, 1.0, 1.0);
        let (l, r, max_active) = run(&s, 2.0, 3);
        assert!(max_active <= MAX_GRAINS);
        assert!(max_active > MAX_GRAINS / 2, "dense setting should fill the pool");
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
    }

    #[test]
    fn same_seed_gives_same_cloud() {
        let s = GrainSettings::new(1.0, 40.0, 60.0, 20.0, 0.5, 0.8, 0.3);
        let (a, _, _) = run(&s, 0.5, 11);
        let (b, _, _) = run(&s, 0.5, 11);
        let (c, _, _) = run(&s, 0.5, 12);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn reset_silences_the_cloud() {
        let (bank, sources) = shared();
        let env = GrainEnv {
            bank,
            sources,
            sample_rate: SR,
        };
        let s = GrainSettings::new(1.0, 40.0, 200.0, 10.0, 0.0, 0.0, 0.0);
        let mut cloud = GrainCloud::new();
        let mut rng = Rng::new(4);
        for _ in 0..10_000 {
            cloud.next(&env, 0.0, &s, 220.0, 0.0, &mut rng);
        }
        assert!(cloud.active_count() > 0);
        cloud.reset();
        assert_eq!(cloud.active_count(), 0);
    }

    #[test]
    fn every_texture_makes_a_stereo_cloud_that_stays_bounded() {
        let base = GrainSettings::new(1.0, 30.0, 120.0, 12.0, 0.3, 0.8, 0.0);
        let mut outputs = Vec::new();
        for source in 1..=4 {
            let s = base.with_source(source);
            let (l, r, _) = run(&s, 1.5, 21);
            assert!(
                l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 3.0),
                "source {source}"
            );
            let energy: f32 = l.iter().map(|x| x * x).sum();
            assert!(energy > 1.0, "source {source} should be audible, energy {energy}");
            let diff: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
            assert!(diff > 10.0, "source {source} should be wide");
            outputs.push(l);
        }
        // different textures must not collapse into the same sound
        for i in 0..outputs.len() {
            for j in i + 1..outputs.len() {
                let d: f32 = outputs[i].iter().zip(&outputs[j]).map(|(a, b)| (a - b).abs()).sum();
                assert!(d > 50.0, "sources {i} and {j} too similar");
            }
        }
    }

    #[test]
    fn texture_position_picks_where_in_the_loop_grains_start() {
        let s = GrainSettings::new(1.0, 30.0, 120.0, 0.0, 0.0, 0.5, 0.0).with_source(1);
        let (a, _, _) = run_at(&s, 0.5, 8, 0.1);
        let (b, _, _) = run_at(&s, 0.5, 8, 0.6);
        let (a2, _, _) = run_at(&s, 0.5, 8, 0.1);
        let diff: f32 = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 10.0, "different positions should sound different");
        assert_eq!(a, a2);
    }

    #[test]
    fn switching_source_mid_cloud_is_safe() {
        let (bank, sources) = shared();
        let env = GrainEnv {
            bank,
            sources,
            sample_rate: SR,
        };
        let mut cloud = GrainCloud::new();
        let mut rng = Rng::new(5);
        let base = GrainSettings::new(1.0, 80.0, 300.0, 30.0, 0.5, 1.0, 0.0);
        let mut phase = 0.0f32;
        for i in 0..96_000 {
            // flip the source every quarter second while grains are sounding
            let s = base.with_source((i / 12_000) % 5);
            let (l, r) = cloud.next(&env, 0.4, &s, 330.0, phase, &mut rng);
            assert!(l.is_finite() && r.is_finite());
            phase = (phase + 330.0 / SR).fract();
        }
    }

    #[test]
    fn texture_without_sources_does_nothing_instead_of_crashing() {
        let (bank, _) = shared();
        let empty = SourceBank::empty();
        let env = GrainEnv {
            bank,
            sources: &empty,
            sample_rate: SR,
        };
        let mut cloud = GrainCloud::new();
        let mut rng = Rng::new(6);
        let s = GrainSettings::new(1.0, 50.0, 100.0, 10.0, 0.3, 0.5, 0.0).with_source(2);
        for _ in 0..20_000 {
            let (l, r) = cloud.next(&env, 0.2, &s, 220.0, 0.0, &mut rng);
            assert_eq!((l, r), (0.0, 0.0));
        }
        assert_eq!(cloud.active_count(), 0);
    }
}

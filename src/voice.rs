//! One polyphonic voice: wavetable oscillator, grain cloud, stereo filter, amp
//! and filter envelopes, stereo position and its own drift streams.
//!
//! Signal path: oscillator and grain cloud (crossfaded) -> filter (left and
//! right copies) -> amp -> pan.

use crate::drift::{Drift, DriftStep};
use crate::env::{Adsr, EnvCoefs};
use crate::filter::{soft_clip, FilterType, Svf, SvfCoefs};
use crate::granular::{GrainCloud, GrainEnv, GrainSettings};
use crate::rng::Rng;
use crate::sources::SourceBank;
use crate::wavetable::WavetableBank;
use std::f32::consts::FRAC_PI_4;

/// Largest random detune at Variation = 100%, in cents either way.
const MAX_DETUNE_CENTS: f32 = 10.0;
/// Fixed headroom per voice so a full chord does not clip at the default gain.
const VOICE_HEADROOM: f32 = 0.5;

// What a drift stream at full scale (+-1) does when Drift = 100%.
const DRIFT_PITCH_CENTS: f32 = 15.0;
const DRIFT_POSITION: f32 = 0.10;
const DRIFT_LEVEL_DB: f32 = 1.5;
const DRIFT_CUTOFF_OCT: f32 = 0.75;
/// Filter Width = 100% offsets the left and right cutoffs by this many octaves.
const WIDTH_OCT: f32 = 0.33;

/// Indices into the voice's drift streams.
const D_PITCH: usize = 0;
const D_POSITION: usize = 1;
const D_LEVEL: usize = 2;
const D_CUTOFF: usize = 3;
/// Moves the left and right cutoffs in opposite directions.
const D_CUTOFF_SPREAD: usize = 4;
const NUM_STREAMS: usize = 5;

/// With a moving filter envelope the filter is recomputed this often (samples).
const FILTER_CHUNK: usize = 16;

/// Everything a voice needs to render one block. Built once per block.
pub struct BlockCtx<'a> {
    pub bank: &'a WavetableBank,
    pub sources: &'a SourceBank,
    pub sample_rate: f32,
    pub env: EnvCoefs,
    pub filter_type: FilterType,
    pub cutoff: f32,
    pub resonance: f32,
    pub key_track: f32,
    pub filter_width: f32,
    /// Filter envelope settings and how many octaves it moves the cutoff.
    pub fenv: EnvCoefs,
    pub fenv_amount: f32,
    pub grain: GrainSettings,
    pub drift_amount: f32,
    pub drift_step: DriftStep,
}

pub struct Voice {
    pub active: bool,
    pub note: u8,
    pub channel: u8,
    pub voice_id: Option<i32>,
    /// Higher means started later. Used to steal the oldest voice first.
    pub age: u64,

    phase: f32,
    base_freq: f32,
    /// log2 of the note's frequency relative to middle C, for key tracking.
    octaves_from_c4: f32,
    level: usize,
    velocity: f32,
    gain_l: f32,
    gain_r: f32,
    env: Adsr,
    fenv: Adsr,
    cloud: GrainCloud,
    filter_l: Svf,
    filter_r: Svf,
    drift: [Drift; NUM_STREAMS],
    rng: Rng,
}

fn midi_note_to_freq(note: u8) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
}

impl Voice {
    pub fn new(index: usize) -> Self {
        // A different, fixed seed per voice, so each voice has its own streams.
        let seed = 0x9E37_79B9u32.wrapping_mul(index as u32 + 1) ^ 0xA5A5_1234;
        Self {
            active: false,
            note: 0,
            channel: 0,
            voice_id: None,
            age: 0,
            phase: 0.0,
            base_freq: 440.0,
            octaves_from_c4: 0.0,
            level: 0,
            velocity: 0.0,
            gain_l: FRAC_PI_4.cos(),
            gain_r: FRAC_PI_4.sin(),
            env: Adsr::new(),
            fenv: Adsr::new(),
            cloud: GrainCloud::new(),
            filter_l: Svf::default(),
            filter_r: Svf::default(),
            drift: [Drift::default(); NUM_STREAMS],
            rng: Rng::new(seed),
        }
    }

    /// Start a note. `variation` (0..1) scales the per-note random differences:
    /// fine detune, start phase and stereo position.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        note: u8,
        channel: u8,
        voice_id: Option<i32>,
        velocity: f32,
        variation: f32,
        sample_rate: f32,
        age: u64,
    ) {
        let cents = self.rng.bipolar() * variation * MAX_DETUNE_CENTS;
        let freq = midi_note_to_freq(note) * 2f32.powf(cents / 1200.0);

        // Constant-power pan from a random position scaled by Variation.
        let pan = self.rng.bipolar() * variation;
        let angle = (pan + 1.0) * FRAC_PI_4;

        self.active = true;
        self.note = note;
        self.channel = channel;
        self.voice_id = voice_id;
        self.age = age;
        self.phase = self.rng.unipolar() * variation;
        self.base_freq = freq;
        self.octaves_from_c4 = (freq / 261.626).log2();
        // A little margin above the note so drift can never push harmonics
        // past Nyquist for the chosen table.
        self.level = WavetableBank::level_for_frequency(freq * 1.02, sample_rate);
        self.velocity = velocity.clamp(0.0, 1.0);
        self.gain_l = angle.cos();
        self.gain_r = angle.sin();

        self.filter_l.reset();
        self.filter_r.reset();
        for d in self.drift.iter_mut() {
            d.randomize(&mut self.rng);
        }
        self.cloud.reset();
        self.env.trigger();
        self.fenv.trigger();
    }

    pub fn release(&mut self) {
        self.env.release();
        self.fenv.release();
    }

    /// Stop at once. A short fade would be gentler; planned for a later phase.
    pub fn choke(&mut self) {
        self.env.kill();
        self.fenv.kill();
        self.active = false;
    }

    pub fn is_releasing(&self) -> bool {
        self.env.is_releasing()
    }

    /// Add this voice to the output buffers.
    ///
    /// `position` and `gain` hold one smoothed value per sample, and all four
    /// slices share the same length.
    pub fn render(
        &mut self,
        ctx: &BlockCtx,
        position: &[f32],
        gain: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        debug_assert_eq!(position.len(), out_l.len());
        debug_assert_eq!(gain.len(), out_l.len());
        debug_assert_eq!(out_r.len(), out_l.len());

        // Advance every drift stream once per block.
        let amt = ctx.drift_amount;
        let mut d = [0.0f32; NUM_STREAMS];
        for (value, stream) in d.iter_mut().zip(self.drift.iter_mut()) {
            *value = stream.step(&ctx.drift_step, &mut self.rng);
        }

        let freq = self.base_freq * 2f32.powf(d[D_PITCH] * amt * DRIFT_PITCH_CENTS / 1200.0);
        let phase_inc = freq / ctx.sample_rate;
        let position_offset = d[D_POSITION] * amt * DRIFT_POSITION;
        let level_gain = 10f32.powf(d[D_LEVEL] * amt * DRIFT_LEVEL_DB / 20.0);

        // Cutoff in octaves: key tracking and common drift move both channels,
        // the spread stream and Filter Width push left and right apart.
        let common = ctx.key_track * self.octaves_from_c4 + d[D_CUTOFF] * amt * DRIFT_CUTOFF_OCT;
        let spread =
            d[D_CUTOFF_SPREAD] * amt * DRIFT_CUTOFF_OCT * 0.5 + ctx.filter_width * WIDTH_OCT;
        let make_coefs = |env_oct: f32| {
            (
                SvfCoefs::new(
                    ctx.cutoff * 2f32.powf(common + env_oct + spread),
                    ctx.resonance,
                    ctx.sample_rate,
                ),
                SvfCoefs::new(
                    ctx.cutoff * 2f32.powf(common + env_oct - spread),
                    ctx.resonance,
                    ctx.sample_rate,
                ),
            )
        };
        let (mut coefs_l, mut coefs_r) = make_coefs(0.0);
        let env_moves_filter = ctx.fenv_amount.abs() > 1e-3;

        let grain_on = ctx.grain.mix > 0.0005;
        if !grain_on {
            self.cloud.reset();
        }
        let wet = ctx.grain.mix;
        let dry = 1.0 - wet;
        let grain_env = GrainEnv {
            bank: ctx.bank,
            sources: ctx.sources,
            sample_rate: ctx.sample_rate,
        };

        for i in 0..out_l.len() {
            let amp = self.env.next(&ctx.env);
            let filter_env = self.fenv.next(&ctx.fenv);
            if self.env.is_idle() {
                self.active = false;
                break;
            }
            if env_moves_filter && i % FILTER_CHUNK == 0 {
                (coefs_l, coefs_r) = make_coefs(filter_env * ctx.fenv_amount);
            }

            let position_now = position[i] + position_offset;
            let osc = ctx.bank.sample(self.phase, position_now.clamp(0.0, 1.0), self.level);

            let (mix_l, mix_r) = if grain_on {
                let (gl, gr) = self.cloud.next(
                    &grain_env,
                    position_now,
                    &ctx.grain,
                    freq,
                    self.phase,
                    &mut self.rng,
                );
                (osc * dry + gl * wet, osc * dry + gr * wet)
            } else {
                (osc, osc)
            };

            let fl = soft_clip(self.filter_l.process(mix_l, &coefs_l, ctx.filter_type));
            let fr = soft_clip(self.filter_r.process(mix_r, &coefs_r, ctx.filter_type));

            let a = amp * self.velocity * gain[i] * level_gain * VOICE_HEADROOM;
            out_l[i] += fl * a * self.gain_l;
            out_r[i] += fr * a * self.gain_r;

            self.phase += phase_inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    const BLOCK: usize = 64;

    struct Setup {
        bank: &'static WavetableBank,
        sources: &'static SourceBank,
        drift: f32,
        width: f32,
        cutoff: f32,
        grain: GrainSettings,
        fenv_amount: f32,
    }

    impl Setup {
        fn new() -> Self {
            let (bank, sources) = crate::sources::test_support::shared();
            Self {
                bank,
                sources,
                drift: 0.0,
                width: 0.0,
                cutoff: 12_000.0,
                grain: GrainSettings::off(),
                fenv_amount: 0.0,
            }
        }

        fn ctx(&self) -> BlockCtx<'_> {
            BlockCtx {
                bank: self.bank,
                sources: self.sources,
                sample_rate: SR,
                env: EnvCoefs::new(0.005, 0.1, 0.8, 0.1, SR),
                filter_type: FilterType::LowPass,
                cutoff: self.cutoff,
                resonance: 0.1,
                key_track: 0.5,
                filter_width: self.width,
                fenv: EnvCoefs::new(0.005, 0.3, 0.2, 0.2, SR),
                fenv_amount: self.fenv_amount,
                grain: self.grain,
                drift_amount: self.drift,
                drift_step: DriftStep::new(1.0, BLOCK, SR),
            }
        }

        /// Render `blocks` blocks of one note and return (left, right).
        fn render(&self, variation: f32, index: usize, blocks: usize) -> (Vec<f32>, Vec<f32>) {
            let ctx = self.ctx();
            let mut voice = Voice::new(index);
            voice.start(60, 0, None, 1.0, variation, SR, 0);
            let (mut l, mut r) = (Vec::new(), Vec::new());
            let position = [0.3; BLOCK];
            let gain = [0.5; BLOCK];
            for _ in 0..blocks {
                let mut bl = [0.0; BLOCK];
                let mut br = [0.0; BLOCK];
                voice.render(&ctx, &position, &gain, &mut bl, &mut br);
                l.extend_from_slice(&bl);
                r.extend_from_slice(&br);
            }
            (l, r)
        }
    }

    #[test]
    fn centred_when_nothing_is_random_or_wide() {
        let (l, r) = Setup::new().render(0.0, 0, 75);
        assert!(l.iter().any(|s| s.abs() > 0.01), "voice should make sound");
        for (a, b) in l.iter().zip(&r) {
            assert!((a - b).abs() < 1e-6, "left and right must match");
        }
    }

    #[test]
    fn variation_moves_voice_off_centre() {
        let (l, r) = Setup::new().render(1.0, 3, 75);
        let diff: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 1.0, "stereo difference expected, got {diff}");
        assert!(l.iter().chain(&r).all(|s| s.is_finite()));
    }

    #[test]
    fn filter_width_alone_makes_a_stereo_difference() {
        let mut setup = Setup::new();
        setup.cutoff = 1_500.0;
        setup.width = 1.0;
        let (l, r) = setup.render(0.0, 0, 75);
        let diff: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 1.0, "width should split the channels, got {diff}");
    }

    #[test]
    fn drift_changes_the_sound_over_time_and_is_repeatable() {
        let mut setup = Setup::new();
        setup.cutoff = 2_000.0;
        let (still, _) = setup.render(0.0, 1, 150);
        setup.drift = 1.0;
        let (a, _) = setup.render(0.0, 1, 150);
        let (b, _) = setup.render(0.0, 1, 150);
        let diff: f32 = still.iter().zip(&a).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 1.0, "drift should alter the sound, got {diff}");
        assert_eq!(a, b, "same voice, same seed, same result");
        assert!(a.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn different_voices_drift_differently() {
        let mut setup = Setup::new();
        setup.drift = 1.0;
        setup.cutoff = 2_000.0;
        let (a, _) = setup.render(0.0, 0, 150);
        let (b, _) = setup.render(0.0, 5, 150);
        let diff: f32 = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 1.0, "voices must not move in lockstep, got {diff}");
    }

    #[test]
    fn grain_cloud_adds_width_and_changes_the_sound() {
        let mut setup = Setup::new();
        let (dry_l, dry_r) = setup.render(0.0, 2, 150);
        setup.grain = GrainSettings::new(1.0, 30.0, 80.0, 12.0, 0.3, 1.0, 0.0);
        let (wet_l, wet_r) = setup.render(0.0, 2, 150);

        let dry_width: f32 = dry_l.iter().zip(&dry_r).map(|(a, b)| (a - b).abs()).sum();
        let wet_width: f32 = wet_l.iter().zip(&wet_r).map(|(a, b)| (a - b).abs()).sum();
        let change: f32 = dry_l.iter().zip(&wet_l).map(|(a, b)| (a - b).abs()).sum();
        assert!(dry_width < 1e-3);
        assert!(wet_width > 5.0, "cloud should be wide, got {wet_width}");
        assert!(change > 5.0);
        assert!(wet_l.iter().chain(&wet_r).all(|s| s.is_finite()));
    }

    #[test]
    fn texture_grains_change_the_sound_and_stay_wide() {
        let mut setup = Setup::new();
        setup.grain = GrainSettings::new(1.0, 30.0, 120.0, 12.0, 0.3, 1.0, 0.0);
        let (wave_l, _) = setup.render(0.0, 2, 150);
        setup.grain = setup.grain.with_source(3);
        let (tex_l, tex_r) = setup.render(0.0, 2, 150);

        let change: f32 = wave_l.iter().zip(&tex_l).map(|(a, b)| (a - b).abs()).sum();
        let width: f32 = tex_l.iter().zip(&tex_r).map(|(a, b)| (a - b).abs()).sum();
        assert!(change > 5.0, "texture should sound different, got {change}");
        assert!(width > 5.0, "texture cloud should be wide, got {width}");
        assert!(tex_l.iter().chain(&tex_r).all(|s| s.is_finite()));
    }

    #[test]
    fn grain_mix_zero_equals_no_grains() {
        let mut setup = Setup::new();
        let (a, _) = setup.render(0.0, 1, 100);
        setup.grain = GrainSettings::new(0.0, 80.0, 200.0, 50.0, 1.0, 1.0, 1.0);
        let (b, _) = setup.render(0.0, 1, 100);
        assert_eq!(a, b);
    }

    #[test]
    fn filter_envelope_opens_the_filter() {
        let mut setup = Setup::new();
        setup.cutoff = 400.0;
        let (closed, _) = setup.render(0.0, 0, 20);
        setup.fenv_amount = 3.0;
        let (opened, _) = setup.render(0.0, 0, 20);

        // High-frequency content: the second difference weights highs heavily.
        let roughness = |v: &[f32]| {
            v.windows(3)
                .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
                .sum::<f32>()
        };
        assert!(
            roughness(&opened) > roughness(&closed) * 1.5,
            "{} vs {}",
            roughness(&opened),
            roughness(&closed)
        );
        assert!(opened.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn released_voice_becomes_inactive() {
        let setup = Setup::new();
        let mut ctx = setup.ctx();
        ctx.env = EnvCoefs::new(0.001, 0.01, 0.8, 0.02, SR);
        let mut voice = Voice::new(0);
        voice.start(72, 0, None, 1.0, 0.5, SR, 0);
        voice.release();

        let position = [0.0; BLOCK];
        let gain = [1.0; BLOCK];
        for _ in 0..750 {
            let mut bl = [0.0; BLOCK];
            let mut br = [0.0; BLOCK];
            voice.render(&ctx, &position, &gain, &mut bl, &mut br);
        }
        assert!(!voice.active);
    }
}

#[cfg(test)]
mod level_tests {
    use super::*;

    const SR: f32 = 48_000.0;
    const BLOCK: usize = 64;

    fn chord_peak(resonance: f32, cutoff: f32, source: usize) -> f32 {
        let (bank, sources) = crate::sources::test_support::shared();
        let ctx = BlockCtx {
            bank,
            sources,
            sample_rate: SR,
            env: EnvCoefs::new(0.005, 0.2, 0.9, 0.2, SR),
            filter_type: FilterType::LowPass,
            cutoff,
            resonance,
            key_track: 0.5,
            filter_width: 0.25,
            fenv: EnvCoefs::new(0.005, 0.3, 0.2, 0.2, SR),
            fenv_amount: 1.0,
            grain: GrainSettings::new(0.5, 30.0, 80.0, 12.0, 0.3, 0.7, 0.0).with_source(source),
            drift_amount: 0.35,
            drift_step: DriftStep::new(1.0, BLOCK, SR),
        };
        let notes = [36u8, 43, 48, 52, 55, 60, 64, 67];
        let mut voices: Vec<Voice> = (0..notes.len()).map(Voice::new).collect();
        for (v, n) in voices.iter_mut().zip(notes) {
            v.start(n, 0, None, 1.0, 0.3, SR, 0);
        }
        let position = [0.35; BLOCK];
        let gain = [0.25; BLOCK]; // default -12 dB
        let mut peak = 0.0f32;
        for _ in 0..750 {
            let mut l = [0.0; BLOCK];
            let mut r = [0.0; BLOCK];
            for v in voices.iter_mut() {
                v.render(&ctx, &position, &gain, &mut l, &mut r);
            }
            peak = l.iter().chain(&r).fold(peak, |m, s| m.max(s.abs()));
        }
        peak
    }

    #[test]
    fn eight_note_chord_leaves_headroom_at_default_gain() {
        let normal = chord_peak(0.1, 12_000.0, 0);
        let screaming = chord_peak(1.0, 1_500.0, 0);
        println!("peak normal {normal:.3}, peak max resonance {screaming:.3}");
        assert!(normal < 0.9, "normal patch peaks at {normal}");
        assert!(screaming < 2.0, "resonant patch peaks at {screaming}");

        for source in 1..=4 {
            let peak = chord_peak(0.1, 12_000.0, source);
            println!("texture {source}: peak {peak:.3}");
            assert!(peak < 1.2, "texture {source} peaks at {peak}");
        }
    }
}

#[cfg(test)]
mod cpu_bench {
    use super::*;
    use std::time::Instant;

    const SR: f32 = 48_000.0;
    const BLOCK: usize = 64;

    /// Run with: cargo test --release cpu_budget -- --ignored --nocapture
    #[test]
    #[ignore]
    fn cpu_budget_full_polyphony() {
        let (bank, sources) = crate::sources::test_support::shared();
        for (name, grain) in [
            ("no grains", GrainSettings::new(0.0, 30.0, 80.0, 12.0, 0.3, 0.7, 0.0)),
            ("default grains", GrainSettings::new(0.35, 30.0, 80.0, 12.0, 0.3, 0.7, 0.0)),
            (
                "default texture grains",
                GrainSettings::new(0.35, 30.0, 80.0, 12.0, 0.3, 0.7, 0.0).with_source(1),
            ),
            ("worst case, pool full", GrainSettings::new(1.0, 100.0, 500.0, 50.0, 0.5, 1.0, 1.0)),
            (
                "worst case, textures",
                GrainSettings::new(1.0, 100.0, 500.0, 50.0, 0.5, 1.0, 1.0).with_source(1),
            ),
        ] {
            let ctx = BlockCtx {
                bank,
                sources,
                sample_rate: SR,
                env: EnvCoefs::new(0.005, 0.2, 1.0, 0.2, SR),
                filter_type: FilterType::LowPass,
                cutoff: 4_000.0,
                resonance: 0.3,
                key_track: 0.5,
                filter_width: 0.25,
                fenv: EnvCoefs::new(0.005, 0.3, 0.3, 0.2, SR),
                fenv_amount: 2.0,
                grain,
                drift_amount: 0.35,
                drift_step: DriftStep::new(1.0, BLOCK, SR),
            };
            let mut voices: Vec<Voice> = (0..16).map(Voice::new).collect();
            for (i, v) in voices.iter_mut().enumerate() {
                v.start(36 + i as u8 * 3, 0, None, 1.0, 0.3, SR, i as u64);
            }
            let position = [0.35; BLOCK];
            let gain = [0.25; BLOCK];
            let seconds = 10;
            let blocks = (SR as usize * seconds) / BLOCK;
            let start = Instant::now();
            for _ in 0..blocks {
                let mut l = [0.0; BLOCK];
                let mut r = [0.0; BLOCK];
                for v in voices.iter_mut() {
                    v.render(&ctx, &position, &gain, &mut l, &mut r);
                }
            }
            let used = start.elapsed().as_secs_f32() / seconds as f32;
            println!("16 voices, {name}: {:.1}% of one core", used * 100.0);
        }
    }
}

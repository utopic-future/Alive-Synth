//! Alive Synth, phase 4: a polyphonic wavetable and granular synth with built-in
//! grain textures, a stereo filter, a filter envelope and per-voice
//! analog-style drift.
//!
//! Later phases add unison, effects and the tabbed interface. See the project
//! plan for the order.

mod drift;
mod env;
mod filter;
mod granular;
mod rng;
mod sources;
mod voice;
mod wavetable;

use drift::DriftStep;
use env::EnvCoefs;
use filter::FilterType;
use granular::GrainSettings;
use sources::{GrainSource, SourceBank};
use nih_plug::prelude::*;
use std::sync::Arc;
use voice::{BlockCtx, Voice};
use wavetable::WavetableBank;

const NUM_VOICES: usize = 16;
/// Events are handled between sub-blocks of at most this many samples.
const MAX_BLOCK_SIZE: usize = 64;

struct AliveSynth {
    params: Arc<AliveParams>,
    bank: WavetableBank,
    sources: SourceBank,
    voices: [Voice; NUM_VOICES],
    sample_rate: f32,
    next_age: u64,
}

#[derive(Params)]
struct AliveParams {
    #[id = "gain"]
    gain: FloatParam,

    /// Morphs through the built-in wavetable frames.
    #[id = "pos"]
    position: FloatParam,

    #[id = "atk"]
    attack: FloatParam,
    #[id = "dec"]
    decay: FloatParam,
    #[id = "sus"]
    sustain: FloatParam,
    #[id = "rel"]
    release: FloatParam,

    /// Scales the random differences between notes: fine detune, start phase
    /// and stereo position.
    #[id = "var"]
    variation: FloatParam,

    #[id = "ftype"]
    filter_type: EnumParam<FilterType>,
    #[id = "cut"]
    cutoff: FloatParam,
    #[id = "res"]
    resonance: FloatParam,
    /// How much the cutoff follows the note pitch.
    #[id = "ktrk"]
    key_track: FloatParam,
    /// Offsets the left and right cutoffs in opposite directions.
    #[id = "fwid"]
    filter_width: FloatParam,

    #[id = "fatk"]
    filter_attack: FloatParam,
    #[id = "fdec"]
    filter_decay: FloatParam,
    #[id = "fsus"]
    filter_sustain: FloatParam,
    #[id = "frel"]
    filter_release: FloatParam,
    /// How many octaves the filter envelope moves the cutoff (negative closes it).
    #[id = "famt"]
    filter_env_amount: FloatParam,

    /// 0 = wavetable only, 100 = grain cloud only.
    #[id = "gmix"]
    grain_mix: FloatParam,
    /// What the grains read: the wavetable or one of the built-in textures.
    #[id = "gsrc"]
    grain_source: EnumParam<GrainSource>,
    /// New grains per second, per voice.
    #[id = "gdens"]
    grain_density: FloatParam,
    #[id = "gsize"]
    grain_size: FloatParam,
    /// Random pitch offset of each grain, in cents either way.
    #[id = "gpitch"]
    grain_pitch_spread: FloatParam,
    /// Random wavetable position offset of each grain.
    #[id = "gscat"]
    grain_scatter: FloatParam,
    #[id = "gpan"]
    grain_pan_spread: FloatParam,
    /// 0 = soft Hann-like grains, 100 = nearly flat grains with short fades.
    #[id = "gshape"]
    grain_shape: FloatParam,

    /// How far the pitch, cutoff, position and level wander.
    #[id = "drift"]
    drift: FloatParam,
    /// How fast they wander.
    #[id = "drate"]
    drift_rate: FloatParam,
}

impl Default for AliveSynth {
    fn default() -> Self {
        Self {
            params: Arc::new(AliveParams::default()),
            // Built in initialize(), because it allocates and takes a moment.
            bank: WavetableBank::empty(),
            sources: SourceBank::empty(),
            voices: std::array::from_fn(Voice::new),
            sample_rate: 44_100.0,
            next_age: 0,
        }
    }
}

impl Default for AliveParams {
    fn default() -> Self {
        let time_range = FloatRange::Skewed {
            min: 0.001,
            max: 8.0,
            factor: FloatRange::skew_factor(-2.0),
        };

        Self {
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(-12.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(0.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 0.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            position: FloatParam::new("Position", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(30.0))
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            attack: FloatParam::new("Attack", 0.005, time_range)
                .with_unit(" s")
                .with_value_to_string(formatters::v2s_f32_rounded(3)),
            decay: FloatParam::new("Decay", 0.3, time_range)
                .with_unit(" s")
                .with_value_to_string(formatters::v2s_f32_rounded(3)),
            sustain: FloatParam::new("Sustain", 0.7, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            release: FloatParam::new("Release", 0.4, time_range)
                .with_unit(" s")
                .with_value_to_string(formatters::v2s_f32_rounded(3)),

            variation: FloatParam::new("Variation", 0.3, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            filter_type: EnumParam::new("Filter Type", FilterType::LowPass),
            cutoff: FloatParam::new(
                "Cutoff",
                12_000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20_000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(30.0))
            // the formatter already adds Hz or kHz
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(1))
            .with_string_to_value(formatters::s2v_f32_hz_then_khz()),
            resonance: FloatParam::new("Resonance", 0.1, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            key_track: FloatParam::new("Key Track", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            filter_width: FloatParam::new(
                "Filter Width",
                0.25,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            filter_attack: FloatParam::new("Filter Attack", 0.005, time_range)
                .with_unit(" s")
                .with_value_to_string(formatters::v2s_f32_rounded(3)),
            filter_decay: FloatParam::new("Filter Decay", 0.3, time_range)
                .with_unit(" s")
                .with_value_to_string(formatters::v2s_f32_rounded(3)),
            filter_sustain: FloatParam::new(
                "Filter Sustain",
                0.3,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            filter_release: FloatParam::new("Filter Release", 0.4, time_range)
                .with_unit(" s")
                .with_value_to_string(formatters::v2s_f32_rounded(3)),
            filter_env_amount: FloatParam::new(
                "Filter Env Amount",
                0.0,
                FloatRange::Linear { min: -4.0, max: 4.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_unit(" oct")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            grain_mix: FloatParam::new("Grain Mix", 0.35, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(30.0))
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            grain_source: EnumParam::new("Grain Source", GrainSource::Wavetable),
            grain_density: FloatParam::new(
                "Grain Density",
                30.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 100.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" /s")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            grain_size: FloatParam::new(
                "Grain Size",
                80.0,
                FloatRange::Skewed {
                    min: 5.0,
                    max: 500.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            grain_pitch_spread: FloatParam::new(
                "Grain Pitch Spread",
                12.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 1200.0,
                    factor: FloatRange::skew_factor(-3.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" ct")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            grain_scatter: FloatParam::new(
                "Grain Scatter",
                0.3,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            grain_pan_spread: FloatParam::new(
                "Grain Pan Spread",
                0.7,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            grain_shape: FloatParam::new("Grain Shape", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(50.0))
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            drift: FloatParam::new("Drift", 0.35, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(100.0))
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            drift_rate: FloatParam::new(
                "Drift Rate",
                1.0,
                FloatRange::Skewed {
                    min: 0.05,
                    max: 4.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" x")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
        }
    }
}

impl AliveSynth {
    fn start_voice(&mut self, voice_id: Option<i32>, channel: u8, note: u8, velocity: f32) {
        let variation = self.params.variation.value();
        let age = self.next_age;
        self.next_age += 1;

        // Prefer a free voice. Otherwise steal the oldest releasing voice, then
        // the oldest voice overall.
        let index = self
            .voices
            .iter()
            .position(|v| !v.active)
            .or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.is_releasing())
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)
            })
            .or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)
            })
            .unwrap_or(0);

        self.voices[index].start(
            note,
            channel,
            voice_id,
            velocity,
            variation,
            self.sample_rate,
            age,
        );
    }

    fn matches(v: &Voice, voice_id: Option<i32>, channel: u8, note: u8) -> bool {
        v.active
            && ((voice_id.is_some() && v.voice_id == voice_id)
                || (v.channel == channel && v.note == note))
    }

    fn release_voices(&mut self, voice_id: Option<i32>, channel: u8, note: u8) {
        for v in self.voices.iter_mut() {
            if Self::matches(v, voice_id, channel, note) && !v.is_releasing() {
                v.release();
            }
        }
    }

    fn choke_voices(&mut self, voice_id: Option<i32>, channel: u8, note: u8) {
        for v in self.voices.iter_mut() {
            if Self::matches(v, voice_id, channel, note) {
                v.choke();
            }
        }
    }
}

impl Plugin for AliveSynth {
    const NAME: &'static str = "Alive Synth";
    const VENDOR: &'static str = "Alive Synth Project";
    const URL: &'static str = "https://example.com";
    const EMAIL: &'static str = "info@example.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        // The wavetables do not depend on the sample rate, the textures do.
        if self.bank.is_empty() {
            self.bank = WavetableBank::new();
        }
        if self.sources.is_empty() || self.sources.sample_rate() != self.sample_rate {
            self.sources = SourceBank::new(self.sample_rate, &self.bank);
        }
        true
    }

    fn reset(&mut self) {
        for v in self.voices.iter_mut() {
            v.choke();
        }
        self.next_age = 0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let num_samples = buffer.samples();
        let output = buffer.as_slice();

        let mut next_event = context.next_event();
        let mut block_start: usize = 0;
        let mut block_end: usize = MAX_BLOCK_SIZE.min(num_samples);

        while block_start < num_samples {
            // Apply every event that is due, and shorten the block so the next
            // event lands exactly on a block boundary (sample accurate timing).
            'events: loop {
                match next_event {
                    Some(event) if (event.timing() as usize) <= block_start => {
                        match event {
                            NoteEvent::NoteOn {
                                voice_id,
                                channel,
                                note,
                                velocity,
                                ..
                            } => self.start_voice(voice_id, channel, note, velocity),
                            NoteEvent::NoteOff {
                                voice_id,
                                channel,
                                note,
                                ..
                            } => self.release_voices(voice_id, channel, note),
                            NoteEvent::Choke {
                                voice_id,
                                channel,
                                note,
                                ..
                            } => self.choke_voices(voice_id, channel, note),
                            _ => {}
                        }
                        next_event = context.next_event();
                    }
                    Some(event) if (event.timing() as usize) < block_end => {
                        block_end = event.timing() as usize;
                        break 'events;
                    }
                    _ => break 'events,
                }
            }

            let block_len = block_end - block_start;

            let mut position = [0.0f32; MAX_BLOCK_SIZE];
            let mut gain = [0.0f32; MAX_BLOCK_SIZE];
            self.params
                .position
                .smoothed
                .next_block(&mut position, block_len);
            self.params.gain.smoothed.next_block(&mut gain, block_len);

            // Per-block settings. Smoothed values advance by one block's worth
            // of samples, so automation still glides.
            let steps = block_len as u32;
            let ctx = BlockCtx {
                bank: &self.bank,
                sources: &self.sources,
                sample_rate: self.sample_rate,
                env: EnvCoefs::new(
                    self.params.attack.value(),
                    self.params.decay.value(),
                    self.params.sustain.value(),
                    self.params.release.value(),
                    self.sample_rate,
                ),
                filter_type: self.params.filter_type.value(),
                cutoff: self.params.cutoff.smoothed.next_step(steps),
                resonance: self.params.resonance.smoothed.next_step(steps),
                key_track: self.params.key_track.value(),
                filter_width: self.params.filter_width.smoothed.next_step(steps),
                fenv: EnvCoefs::new(
                    self.params.filter_attack.value(),
                    self.params.filter_decay.value(),
                    self.params.filter_sustain.value(),
                    self.params.filter_release.value(),
                    self.sample_rate,
                ),
                fenv_amount: self.params.filter_env_amount.smoothed.next_step(steps),
                grain: GrainSettings::new(
                    self.params.grain_mix.smoothed.next_step(steps),
                    self.params.grain_density.smoothed.next_step(steps),
                    self.params.grain_size.smoothed.next_step(steps),
                    self.params.grain_pitch_spread.smoothed.next_step(steps),
                    self.params.grain_scatter.smoothed.next_step(steps),
                    self.params.grain_pan_spread.smoothed.next_step(steps),
                    self.params.grain_shape.smoothed.next_step(steps),
                )
                .with_source(self.params.grain_source.value().index()),
                drift_amount: self.params.drift.smoothed.next_step(steps),
                drift_step: DriftStep::new(
                    self.params.drift_rate.value(),
                    block_len,
                    self.sample_rate,
                ),
            };

            let (left, right) = output.split_at_mut(1);
            let out_l = &mut left[0][block_start..block_end];
            let out_r = &mut right[0][block_start..block_end];
            out_l.fill(0.0);
            out_r.fill(0.0);

            for voice in self.voices.iter_mut().filter(|v| v.active) {
                voice.render(&ctx, &position[..block_len], &gain[..block_len], out_l, out_r);
            }

            block_start = block_end;
            block_end = (block_start + MAX_BLOCK_SIZE).min(num_samples);
        }

        ProcessStatus::Normal
    }
}

impl Vst3Plugin for AliveSynth {
    // Exactly 16 bytes. Keep it unique and never change it after release.
    const VST3_CLASS_ID: [u8; 16] = *b"AliveSynthRustV1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Synth,
        Vst3SubCategory::Stereo,
    ];
}

nih_export_vst3!(AliveSynth);

#[cfg(test)]
mod docs {
    use super::*;

    /// Prints the parameter table used in docs/PARAMETERS.md.
    /// Run with: cargo test dump_parameters -- --ignored --nocapture
    #[test]
    #[ignore]
    fn dump_parameters() {
        let params = AliveParams::default();
        println!("| ID | Name | Min | Max | Default |");
        println!("| --- | --- | --- | --- | --- |");
        for (id, ptr, _group) in params.param_map() {
            // ParamPtr methods are unsafe because the pointer must stay valid.
            // It does, because `params` outlives this loop.
            unsafe {
                println!(
                    "| `{}` | {} | {} | {} | {} |",
                    id,
                    ptr.name(),
                    ptr.normalized_value_to_string(0.0, true),
                    ptr.normalized_value_to_string(1.0, true),
                    ptr.normalized_value_to_string(ptr.default_normalized_value(), true),
                );
            }
        }
    }
}

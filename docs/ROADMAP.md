# Roadmap

Tick items off here when they are done, and keep `HANDOFF.md` in sync.

## Done

- [x] Phase 1: wavetable bank, polyphony, ADSR, stereo output, Variation
- [x] Phase 2: stereo filter (low, band, high), Filter Width, Key Track, drift system
- [x] Phase 3: granular engine, Grain Mix, filter envelope
- [x] Phase 4: four built-in grain textures, Grain Source selector, build-time and CPU measurements

## Next: phase 5, unison and effects

- [ ] Unison: voice count, detune curve, stereo spread and phase spread as separate controls
- [ ] Ensemble or chorus with width
- [ ] Delay with width
- [ ] Reverb with width
- [ ] Master width, plus a correlation value for the meter
- [ ] Tests: unison stays in tune at zero detune, effects stay finite and bounded, mono sum of a
      wide patch does not collapse (measure the level loss)
- [ ] Re-measure CPU with unison and effects (`cargo test --release cpu_budget -- --ignored --nocapture`)

## Phase 6: tabbed GUI

- [ ] Decide the toolkit: `nih_plug_vizia` (mature, suits tabs) or `nih_plug_egui` (faster to prototype)
- [ ] Five tabs: Sound, Filter and Amp, Life, Mod, FX. Fixed header: preset name, master volume,
      output meter, correlation meter, mono-sum button
- [ ] Titled sections, knobs grouped under them, basic calm colors, visible defaults
- [ ] Waveform or texture display with the grain position marker on the Sound tab

## Phase 7: modulation and presets

- [ ] LFOs and extra envelopes, modulation matrix
- [ ] Preset save and load, factory presets that show off drift, grains and width
- [ ] Velocity and note-number routing

## Phase 8: sample loading (optional)

- [ ] Load WAV (and maybe FLAC) with a decoder crate, on a background thread, then swap the buffer in
- [ ] Length cap (30 to 60 seconds), resample to the host rate, band-limited levels like the textures
- [ ] Store the path (or short samples) in the plugin state
- [ ] Waveform display with position marker

## Polish and quality

- [ ] Short fade when a voice is stolen or choked
- [ ] Cubic interpolation for wavetable and grain reads
- [ ] Half-octave mip steps so low notes keep their top harmonics
- [ ] Listen in a DAW and tune every default: drift amounts, grain defaults, filter ranges
- [ ] Stereo texture sources (optional)
- [ ] Aftertouch or MPE reaction (optional, from the "alive" ideas)
- [ ] Replace the placeholder vendor, URL, email and VST3 class ID, decide the license
- [ ] Test in several VST3 hosts on Windows, macOS and Linux

# Alive Synth

A VST3 synth in Rust (nih-plug) built to sound alive, with stereo designed in from the start.

## Status: phase 4

Picking this project up in a new chat? Start with `HANDOFF.md`.

- 8 built-in wavetable frames (sine, triangle, saw, square, pulse, organ, formant, hollow),
  morphed by **Position**, band-limited with mipmapped tables
- 16-voice polyphony with oldest-first voice stealing, ADSR amp envelope
- **Granular engine**: each voice can spawn a cloud of short grains (up to 32 at once). Every
  grain gets its own random pitch, position, pan, length and level. Controls: **Grain Mix**,
  **Source**, **Density**, **Size**, **Pitch Spread**, **Scatter**, **Pan Spread**, **Shape**.
- **Grain Source**: the wavetable, or four built-in textures generated in code at the host
  sample rate: **Choir** (two singers gliding through vowels), **Glass Bells** (shimmering
  inharmonic partials), **Breath** (sweeping filtered noise) and **Saw Swarm** (seven slowly
  breathing detuned saws). Position chooses where in the loop new grains start.
- **Stereo filter**: low, band or high-pass with Cutoff, Resonance, Key Track and
  **Filter Width**, which pushes the left and right cutoffs apart
- **Filter envelope**: its own ADSR and an amount in octaves
- **Drift**: five slow random-walk streams per voice (pitch, wavetable position, level, cutoff and
  a left/right cutoff spread). **Drift** sets the amount, **Drift Rate** the speed.
- **Variation**: per-note random fine detune, start phase and stereo position
- Smoothed parameters, sample-accurate note timing, no allocation on the audio thread

Not yet built: unison, effects, tabbed GUI, modulation matrix, presets, sample loading.
All parameters are listed in `docs/PARAMETERS.md`.

## CPU and memory

Measured in a release build with 16 voices all sounding (one core, on the build machine):

| Setting | Load |
| --- | --- |
| Grain Mix 0% | about 3% |
| Defaults (35% grains, 30/s, 80 ms) | about 7% |
| Worst case (100%, 100/s, 500 ms, pool full) | about 36 to 45% |

The textures take about 0.5 s to build when the plugin loads (1 s at 96 kHz) and use about
13 MB (26 MB at 96 kHz). Re-run the measurements with `docs/ENVIRONMENT.md`. If the worst case is
too heavy, lower `MAX_GRAINS` in `src/granular.rs`.

## Build

You need a recent stable Rust (install with rustup). Distro packages such as Ubuntu's
Rust 1.75 are too old for the current dependencies.

```
cargo test
cargo xtask bundle alive_synth --release
```

The plugin appears in `target/bundled/Alive Synth.vst3`. Copy it to your VST3 folder:

- Windows: `C:\Program Files\Common Files\VST3`
- macOS: `~/Library/Audio/Plug-Ins/VST3`
- Linux: `~/.vst3`

While developing, `cargo xtask bundle alive_synth` (debug build) also turns on the
allocation checker, which aborts if anything allocates on the audio thread.

## Before you release

- Change `VENDOR`, `URL` and `EMAIL` in `src/lib.rs`.
- Pick your own `VST3_CLASS_ID` (16 bytes) and never change it afterwards.
- nih-plug is ISC-licensed, but its VST3 bindings are GPLv3. A VST3 plugin built this way
  must be released under GPLv3 or later, unless you hold a Steinberg VST3 license
  that allows otherwise. Check this before distributing.

## Layout

| File | Purpose |
| --- | --- |
| `src/lib.rs` | Plugin, parameters, MIDI events, voice allocation, audio loop |
| `src/voice.rs` | One voice: oscillator, grain cloud, stereo filter, envelopes, drift, stereo position |
| `src/granular.rs` | Grain cloud: spawning, windows, per-grain pitch, position and pan |
| `src/sources.rs` | Built-in grain textures, generated in code |
| `HANDOFF.md`, `docs/` | Project context for resuming in another chat |
| `src/filter.rs` | State variable filter and soft saturator |
| `src/drift.rs` | Mean-reverting random-walk drift streams |
| `src/wavetable.rs` | Mipmapped wavetable bank |
| `src/env.rs` | ADSR envelope |
| `src/rng.rs` | xorshift random generator |

## Next (phase 5)

Unison with separate detune, spread and phase controls, then the effects (ensemble, delay,
reverb, master width). See `docs/ROADMAP.md`.

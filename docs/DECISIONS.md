# Decisions

**Owner** means the owner stated it. **Claude** means it was proposed during the work and the owner
approved the direction by continuing. Change the Claude ones freely if listening says otherwise.

## Product

| Decision | By | Reason |
| --- | --- | --- |
| Wavetable plus granular engine | Owner | Loves granular, and wavetables are needed too |
| "Alive" = analog-style drift, per-voice randomness, unison movement | Owner | Core identity |
| Stereo is the top priority | Owner | Core identity |
| Tabbed interface, easy, not minimal, basic colors | Owner | Usability, no distraction |
| Rust and nih-plug, VST3 | Owner | Their stack |
| Grain material mainly built-in, with optional sample loading | Owner | Keeps v1 focused |
| Five tabs: Sound, Filter and Amp, Life, Mod, FX, with a fixed header | Claude | Life tab gives the plugin an identity |
| Sample loading comes last | Claude | It touches threads and saved state |

## Sound design

| Decision | By | Reason |
| --- | --- | --- |
| Drift is a mean-reverting random walk, not an LFO | Claude | Never repeats, bounded, reads as analog |
| One drift amount knob and one rate knob | Claude | Simple, scales everything |
| Variation (per note) and Drift (over time) are separate | Claude | Different jobs: note-to-note versus within a note |
| Filter is applied per channel with different cutoffs | Claude | Cheap width without delays |
| Soft saturator after the filter | Claude | Keeps resonance from running away |
| Unison split into detune, spread and phase controls (phase 5) | Claude | Avoid one "unison blob" |

## Engineering

| Decision | By | Reason |
| --- | --- | --- |
| 16 voices, 32 grains per voice | Claude | Balance of richness and CPU (see HANDOFF) |
| Grains read the wavetable bank directly (phase 3), not a recorded copy of the oscillator | Claude | No latency, no per-voice buffers, and Scatter can pick different wavetable positions. This changed the earlier plan |
| Textures are generated in code at the host sample rate | Claude | No licensing, no files, always in tune |
| Textures are band-limited in 6 levels | Claude | Playing a loop fast would alias otherwise |
| Texture sources are mono | Claude | Memory and build time; width comes from grain pan |
| Grain Mix is a linear crossfade | Claude | Predictable; correlated signals do not add loudness |
| Texture grains start where Position points, wavetable grains follow it live | Claude | Natural for each kind of source |
| Parameter IDs are short and fixed | Claude | Host sessions depend on them |
| `f64` for texture read positions | Claude | `f32` positions in a 144000-sample loop are too coarse and detune grains |
| Test with offline renders and numbers | Claude | No DAW or ears available during development |

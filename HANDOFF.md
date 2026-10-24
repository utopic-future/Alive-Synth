# Alive Synth: handoff

This file is the starting point for a new chat or a new developer. It holds everything the
original conversation knew that is not obvious from the code.

Last updated at the end of **phase 4**. 48 tests pass, 3 more (benchmarks and a docs dump) are
marked `ignored` and run on demand.

## Resume in a new chat

1. Upload `alive-synth.zip` (or the unpacked folder) to the new chat.
2. Paste this prompt:

> I am continuing the Alive Synth VST3 project (Rust, nih-plug). Read `HANDOFF.md`, then
> `docs/ROADMAP.md` and `docs/ARCHITECTURE.md` from the project. Set up the Rust toolchain as
> described in `docs/ENVIRONMENT.md` and run `cargo test`. Then summarize what you understood in
> five lines, and continue with the first open item in the roadmap. Follow the rules in
> `HANDOFF.md`, and update the docs when you change behavior.

If the new chat is in the same claude.ai Project ("Alive Synth"), project memory also holds the
owner's decisions, but treat this folder as the source of truth.

## What this is

A polyphonic VST3 synth in Rust (nih-plug) that should feel **alive**, with **stereo** as the top
priority. Two sound engines, a wavetable oscillator and a granular engine, blend into one stereo
filter, with slow analog-style drift and per-note randomness everywhere.

## What the owner asked for (their words, summarized)

- More than a synth: it has to *feel alive*.
- Stereo is very important.
- Interface: easy and intuitive, **not minimal**, divided in **tabs**, basic colors, not distracting.
- Sound: loves **granular**, and it also needs **wavetables**.
- "Alive" means analog-style drift and imperfection, and per-voice randomness / unison movement.
- Built with **Rust and nih-plug**.
- Granular material: mainly **built-in**, with an **option to load samples**.
- Wants best effort on the whole project, and works phase by phase ("go on").

## Status

| Phase | Content | State |
| --- | --- | --- |
| 1 | Wavetable synth, polyphony, ADSR, stereo output | done |
| 2 | Stereo filter, drift system | done |
| 3 | Granular engine, grain mix, filter envelope | done |
| 4 | Built-in grain textures and a source selector | done |
| 5 | Unison, effects (ensemble, delay, reverb), master width | next |
| 6 | Tabbed GUI | open |
| 7 | Modulation matrix, LFOs, presets | open |
| 8 | Optional sample loading | open |

Details and the checklist are in `docs/ROADMAP.md`.

## Not verified yet

The plugin has **never been loaded in a DAW or heard**. Everything was checked with unit tests,
offline rendering inside tests, a CPU benchmark and the VST3 bundle export. The sound design
numbers (drift amounts, grain defaults, filter ranges) are first guesses that need listening.
The first job on a machine with a DAW is to load it, play it, and note what sounds wrong.

## File map

| Path | Purpose |
| --- | --- |
| `src/lib.rs` | Plugin, parameters, MIDI events, voice allocation, audio loop |
| `src/voice.rs` | One voice: oscillator, grain cloud, stereo filter, envelopes, drift, pan |
| `src/granular.rs` | Grain cloud: spawning, windows, per-grain pitch, position, pan |
| `src/sources.rs` | Built-in grain textures, generated in code, in band-limited levels |
| `src/wavetable.rs` | Mipmapped wavetable bank (8 frames) |
| `src/filter.rs` | State variable filter and soft saturator |
| `src/drift.rs` | Mean-reverting random-walk drift streams |
| `src/env.rs` | ADSR envelope |
| `src/rng.rs` | xorshift32 random generator |
| `docs/` | Architecture, parameters, decisions, roadmap, environment notes |

## Rules for every change

1. **Real-time safety.** No allocation, locks, file access or logging on the audio thread.
   Build big things in `initialize()`. Debug builds abort on allocation in `process()`.
2. **Never rename or reuse a parameter ID** (`#[id = "..."]`). Hosts store them in projects.
3. **Every behavior gets a test**, preferably an offline render that checks numbers
   (levels, stereo difference, finiteness, repeatability), because nobody can listen in CI.
4. **Keep it deterministic.** Randomness comes from the per-voice `Rng`, never from the system.
5. **Stereo first.** Width should come from voices, grains, filters and drift, not only from a
   master effect, and the sound must stay mono-compatible (a mono-sum button is planned).
6. **Run before you hand over:** `cargo test`, then `cargo xtask bundle alive_synth --release`.
   After changing parameters, regenerate `docs/PARAMETERS.md` (command inside that file).
7. **Update the docs** (`HANDOFF.md` status, `docs/ROADMAP.md`, `docs/DECISIONS.md`) in the same change.
8. Tell the owner plainly what is guessed and what is measured.

## Known limitations

- A stolen or choked voice stops instantly and can click. A short fade is planned.
- Wavetable and grain reads use linear interpolation. Cubic would be cleaner for bass.
- Lowest notes lose harmonics above about 14 kHz (mip levels step by octaves). Half-octave
  steps would fix it.
- Texture sources are mono; width comes from grain pan, filters and drift.
- Building the textures takes about 0.5 s at 48 kHz and 1 s at 96 kHz, and uses about 13 MB at
  48 kHz (26 MB at 96 kHz). It happens in `initialize()`.
- Worst case CPU (16 voices, 100 grains/s, 500 ms, pool full) is about 40% of one core on the
  build machine. Defaults are around 8%.
- No GUI yet: hosts show their generic parameter list.
- VST3 licensing: nih-plug's VST3 bindings are GPLv3, so a release is GPLv3 unless the owner holds a
  Steinberg license. `Cargo.toml` says GPL-3.0-or-later. Vendor, URL, email and the
  VST3 class ID in `src/lib.rs` are placeholders.

## Links

- Project plan document (a Claude Doc in the owner's claude.ai account):
  https://claude.ai/artifact/NkgSiv1WayYkk91x3DhZht

# Architecture

## Signal flow

```
                      Life layer: drift streams + Variation (per voice)
                                        |
   +------------------------------- one voice ---------------------------------+
   |                                                                            |
   |  wavetable oscillator ---------------------+                               |
   |   (8 frames, Position morph)               |   crossfade (Grain Mix)       |
   |                                            +--> mix L ---> filter L --+    |
   |  grain cloud (up to 32 grains) -----------+--> mix R ---> filter R --+--> amp x velocity x level
   |   reads wavetable or a texture                (cutoffs differ)           |   -> pan -> voice sum
   +----------------------------------------------------------------------------+
                                        |
                              16 voices summed -> output (stereo)
```

Planned after the voice sum: ensemble, delay, reverb, master width, meters.

## Per-sample, per-block, per-note

| Rate | Work |
| --- | --- |
| Per note | Random detune, pan, start phase, drift start values, key tracking octave, mip level |
| Per block (at most 64 samples, shorter at MIDI events) | Drift streams step, filter coefficients, envelope coefficients, grain settings, smoothed parameter steps |
| Per 16 samples | Filter coefficients, only while the filter envelope amount is not zero |
| Per sample | Oscillator, grains, filters, envelopes, pan |

Blocks are split at every MIDI event, so note timing is sample accurate.

## Modules

- **lib.rs**: parameters (`AliveParams`), voice allocation (free voice, else oldest releasing,
  else oldest), the block loop, and building `BlockCtx` once per block.
- **voice.rs**: `Voice` and `BlockCtx`. The voice renders one block into the output buffers.
  Five drift streams: pitch, position, level, cutoff, cutoff spread (left versus right).
- **granular.rs**: `GrainSettings` (per block), `GrainCloud` (per voice), `GrainEnv` (what grains
  read). A grain remembers its source at spawn, so switching source never breaks running grains.
  Grain level compensation divides by the overlap, by its square root for detuned or texture
  grains.
- **sources.rs**: four loops of 3 seconds made at the host sample rate: Choir (two singers through
  five vowels, with vibrato), Glass Bells (inharmonic partials, doubled with a detune), Breath (noise
  through a sweeping band-pass), Saw Swarm (seven detuned saws). Everything uses whole cycles per
  loop, so loops are seamless. Six band-limited levels per source cover playback speeds up to 32x.
  Root pitch is 128 Hz; a grain plays at speed = grain frequency / 128 Hz.
- **wavetable.rs**: 8 frames x 10 mip levels x 2048 samples, built by additive synthesis.
  Level `l` keeps `512 >> l` harmonics. One gain per frame keeps loudness equal across levels.
- **filter.rs**: Cytomic/TPT state variable filter, Q from 0.707 to about 15, plus `soft_clip`
  (saturates at about plus or minus 2) after each filter.
- **drift.rs**: `x = x * (1 - a) + sigma * noise`, clamped to -1..1, with `a` and `sigma` derived from the
  block length, so statistics do not depend on the host buffer size. Long-run spread is about 0.5.
- **env.rs**: ADSR. Attack is linear, decay and release are exponential and reach 0.1% of their
  distance (-60 dB) in the set time.
- **rng.rs**: xorshift32, one per voice, with a fixed seed per voice index.

## Stereo design

Width is built at every level:

1. Each grain has its own pan.
2. Each voice has a random pan from Variation, and its own drift streams.
3. The left and right filters have different cutoffs (Filter Width, plus a drift stream that moves
   them in opposite directions over time).
4. Planned: ensemble, delay, reverb with width controls, a master width, a correlation meter and a
   mono-sum button.

## Constants worth knowing

| Constant | Where | Value |
| --- | --- | --- |
| Voices | `lib.rs` `NUM_VOICES` | 16 |
| Max block | `lib.rs` `MAX_BLOCK_SIZE` | 64 samples |
| Grains per voice | `granular.rs` `MAX_GRAINS` | 32 |
| Texture loop | `sources.rs` | 3 s, root 128 Hz, 6 levels |
| Drift scales at 100% | `voice.rs` | pitch 15 cents, position 0.10, level 1.5 dB, cutoff 0.75 oct |
| Filter Width at 100% | `voice.rs` `WIDTH_OCT` | 0.33 octave each side |
| Variation at 100% | `voice.rs` | detune up to 10 cents, random pan, random start phase |

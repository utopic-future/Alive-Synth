# Parameters

Generated from the code with `cargo test dump_parameters -- --ignored --nocapture`, then the last column was added by hand.
Parameter IDs are what a DAW stores in projects and presets. **Never rename or reuse an ID** after release.

| ID | Name | Min | Max | Default | What it does |
| --- | --- | --- | --- | --- | --- |
| `gain` | Gain | -60.0 dB | 0.0 dB | -12.0 dB | Master level of the synth (before the host fader). |
| `pos` | Position | 0 % | 100 % | 0 % | Morphs through the 8 wavetable frames. Also where texture grains start in their loop. |
| `atk` | Attack | 0.001 s | 8.000 s | 0.005 s | Amp envelope attack. |
| `dec` | Decay | 0.001 s | 8.000 s | 0.300 s | Amp envelope decay. |
| `sus` | Sustain | 0 % | 100 % | 70 % | Amp envelope sustain level. |
| `rel` | Release | 0.001 s | 8.000 s | 0.400 s | Amp envelope release. |
| `var` | Variation | 0 % | 100 % | 30 % | Per-note random fine detune, start phase and stereo position. |
| `ftype` | Filter Type | Low-pass | High-pass | Low-pass | Low-pass, band-pass or high-pass state variable filter. |
| `cut` | Cutoff | 20.0 Hz | 20.0 kHz | 12.0 kHz | Filter cutoff before key tracking, envelope and drift. |
| `res` | Resonance | 0 % | 100 % | 10 % | Filter resonance (Q about 0.7 up to about 15). |
| `ktrk` | Key Track | 0 % | 100 % | 50 % | How much the cutoff follows the note pitch (100% = one octave per octave). |
| `fwid` | Filter Width | 0 % | 100 % | 25 % | Pushes left and right cutoffs apart (up to about 0.33 octave each way). |
| `fatk` | Filter Attack | 0.001 s | 8.000 s | 0.005 s | Filter envelope attack. |
| `fdec` | Filter Decay | 0.001 s | 8.000 s | 0.300 s | Filter envelope decay. |
| `fsus` | Filter Sustain | 0 % | 100 % | 30 % | Filter envelope sustain. |
| `frel` | Filter Release | 0.001 s | 8.000 s | 0.400 s | Filter envelope release. |
| `famt` | Filter Env Amount | -4.00 oct | 4.00 oct | 0.00 oct | Octaves the filter envelope moves the cutoff. Negative closes the filter. |
| `gmix` | Grain Mix | 0 % | 100 % | 35 % | Linear crossfade from the wavetable oscillator (0%) to the grain cloud (100%). |
| `gsrc` | Grain Source | Wavetable | Saw Swarm | Wavetable | What grains read: Wavetable, or the built-in textures Choir, Glass Bells, Breath, Saw Swarm. |
| `gdens` | Grain Density | 1 /s | 100 /s | 30 /s | New grains per second, per voice. The pool holds 32 grains per voice. |
| `gsize` | Grain Size | 5 ms | 500 ms | 80 ms | Grain length (each grain varies by up to 20%). |
| `gpitch` | Grain Pitch Spread | 0 ct | 1200 ct | 12 ct | Random pitch offset per grain, in cents either way. At 0 a wavetable cloud stays in tune and in phase. |
| `gscat` | Grain Scatter | 0 % | 100 % | 30 % | Random offset per grain: wavetable position (up to 0.4) or start point in the texture loop (up to half a loop). |
| `gpan` | Grain Pan Spread | 0 % | 100 % | 70 % | Random pan per grain. |
| `gshape` | Grain Shape | 0 % | 100 % | 0 % | 0 = soft Hann-like grains, 100 = nearly flat grains with short fades. |
| `drift` | Drift | 0 % | 100 % | 35 % | Amount of the slow random walks on pitch, position, level and cutoff (per voice). |
| `drate` | Drift Rate | 0.05 x | 4.00 x | 1.00 x | Speed of the drift. 1.0 means a time constant of about 2 seconds. |

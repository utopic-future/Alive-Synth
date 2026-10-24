//! Built-in band-limited wavetable bank.
//!
//! The bank holds `NUM_FRAMES` waveforms. Each frame is stored as `NUM_LEVELS`
//! mipmapped tables, built by additive synthesis with fewer harmonics at every
//! level, so high notes read a table that cannot alias. The Position control
//! morphs between neighbouring frames.
//!
//! Building the bank allocates and takes a few tens of milliseconds, so it
//! happens in `initialize()`, never on the audio thread.

use std::f32::consts::{PI, TAU};

pub const TABLE_SIZE: usize = 2048;
const TABLE_MASK: usize = TABLE_SIZE - 1;
pub const NUM_FRAMES: usize = 8;
pub const NUM_LEVELS: usize = 10;
/// Harmonics in the most detailed table. Level `l` keeps `MAX_HARMONICS >> l`.
const MAX_HARMONICS: usize = 512;

pub struct WavetableBank {
    /// Flattened `[frame][level][sample]`.
    tables: Vec<f32>,
}

impl WavetableBank {
    /// An empty bank, only valid until `new()` replaces it in `initialize()`.
    pub fn empty() -> Self {
        Self { tables: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    pub fn new() -> Self {
        let sine: Vec<f32> = (0..TABLE_SIZE)
            .map(|i| (TAU * i as f32 / TABLE_SIZE as f32).sin())
            .collect();

        let mut tables = vec![0.0f32; NUM_FRAMES * NUM_LEVELS * TABLE_SIZE];

        for frame in 0..NUM_FRAMES {
            let amps: Vec<f32> = (1..=MAX_HARMONICS).map(|n| harmonic_amp(frame, n)).collect();
            let mut norm = 1.0f32;

            for level in 0..NUM_LEVELS {
                let harmonics = (MAX_HARMONICS >> level).max(1);
                let start = (frame * NUM_LEVELS + level) * TABLE_SIZE;
                let table = &mut tables[start..start + TABLE_SIZE];

                for (n, &amp) in amps.iter().enumerate().take(harmonics) {
                    if amp == 0.0 {
                        continue;
                    }
                    let h = n + 1;
                    for (i, sample) in table.iter_mut().enumerate() {
                        *sample += amp * sine[(h * i) & TABLE_MASK];
                    }
                }

                if level == 0 {
                    // One gain per frame, taken from the most detailed table, so
                    // loudness stays consistent as the mip level changes.
                    let peak = table.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                    norm = if peak > 0.0 { 1.0 / peak } else { 1.0 };
                }
                for sample in table.iter_mut() {
                    *sample *= norm;
                }
            }
        }

        Self { tables }
    }

    /// Mip level that keeps every harmonic below Nyquist for this frequency.
    pub fn level_for_frequency(freq: f32, sample_rate: f32) -> usize {
        let allowed = (sample_rate * 0.5 / freq.max(1.0)).floor().max(1.0);
        let ratio = MAX_HARMONICS as f32 / allowed;
        let level = if ratio <= 1.0 {
            0
        } else {
            ratio.log2().ceil() as usize
        };
        level.min(NUM_LEVELS - 1)
    }

    /// Read the morphed waveform at `phase` (0..1) and `position` (0..1).
    #[inline]
    pub fn sample(&self, phase: f32, position: f32, level: usize) -> f32 {
        let scaled = position.clamp(0.0, 1.0) * (NUM_FRAMES - 1) as f32;
        let a = (scaled as usize).min(NUM_FRAMES - 2);
        let t = scaled - a as f32;
        let x = self.read(a, level, phase);
        let y = self.read(a + 1, level, phase);
        x + (y - x) * t
    }

    #[inline]
    fn read(&self, frame: usize, level: usize, phase: f32) -> f32 {
        let pos = phase * TABLE_SIZE as f32;
        let index = pos as usize;
        let frac = pos - index as f32;
        let start = (frame * NUM_LEVELS + level) * TABLE_SIZE;
        let table = &self.tables[start..start + TABLE_SIZE];
        let a = table[index & TABLE_MASK];
        let b = table[(index + 1) & TABLE_MASK];
        a + (b - a) * frac
    }
}

/// Amplitude of harmonic `n` (1 = fundamental) for each built-in frame.
fn harmonic_amp(frame: usize, n: usize) -> f32 {
    let nf = n as f32;
    let odd = n % 2 == 1;
    match frame {
        // sine
        0 => {
            if n == 1 {
                1.0
            } else {
                0.0
            }
        }
        // triangle
        1 => {
            if odd {
                let sign = if (n / 2) % 2 == 0 { 1.0 } else { -1.0 };
                sign / (nf * nf)
            } else {
                0.0
            }
        }
        // saw
        2 => 1.0 / nf,
        // square
        3 => {
            if odd {
                1.0 / nf
            } else {
                0.0
            }
        }
        // 25% pulse
        4 => (PI * nf * 0.25).sin() / nf,
        // organ: a handful of drawbar-like harmonics
        5 => match n {
            1 => 1.0,
            2 => 0.7,
            3 => 0.5,
            4 => 0.35,
            6 => 0.25,
            8 => 0.2,
            _ => 0.0,
        },
        // formant: saw with a bump around the 8th harmonic
        6 => (1.0 / nf) * (1.0 + 3.0 * (-((nf - 8.0) / 2.5).powi(2)).exp()),
        // hollow and bright: odd harmonics falling slowly
        _ => {
            if odd {
                1.0 / nf.sqrt()
            } else {
                0.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_is_normalised_and_finite() {
        let bank = WavetableBank::new();
        for frame in 0..NUM_FRAMES {
            let start = frame * NUM_LEVELS * TABLE_SIZE;
            let table = &bank.tables[start..start + TABLE_SIZE];
            let peak = table.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(table.iter().all(|s| s.is_finite()));
            assert!((peak - 1.0).abs() < 1e-4, "frame {frame} peak {peak}");
        }
    }

    #[test]
    fn level_grows_with_pitch() {
        let sr = 48_000.0;
        let low = WavetableBank::level_for_frequency(40.0, sr);
        let mid = WavetableBank::level_for_frequency(440.0, sr);
        let high = WavetableBank::level_for_frequency(8_000.0, sr);
        assert!(low < mid && mid < high);
        assert_eq!(low, 0);
        assert!(high < NUM_LEVELS);
    }

    #[test]
    fn harmonics_never_pass_nyquist() {
        let sr = 48_000.0;
        for midi in 0..128u32 {
            let freq = 440.0 * 2f32.powf((midi as f32 - 69.0) / 12.0);
            let level = WavetableBank::level_for_frequency(freq, sr);
            let harmonics = (MAX_HARMONICS >> level).max(1) as f32;
            // the table is only used with at most Nyquist worth of harmonics,
            // except the single sine of the last level
            assert!(harmonics * freq <= sr * 0.5 || harmonics == 1.0, "note {midi}");
        }
    }

    #[test]
    fn position_morphs_between_frames() {
        let bank = WavetableBank::new();
        let sine = bank.sample(0.25, 0.0, 0);
        let saw = bank.sample(0.25, 2.0 / 7.0, 0);
        let mixed = bank.sample(0.25, 1.0 / 7.0, 0);
        assert!((sine - 1.0).abs() < 1e-3, "sine peak at quarter cycle");
        assert!(mixed.is_finite() && saw.is_finite());
    }
}

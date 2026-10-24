//! Amplitude envelope (ADSR) with exponential decay and release.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Per-block envelope settings, computed once and shared by every voice.
#[derive(Clone, Copy)]
pub struct EnvCoefs {
    attack_inc: f32,
    decay_coef: f32,
    sustain: f32,
    release_coef: f32,
}

impl EnvCoefs {
    pub fn new(attack: f32, decay: f32, sustain: f32, release: f32, sample_rate: f32) -> Self {
        // Exponential stages cover 99.9% of their distance (-60 dB) in the given time.
        let coef = |time: f32| 0.001f32.powf(1.0 / (time.max(0.001) * sample_rate));
        Self {
            attack_inc: 1.0 / (attack.max(0.001) * sample_rate),
            decay_coef: coef(decay),
            sustain: sustain.clamp(0.0, 1.0),
            release_coef: coef(release),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Adsr {
    stage: Stage,
    level: f32,
}

impl Adsr {
    pub const fn new() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
        }
    }

    /// Start a fresh note from silence.
    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
        self.level = 0.0;
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// Stop immediately (used when a voice is choked or stolen).
    pub fn kill(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn is_releasing(&self) -> bool {
        self.stage == Stage::Release
    }

    #[inline]
    pub fn next(&mut self, c: &EnvCoefs) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += c.attack_inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = c.sustain + (self.level - c.sustain) * c.decay_coef;
                if (self.level - c.sustain).abs() < 1e-4 {
                    self.level = c.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = c.sustain,
            Stage::Release => {
                self.level *= c.release_coef;
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn reaches_sustain_then_releases_to_idle() {
        let c = EnvCoefs::new(0.01, 0.05, 0.6, 0.1, SR);
        let mut env = Adsr::new();
        env.trigger();
        let mut peak = 0.0f32;
        for _ in 0..(SR as usize) {
            peak = peak.max(env.next(&c));
        }
        assert!((peak - 1.0).abs() < 1e-6, "attack should reach 1.0");
        assert!((env.next(&c) - 0.6).abs() < 1e-3, "should rest at sustain");

        env.release();
        assert!(env.is_releasing());
        for _ in 0..(SR as usize) {
            env.next(&c);
        }
        assert!(env.is_idle(), "release should end in idle");
    }

    #[test]
    fn kill_silences_at_once() {
        let c = EnvCoefs::new(0.001, 0.1, 1.0, 0.1, SR);
        let mut env = Adsr::new();
        env.trigger();
        for _ in 0..1000 {
            env.next(&c);
        }
        env.kill();
        assert_eq!(env.next(&c), 0.0);
        assert!(env.is_idle());
    }
}

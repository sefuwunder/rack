// Vendored from grandmother (see README: vendored engines).
//! ADSR envelope generator, linear segments, sample-accurate.
//! `gate_on` restarts the attack from the current level (smooth retrigger
//! for the arpeggiator); `gate_off` enters a linear release from the
//! level where release began.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Debug)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    sr: f32,
    stage: Stage,
    level: f32,
    release_step: f32,
}

impl Adsr {
    pub fn new(sr: f32) -> Self {
        Adsr {
            attack: 0.01,
            decay: 0.2,
            sustain: 0.7,
            release: 0.2,
            sr,
            stage: Stage::Idle,
            level: 0.0,
            release_step: 0.0,
        }
    }

    pub fn set(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.attack = attack.max(0.0);
        self.decay = decay.max(0.0);
        self.sustain = sustain.clamp(0.0, 1.0);
        self.release = release.max(0.0);
    }

    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
            self.release_step = if self.release > 0.0 {
                self.level / (self.release * self.sr)
            } else {
                self.level
            };
        }
    }

    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {
                self.level = 0.0;
            }
            Stage::Attack => {
                if self.attack <= 0.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                } else {
                    self.level += 1.0 / (self.attack * self.sr);
                    if self.level >= 1.0 {
                        self.level = 1.0;
                        self.stage = Stage::Decay;
                    }
                }
            }
            Stage::Decay => {
                if self.decay <= 0.0 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                } else {
                    let step = (1.0 - self.sustain) / (self.decay * self.sr);
                    self.level -= step;
                    if self.level <= self.sustain {
                        self.level = self.sustain;
                        self.stage = Stage::Sustain;
                    }
                }
            }
            Stage::Sustain => {}
            Stage::Release => {
                self.level -= self.release_step;
                if self.level <= 0.0 {
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

    #[test]
    fn attack_decay_sustain_release() {
        let mut e = Adsr::new(1000.0);
        e.set(0.1, 0.1, 0.5, 0.1);
        e.gate_on();
        let mut peak = 0.0f32;
        for _ in 0..100 {
            peak = peak.max(e.next());
        }
        assert!((peak - 1.0).abs() < 1e-6, "attack reaches 1: {peak}");
        for _ in 0..100 {
            e.next();
        }
        assert!((e.level - 0.5).abs() < 0.01, "decay lands on sustain: {}", e.level);
        e.gate_off();
        for _ in 0..200 {
            e.next();
        }
        assert!(e.level == 0.0, "release ends at zero");
    }

    #[test]
    fn instant_attack_and_retrigger() {
        let mut e = Adsr::new(1000.0);
        e.set(0.0, 0.05, 0.8, 0.05);
        e.gate_on();
        assert_eq!(e.next(), 1.0);
        e.gate_off();
        e.gate_on();
        assert_eq!(e.next(), 1.0, "retrigger restarts attack");
    }
}

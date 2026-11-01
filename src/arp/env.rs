// Vendored from arp2600 (see README: vendored engines).
//! ADSR and AR transient generators. Linear segments, sample-counted;
//! times are seconds (log-scaled by the UI, linear here).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32, // 0..1
    pub release: f32,
    stage: EnvStage,
    value: f32,
    t: f32, // seconds spent in current stage
}

impl Adsr {
    pub fn new() -> Self {
        Self {
            attack: 0.01,
            decay: 0.1,
            sustain: 0.7,
            release: 0.2,
            stage: EnvStage::Idle,
            value: 0.0,
            t: 0.0,
        }
    }

    pub fn gate_on(&mut self) {
        self.stage = EnvStage::Attack;
        self.t = 0.0;
    }

    pub fn gate_off(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
            self.t = 0.0;
        }
    }

    pub fn stage(&self) -> EnvStage {
        self.stage
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn next(&mut self, dt: f32) -> f32 {
        self.t += dt;
        match self.stage {
            EnvStage::Idle => self.value = 0.0,
            EnvStage::Attack => {
                let a = self.attack.max(0.001);
                self.value = (self.t / a).min(1.0);
                if self.t >= a {
                    self.stage = EnvStage::Decay;
                    self.t = 0.0;
                }
            }
            EnvStage::Decay => {
                let d = self.decay.max(0.001);
                let k = (self.t / d).min(1.0);
                self.value = 1.0 + (self.sustain - 1.0) * k;
                if self.t >= d {
                    self.stage = EnvStage::Sustain;
                }
            }
            EnvStage::Sustain => self.value = self.sustain,
            EnvStage::Release => {
                let r = self.release.max(0.001);
                let k = (self.t / r).min(1.0);
                // Release from the value held at gate-off: approximate by
                // scaling the sustain level; retrigger resets cleanly.
                let start = self.value.max(self.sustain);
                self.value = start * (1.0 - k);
                if self.t >= r {
                    self.stage = EnvStage::Idle;
                    self.value = 0.0;
                }
            }
        }
        self.value
    }
}

/// Attack/Release envelope (the 2600's second transient generator).
#[derive(Clone, Copy)]
pub struct Ar {
    pub attack: f32,
    pub release: f32,
    stage: EnvStage,
    value: f32,
    t: f32,
}

impl Ar {
    pub fn new() -> Self {
        Self {
            attack: 0.01,
            release: 0.2,
            stage: EnvStage::Idle,
            value: 0.0,
            t: 0.0,
        }
    }

    pub fn gate_on(&mut self) {
        self.stage = EnvStage::Attack;
        self.t = 0.0;
    }

    pub fn gate_off(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
            self.t = 0.0;
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn next(&mut self, dt: f32) -> f32 {
        self.t += dt;
        match self.stage {
            EnvStage::Idle => self.value = 0.0,
            EnvStage::Attack => {
                let a = self.attack.max(0.001);
                self.value = (self.t / a).min(1.0);
                if self.t >= a {
                    self.stage = EnvStage::Sustain;
                }
            }
            EnvStage::Decay | EnvStage::Sustain => self.value = 1.0,
            EnvStage::Release => {
                let r = self.release.max(0.001);
                let k = (self.t / r).min(1.0);
                self.value = 1.0 * (1.0 - k);
                if self.t >= r {
                    self.stage = EnvStage::Idle;
                    self.value = 0.0;
                }
            }
        }
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adsr_reaches_sustain_and_releases() {
        let mut e = Adsr::new();
        e.attack = 0.01;
        e.decay = 0.01;
        e.sustain = 0.5;
        e.release = 0.01;
        e.gate_on();
        let dt = 1.0 / 44100.0;
        for _ in 0..4410 {
            e.next(dt);
        }
        assert!((e.value() - 0.5).abs() < 0.01);
        e.gate_off();
        for _ in 0..4410 {
            e.next(dt);
        }
        assert_eq!(e.value(), 0.0);
        assert_eq!(e.stage(), EnvStage::Idle);
    }

    #[test]
    fn ar_releases_to_zero() {
        let mut e = Ar::new();
        e.attack = 0.01;
        e.release = 0.01;
        e.gate_on();
        let dt = 1.0 / 44100.0;
        for _ in 0..882 {
            e.next(dt);
        }
        assert!(e.value() > 0.9);
        e.gate_off();
        for _ in 0..882 {
            e.next(dt);
        }
        assert_eq!(e.value(), 0.0);
    }
}

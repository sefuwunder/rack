// Vendored from arp2600 (see README: vendored engines).
//! Small DSP helpers: noise, sample & hold, lag (slew), ring modulator.

// xorshift32 white noise in -1..1.
#[derive(Clone, Copy)]
pub struct WhiteNoise {
    state: u32,
}

impl WhiteNoise {
    pub fn new(seed: u32) -> Self {
        Self {
            state: seed | 1,
        }
    }

    pub fn next(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Pink noise (Paul Kellet's filter), roughly -1..1.
#[derive(Clone, Copy)]
pub struct PinkNoise {
    b: [f32; 7],
    white: WhiteNoise,
}

impl PinkNoise {
    pub fn new(seed: u32) -> Self {
        Self {
            b: [0.0; 7],
            white: WhiteNoise::new(seed),
        }
    }

    pub fn next(&mut self) -> f32 {
        let w = self.white.next();
        let b = &mut self.b;
        b[0] = 0.99886 * b[0] + w * 0.0555179;
        b[1] = 0.99332 * b[1] + w * 0.0750759;
        b[2] = 0.96900 * b[2] + w * 0.1538520;
        b[3] = 0.86650 * b[3] + w * 0.3104856;
        b[4] = 0.55000 * b[4] + w * 0.5329522;
        b[5] = -0.7616 * b[5] - w * 0.0168980;
        let pink = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
        b[6] = w * 0.115926;
        (pink * 0.11).clamp(-1.0, 1.0)
    }
}

/// Sample & hold: on each clock tick, capture the input.
#[derive(Clone, Copy)]
pub struct SampleHold {
    pub held: f32,
    phase: f32,
}

impl SampleHold {
    pub fn new() -> Self {
        Self {
            held: 0.0,
            phase: 0.0,
        }
    }

    /// `rate_hz` is the internal clock; `ext_clock` high edge also ticks.
    /// Returns the held value after this sample.
    pub fn next(
        &mut self,
        input: f32,
        rate_hz: f32,
        ext_clock: bool,
        prev_ext_clock: bool,
        sr: f32,
    ) -> f32 {
        self.phase += rate_hz / sr;
        let mut tick = false;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            tick = true;
        }
        if ext_clock && !prev_ext_clock {
            tick = true;
        }
        if tick {
            self.held = input;
        }
        self.held
    }
}

/// One-pole lag (slew limiter). `time_s` is the 63% time.
#[derive(Clone, Copy)]
pub struct Lag {
    pub value: f32,
}

impl Lag {
    pub fn new() -> Self {
        Self { value: 0.0 }
    }

    pub fn next(&mut self, input: f32, time_s: f32, sr: f32) -> f32 {
        let t = time_s.max(0.0001);
        let g = 1.0 - (-1.0 / (t * sr)).exp();
        self.value += g * (input - self.value);
        self.value
    }
}

/// Envelope follower (the 2600 preamp's follower output): rectifies
/// the input and smooths it with a fast attack and a slower release,
/// so the output tracks amplitude without audio-rate ripple.
#[derive(Clone, Copy)]
pub struct EnvFollower {
    pub level: f32,
    attack: f32,
    release: f32,
}

impl EnvFollower {
    pub fn new(sr: f32) -> Self {
        let coeff = |t: f32| 1.0 - (-1.0 / (t * sr)).exp();
        Self {
            level: 0.0,
            attack: coeff(0.002),
            release: coeff(0.15),
        }
    }

    pub fn next(&mut self, input: f32) -> f32 {
        let v = input.abs();
        let g = if v > self.level {
            self.attack
        } else {
            self.release
        };
        self.level += g * (v - self.level);
        self.level
    }
}

/// Balanced (ring) modulator: product of two inputs.
pub fn ring(a: f32, b: f32) -> f32 {
    a * b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_bounded() {
        let mut w = WhiteNoise::new(12345);
        let mut p = PinkNoise::new(999);
        for _ in 0..44100 {
            assert!(w.next().abs() <= 1.0);
            assert!(p.next().abs() <= 1.0);
        }
    }

    #[test]
    fn sh_holds_between_ticks() {
        let mut sh = SampleHold::new();
        let v = sh.next(0.7, 1.0, false, false, 44100.0);
        // No tick yet at 1 Hz over one sample: held stays initial.
        assert_eq!(v, 0.0);
        // Force a tick with an external rising edge.
        let v = sh.next(0.7, 0.0, true, false, 44100.0);
        assert_eq!(v, 0.7);
        let v = sh.next(-0.3, 0.0, false, true, 44100.0);
        assert_eq!(v, 0.7);
    }

    #[test]
    fn follower_tracks_and_releases() {
        let mut f = EnvFollower::new(44100.0);
        for _ in 0..1000 {
            f.next(0.8);
        }
        assert!(f.level > 0.7, "level={}", f.level);
        // After the input stops, the slow release keeps it up briefly…
        for _ in 0..1000 {
            f.next(0.0);
        }
        assert!(f.level > 0.3, "level={}", f.level);
        // …then lets go.
        for _ in 0..44100 {
            f.next(0.0);
        }
        assert!(f.level < 0.01, "level={}", f.level);
    }
}

// Vendored from grandmother (see README: vendored engines).
//! Oscillators: phase-accumulator VCOs plus white noise.
//! All wave shapes are pure functions of phase so the patch bay can offer
//! SAW and PULSE outputs simultaneously, like the hardware, regardless of
//! which wave the mixer selector picks.

use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave {
    Saw,
    Square,
    Triangle,
}

pub fn saw(phase: f32) -> f32 {
    2.0 * phase - 1.0
}

pub fn pulse(phase: f32, pw: f32) -> f32 {
    if phase < pw {
        1.0
    } else {
        -1.0
    }
}

pub fn triangle(phase: f32) -> f32 {
    if phase < 0.25 {
        4.0 * phase
    } else if phase < 0.75 {
        2.0 - 4.0 * phase
    } else {
        -4.0 + 4.0 * phase
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Osc {
    pub phase: f32,
}

impl Osc {
    pub fn new() -> Self {
        Osc { phase: 0.0 }
    }

    /// Advance the phase by one sample at `freq`. Wraps to [0, 1).
    pub fn advance(&mut self, freq: f32, sr: f32) {
        self.phase += freq / sr;
        self.phase -= self.phase.floor();
    }

    /// The selected wave at the current phase.
    pub fn output(&self, wave: Wave, pw: f32) -> f32 {
        match wave {
            Wave::Saw => saw(self.phase),
            Wave::Square => pulse(self.phase, pw),
            Wave::Triangle => triangle(self.phase),
        }
    }
}

impl Default for Osc {
    fn default() -> Self {
        Self::new()
    }
}

/// White noise from a xorshift32 generator (deterministic seed).
#[derive(Clone, Copy, Debug)]
pub struct Noise {
    state: u32,
}

impl Noise {
    pub fn new() -> Self {
        Noise { state: 0x1234_5678 }
    }

    pub fn next(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        // Map to [-1, 1).
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

impl Default for Noise {
    fn default() -> Self {
        Self::new()
    }
}

/// Frequency of a 1V/oct CV in volts, with C2 (65.41 Hz) at 0 V.
pub fn cv_to_freq(cv_volts: f32) -> f32 {
    65.41 * 2.0f32.powf(cv_volts)
}

/// MIDI note number as a 1V/oct CV (C2 = 0 V).
pub fn midi_to_cv(midi: u8) -> f32 {
    (midi as f32 - 36.0) / 12.0
}

#[allow(dead_code)]
fn _tau_is_used() {
    let _ = TAU;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waves_stay_in_range() {
        let mut o = Osc::new();
        for i in 0..4410 {
            o.advance(440.0, 44100.0);
            for w in [Wave::Saw, Wave::Square, Wave::Triangle] {
                let v = o.output(w, 0.5);
                assert!(v >= -1.0 && v <= 1.0, "wave {w:?} out of range at {i}: {v}");
            }
        }
    }

    #[test]
    fn square_duty_follows_pw() {
        let mut o = Osc::new();
        let mut high = 0;
        let n = 4410;
        for _ in 0..n {
            o.advance(440.0, 44100.0);
            if o.output(Wave::Square, 0.25) > 0.0 {
                high += 1;
            }
        }
        let ratio = high as f32 / n as f32;
        assert!((ratio - 0.25).abs() < 0.03, "duty {ratio}");
    }

    #[test]
    fn noise_is_bounded_and_varying() {
        let mut nz = Noise::new();
        let mut prev = nz.next();
        let mut changed = false;
        for _ in 0..1000 {
            let v = nz.next();
            assert!(v >= -1.0 && v < 1.0);
            if v != prev {
                changed = true;
            }
            prev = v;
        }
        assert!(changed);
    }

    #[test]
    fn cv_freq_roundtrip() {
        // C2 = 0 V -> 65.41 Hz; +1 V -> one octave up.
        assert!((cv_to_freq(0.0) - 65.41).abs() < 0.01);
        assert!((cv_to_freq(1.0) - 130.82).abs() < 0.02);
        assert!((midi_to_cv(60) - 2.0).abs() < 1e-6);
    }
}

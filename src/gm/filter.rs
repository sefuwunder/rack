// Vendored from grandmother (see README: vendored engines).
//! Moog-style 4-pole ladder filter: four cascaded one-pole sections with
//! feedback and tanh saturation in the loop, the topology of the classic
//! ladder. Each pole is the exact one-pole discretization
//! `s += a * (x - s)` with `a = 1 - exp(-2*pi*fp/sr)`, so the filter is
//! stable for any cutoff below Nyquist and bounded for any input: the
//! tanh in the feedback path keeps self-oscillation at a musical level
//! instead of blowing up.
//!
//! The four poles are pre-warped so the cascade's -3 dB point tracks the
//! CUTOFF knob: four identical poles each at fp have their combined -3 dB
//! point at fp * sqrt(2^(1/4) - 1) ~= 0.435 * fp, so fp = cutoff / 0.435.

use std::f32::consts::PI;

#[derive(Clone, Debug)]
pub struct Ladder {
    pub cutoff: f32,
    pub reso: f32, // 0..1, 1 = self-oscillation territory
    sr: f32,
    poles: [f32; 4],
}

impl Ladder {
    pub fn new(sr: f32) -> Self {
        Ladder {
            cutoff: 1000.0,
            reso: 0.0,
            sr,
            poles: [0.0; 4],
        }
    }

    pub fn process(&mut self, x: f32) -> f32 {
        // Pre-warped pole frequency, clamped below Nyquist.
        let fp = (self.cutoff / 0.435).min(self.sr * 0.45);
        let a = 1.0 - (-2.0 * PI * fp / self.sr).exp();
        let k = self.reso * 4.0;
        let mut u = (x - k * self.poles[3]).tanh();
        for p in self.poles.iter_mut() {
            *p += a * (u - *p);
            u = *p;
        }
        self.poles[3]
    }
}

/// One-pole high-pass (the hardware's patchable utility filter, -6 dB/oct):
/// subtract a one-pole low-pass from the input.
#[derive(Clone, Debug)]
pub struct OnePoleHp {
    lp: f32,
}

impl OnePoleHp {
    pub fn new() -> Self {
        OnePoleHp { lp: 0.0 }
    }

    pub fn process(&mut self, x: f32, cutoff: f32, sr: f32) -> f32 {
        let a = 1.0 - (-2.0 * PI * cutoff.max(1.0).min(sr * 0.45) / sr).exp();
        self.lp += a * (x - self.lp);
        x - self.lp
    }
}

impl Default for OnePoleHp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_passes_and_output_bounded() {
        let mut f = Ladder::new(44100.0);
        f.cutoff = 1000.0;
        f.reso = 0.0;
        let mut y = 0.0;
        for _ in 0..44100 {
            y = f.process(0.5);
        }
        // Steady state is tanh(0.5): the tanh sits in the input path, so
        // DC passes at the saturated value, not at unity gain.
        assert!((y - 0.5f32.tanh()).abs() < 0.01, "dc steady state: {y}");
        // Max resonance + hot input must stay bounded (tanh loop).
        f.cutoff = 800.0;
        f.reso = 1.0;
        for _ in 0..44100 {
            y = f.process(2.0);
            assert!(y.is_finite() && y.abs() <= 1.5, "blowup: {y}");
        }
    }

    #[test]
    fn cutoff_sweeps_attenuate_highs() {
        // 5 kHz sine through a 200 Hz filter should be nearly gone;
        // through a 12 kHz filter it should mostly survive.
        let rms = |cutoff: f32| -> f32 {
            let mut f = Ladder::new(44100.0);
            f.cutoff = cutoff;
            f.reso = 0.0;
            let mut acc = 0.0f32;
            let n = 44100;
            for i in 0..n {
                let x = (2.0 * PI * 5000.0 * i as f32 / 44100.0).sin();
                let y = f.process(x);
                if i > 4410 {
                    acc += y * y;
                }
            }
            (acc / (n - 4410) as f32).sqrt()
        };
        let low = rms(200.0);
        let high = rms(12000.0);
        assert!(low < 0.05, "200 Hz filter should kill 5 kHz: {low}");
        assert!(high > 0.4, "12 kHz filter should pass 5 kHz: {high}");
    }

    #[test]
    fn high_pass_blocks_dc_and_passes_highs() {
        let mut hp = OnePoleHp::new();
        let mut y = 0.0;
        for _ in 0..44100 {
            y = hp.process(0.5, 1000.0, 44100.0);
        }
        assert!(y.abs() < 0.01, "DC should be blocked: {y}");
        let mut hp = OnePoleHp::new();
        let mut acc = 0.0f32;
        let n = 44100;
        for i in 0..n {
            let x = (2.0 * PI * 5000.0 * i as f32 / 44100.0).sin();
            let y = hp.process(x, 1000.0, 44100.0);
            if i > 4410 {
                acc += y * y;
            }
        }
        let rms = (acc / (n - 4410) as f32).sqrt();
        assert!(rms > 0.4, "5 kHz through a 1 kHz HP should mostly pass: {rms}");
    }
}

// Vendored from arp2600 (see README: vendored engines).
//! 4-pole ladder lowpass in the Moog style (four cascaded one-poles
//! with resonance feedback). Stable for resonance in 0..1: the feedback
//! gain is clamped so the filter rings but never blows up.

#[derive(Clone, Copy)]
pub struct Ladder {
    pub cutoff: f32,    // Hz, 20..20000
    pub resonance: f32, // 0..1
    pub cv_amount: f32, // octaves per unit CV, 0..4
    pub cv_input: f32,  // summed CV, roughly -1..1
    pub key_track: f32, // 0..1, octaves of cutoff shift per octave of note
    pub key_midi: f32,  // current note, for tracking
    s: [f32; 4],
}

impl Ladder {
    pub fn new() -> Self {
        Self {
            cutoff: 1000.0,
            resonance: 0.0,
            cv_amount: 0.0,
            cv_input: 0.0,
            key_track: 0.0,
            key_midi: 69.0,
            s: [0.0; 4],
        }
    }

    fn eff_cutoff(&self) -> f32 {
        let mut fc = self.cutoff;
        if self.cv_amount != 0.0 && self.cv_input != 0.0 {
            fc *= 2.0f32.powf(self.cv_input * self.cv_amount);
        }
        if self.key_track != 0.0 {
            fc *= 2.0f32.powf((self.key_midi - 69.0) / 12.0 * self.key_track);
        }
        fc.clamp(20.0, 20000.0)
    }

    pub fn next(&mut self, x: f32, sr: f32) -> f32 {
        let fc = self.eff_cutoff();
        // One-pole coefficient from the bilinear-ish approximation.
        let g = 1.0 - (-std::f32::consts::TAU * fc / sr).exp();
        let g = g.clamp(0.0, 1.0);
        // Resonance feedback, compensated so Q=1 self-oscillates gently
        // instead of diverging.
        let k = self.resonance.clamp(0.0, 1.0) * 3.6;
        let mut u = x - k * self.s[3];
        for stage in self.s.iter_mut() {
            *stage += g * (u - *stage);
            u = *stage;
        }
        // Mild output compensation so high resonance stays audible.
        self.s[3] * (1.0 + 0.5 * self.resonance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_passes_at_open_cutoff() {
        let mut f = Ladder::new();
        f.cutoff = 20000.0;
        let mut y = 0.0;
        for _ in 0..4410 {
            y = f.next(0.8, 44100.0);
        }
        assert!((y - 0.8).abs() < 0.05, "y={y}");
    }

    #[test]
    fn closed_cutoff_kills_high_freq() {
        let mut f = Ladder::new();
        f.cutoff = 20.0;
        let mut y = 0.0;
        // Alternating full-scale input: far above a 20 Hz cutoff.
        for i in 0..44100 {
            let x = if i % 2 == 0 { 1.0 } else { -1.0 };
            y = f.next(x, 44100.0);
        }
        assert!(y.abs() < 0.05, "y={y}");
    }

    #[test]
    fn max_resonance_stays_finite() {
        let mut f = Ladder::new();
        f.cutoff = 800.0;
        f.resonance = 1.0;
        for _ in 0..44100 {
            let y = f.next(0.5, 44100.0);
            assert!(y.is_finite() && y.abs() < 8.0, "y={y}");
        }
    }
}

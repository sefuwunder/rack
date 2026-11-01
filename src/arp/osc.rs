// Vendored from arp2600 (see README: vendored engines).
//! VCO: band-limited-ish classic waveforms with exponential FM, PWM,
//! and 1V/oct CV. Phase accumulates in cycles; the waveform functions
//! are evaluated from phase so FM stays in tune.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave {
    Saw,
    Square,
    Pulse,
    Sine,
    Triangle,
}

pub fn midi_to_freq(midi: f32) -> f32 {
    440.0 * 2.0f32.powf((midi - 69.0) / 12.0)
}

/// A single oscillator. `base_midi` is the note CV (keyboard, already
/// glided); coarse is semitones, fine is cents.
#[derive(Clone, Copy)]
pub struct Vco {
    pub wave: Wave,
    pub coarse: f32, // semitones, -24..24
    pub fine: f32,   // cents, -100..100
    pub pulse_width: f32, // 0.05..0.95, pulse wave only
    pub fm_amount: f32,   // octaves per unit FM input, 0..2
    pub fm_input: f32,    // summed FM signal, roughly -1..1
    pub cv_input: f32,    // extra 1V/oct CV (patchable), volts
    phase: f32,
}

impl Vco {
    pub fn new(wave: Wave) -> Self {
        Self {
            wave,
            coarse: 0.0,
            fine: 0.0,
            pulse_width: 0.5,
            fm_amount: 0.0,
            fm_input: 0.0,
            cv_input: 0.0,
            phase: 0.0,
        }
    }

    pub fn freq(&self, base_midi: f32) -> f32 {
        let midi = base_midi + self.coarse + self.fine / 100.0 + self.cv_input * 12.0;
        let mut f = midi_to_freq(midi);
        if self.fm_amount != 0.0 && self.fm_input != 0.0 {
            f *= 2.0f32.powf(self.fm_input * self.fm_amount);
        }
        f.clamp(0.05, 12000.0)
    }

    /// Advance one sample, returning the waveform value in -1..1.
    pub fn next(&mut self, base_midi: f32, sr: f32) -> f32 {
        let f = self.freq(base_midi);
        self.advance(f, sr);
        self.eval(self.wave)
    }

    /// Advance the phase without evaluating (voice.rs advances once
    /// per VCO, then reads every waveform from the shared phase).
    pub fn advance(&mut self, freq: f32, sr: f32) {
        self.phase = (self.phase + freq / sr).fract();
    }

    /// Evaluate a waveform at the current phase.
    pub fn eval(&self, wave: Wave) -> f32 {
        let p = self.phase;
        match wave {
            Wave::Saw => 2.0 * p - 1.0,
            Wave::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Pulse => {
                if p < self.pulse_width.clamp(0.05, 0.95) {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Sine => (p * std::f32::consts::TAU).sin(),
            Wave::Triangle => {
                if p < 0.5 {
                    4.0 * p - 1.0
                } else {
                    3.0 - 4.0 * p
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a440_is_440() {
        assert!((midi_to_freq(69.0) - 440.0).abs() < 0.01);
    }

    #[test]
    fn octave_doubles() {
        assert!((midi_to_freq(81.0) - 880.0).abs() < 0.01);
    }

    #[test]
    fn saw_spans_range() {
        let mut v = Vco::new(Wave::Saw);
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for _ in 0..4410 {
            let x = v.next(69.0, 44100.0);
            lo = lo.min(x);
            hi = hi.max(x);
        }
        assert!(lo < -0.9 && hi > 0.9);
    }

    #[test]
    fn fm_moves_freq() {
        let mut v = Vco::new(Wave::Sine);
        v.fm_amount = 1.0;
        v.fm_input = 1.0;
        assert!((v.freq(69.0) - 880.0).abs() < 1.0);
    }
}

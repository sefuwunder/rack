// Vendored from grandmother (see README: vendored engines).
//! Low-frequency oscillator: sine / triangle / saw / square, bipolar output.

use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LfoWave {
    Sine,
    Triangle,
    Saw,
    Square,
}

#[derive(Clone, Debug)]
pub struct Lfo {
    pub rate: f32,
    pub wave: LfoWave,
    phase: f32,
    wrapped: bool,
}

impl Lfo {
    pub fn new() -> Self {
        Lfo {
            rate: 1.0,
            wave: LfoWave::Triangle,
            phase: 0.0,
            wrapped: false,
        }
    }

    /// Reset the phase to the start of the cycle (SYNC IN behavior).
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.wrapped = false;
    }

    /// True if the most recent `next()` call wrapped past the end of the
    /// cycle. The S/H output samples the noise source on this edge.
    pub fn wrapped(&self) -> bool {
        self.wrapped
    }

    pub fn next(&mut self, sr: f32) -> f32 {
        let p = self.phase;
        self.phase += self.rate / sr;
        self.wrapped = self.phase >= 1.0;
        self.phase -= self.phase.floor();
        match self.wave {
            LfoWave::Sine => (TAU * p).sin(),
            LfoWave::Triangle => crate::gm::osc::triangle(p),
            LfoWave::Saw => crate::gm::osc::saw(p),
            LfoWave::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfo_waves_bounded_and_periodic() {
        for wave in [
            LfoWave::Sine,
            LfoWave::Triangle,
            LfoWave::Saw,
            LfoWave::Square,
        ] {
            let mut l = Lfo::new();
            l.rate = 10.0;
            l.wave = wave;
            let first = l.next(1000.0);
            let mut min = first;
            let mut max = first;
            for _ in 0..1000 {
                let v = l.next(1000.0);
                assert!(v >= -1.0 && v <= 1.0);
                min = min.min(v);
                max = max.max(v);
            }
            assert!(max - min > 1.5, "{wave:?} should swing near rail to rail");
        }
    }
}

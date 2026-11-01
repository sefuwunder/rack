// Vendored from arp2600 (see README: vendored engines).
//! Patch bay: output jacks, input jacks, cables, and normalization.
//! Patching an input breaks its normalized connection; one cable per
//! input (a new cable replaces the old one). All patch reads use the
//! previous sample's jack values, which keeps every feedback patch
//! stable with a single sample of latency.

pub mod out {
    pub const VCO1_SAW: usize = 0;
    pub const VCO1_SQR: usize = 1;
    pub const VCO2_SAW: usize = 2;
    pub const VCO2_PLS: usize = 3;
    pub const VCO2_SIN: usize = 4;
    pub const VCO2_TRI: usize = 5;
    pub const VCO3_SAW: usize = 6;
    pub const VCO3_PLS: usize = 7;
    pub const VCO3_SIN: usize = 8;
    pub const VCO3_TRI: usize = 9;
    pub const NOISE_W: usize = 10;
    pub const NOISE_P: usize = 11;
    pub const RING: usize = 12;
    pub const SH: usize = 13;
    pub const VCF: usize = 14;
    pub const VCA: usize = 15;
    pub const ADSR: usize = 16;
    pub const AR: usize = 17;
    pub const KB_CV: usize = 18;
    pub const GATE: usize = 19;
    pub const LAG: usize = 20;
    pub const MIXER: usize = 21;
    pub const REVERB: usize = 22;
    pub const VOICE: usize = 23;
    pub const PRE: usize = 24;
    pub const ENV: usize = 25;
    pub const COUNT: usize = 26;
}

pub mod inp {
    pub const VCO1_FM1: usize = 0;
    pub const VCO1_FM2: usize = 1;
    pub const VCO1_CV: usize = 2;
    pub const VCO2_FM1: usize = 3;
    pub const VCO2_FM2: usize = 4;
    pub const VCO2_CV: usize = 5;
    pub const VCO2_PWM: usize = 6;
    pub const VCO3_FM1: usize = 7;
    pub const VCO3_FM2: usize = 8;
    pub const VCO3_CV: usize = 9;
    pub const VCO3_PWM: usize = 10;
    pub const VCF_CV1: usize = 11;
    pub const VCF_CV2: usize = 12;
    pub const VCF_IN1: usize = 13;
    pub const VCF_IN2: usize = 14;
    pub const VCA_CV1: usize = 15;
    pub const VCA_CV2: usize = 16;
    pub const VCA_IN: usize = 17;
    pub const RING_IN1: usize = 18;
    pub const RING_IN2: usize = 19;
    pub const SH_IN: usize = 20;
    pub const SH_CLK: usize = 21;
    pub const LAG_IN: usize = 22;
    pub const MIX_IN1: usize = 23;
    pub const MIX_IN2: usize = 24;
    pub const MIX_IN3: usize = 25;
    pub const MIX_IN4: usize = 26;
    pub const REV_IN1: usize = 27;
    pub const REV_IN2: usize = 28;
    pub const PRE_IN: usize = 29;
    pub const COUNT: usize = 30;
}

pub const OUT_LABELS: [&str; out::COUNT] = [
    "V1 SAW", "V1 SQR", "V2 SAW", "V2 PLS", "V2 SIN", "V2 TRI", "V3 SAW", "V3 PLS",
    "V3 SIN", "V3 TRI", "NOISE W", "NOISE P", "RING", "S&H", "VCF", "VCA", "ADSR",
    "AR", "KB CV", "GATE", "LAG", "MIXER", "REVERB", "VOICE", "PRE", "ENV",
];

pub const IN_LABELS: [&str; inp::COUNT] = [
    "V1 FM1", "V1 FM2", "V1 CV", "V2 FM1", "V2 FM2", "V2 CV", "V2 PWM", "V3 FM1",
    "V3 FM2", "V3 CV", "V3 PWM", "VCF CV1", "VCF CV2", "VCF IN1", "VCF IN2",
    "VCA CV1", "VCA CV2", "VCA IN", "RING 1", "RING 2", "S&H IN", "S&H CLK",
    "LAG IN", "MIX 1", "MIX 2", "MIX 3", "MIX 4", "REV 1", "REV 2", "PRE IN",
];

/// Normalized connections: (input, default output).
pub const NORMALIZED: &[(usize, usize)] = &[
    (inp::VCO1_CV, out::KB_CV),
    (inp::VCO2_CV, out::KB_CV),
    (inp::VCO3_CV, out::KB_CV),
    (inp::VCF_IN1, out::VOICE),
    (inp::VCF_CV1, out::AR),
    (inp::VCA_IN, out::VCF),
    (inp::VCA_CV1, out::ADSR),
    (inp::RING_IN1, out::VCO1_SAW),
    (inp::RING_IN2, out::VCO2_SIN),
    (inp::SH_IN, out::NOISE_W),
    (inp::LAG_IN, out::KB_CV),
    (inp::MIX_IN1, out::VCO1_SAW),
    (inp::MIX_IN2, out::VCO2_SAW),
    (inp::MIX_IN3, out::VCO3_SAW),
    (inp::MIX_IN4, out::NOISE_W),
    (inp::REV_IN1, out::VCA),
    (inp::PRE_IN, out::VCA),
];

#[derive(Clone, Default)]
pub struct Patch {
    /// cables[input] = Some(output), one cable per input.
    pub cables: Vec<Option<usize>>,
}

impl Patch {
    pub fn new() -> Self {
        Self {
            cables: vec![None; inp::COUNT],
        }
    }

    pub fn connect(&mut self, output: usize, input: usize) {
        if output < out::COUNT && input < inp::COUNT {
            self.cables[input] = Some(output);
        }
    }

    pub fn disconnect(&mut self, input: usize) {
        if input < inp::COUNT {
            self.cables[input] = None;
        }
    }

    pub fn cable_for(&self, input: usize) -> Option<usize> {
        self.cables.get(input).copied().flatten()
    }

    /// Effective source for an input: cable wins, else normalization,
    /// else silence.
    pub fn source_for(&self, input: usize) -> Option<usize> {
        if let Some(o) = self.cable_for(input) {
            return Some(o);
        }
        NORMALIZED
            .iter()
            .find(|(i, _)| *i == input)
            .map(|(_, o)| *o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_and_override() {
        let mut p = Patch::new();
        assert_eq!(p.source_for(inp::VCF_IN1), Some(out::VOICE));
        assert_eq!(p.source_for(inp::VCF_IN2), None);
        p.connect(out::VCO1_SAW, inp::VCF_IN1);
        assert_eq!(p.source_for(inp::VCF_IN1), Some(out::VCO1_SAW));
        p.disconnect(inp::VCF_IN1);
        assert_eq!(p.source_for(inp::VCF_IN1), Some(out::VOICE));
    }

    #[test]
    fn preamp_input_normalizes_to_vca() {
        let p = Patch::new();
        assert_eq!(p.source_for(inp::PRE_IN), Some(out::VCA));
    }

    #[test]
    fn one_cable_per_input() {
        let mut p = Patch::new();
        p.connect(out::VCO1_SAW, inp::VCA_IN);
        p.connect(out::VCF, inp::VCA_IN);
        assert_eq!(p.cable_for(inp::VCA_IN), Some(out::VCF));
    }
}

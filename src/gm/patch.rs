// Vendored from grandmother (see README: vendored engines).
//! The patch bay: 19 outputs, 20 inputs, and a parallel-wired 4-jack
//! mult — 43 points covering every function of the hardware's front-panel
//! bay, plus the variant's extras (per-waveform oscillator outputs, the
//! second envelope output, a dedicated noise output, OSC2 pulse-width CV,
//! and a keyboard-gate override input).
//!
//! Routing rules, matching semi-modular hardware:
//! - SWITCHED inputs: patching a cable breaks the normalized connection.
//!   Multiple cables into one input sum.
//! - ADDITIVE inputs (VCF CUTOFF, VCA CV, LIN FM, ENV AMT): patched CV
//!   adds on top of the normalized modulation, which keeps working.
//! - MULT: the four jacks share one bus. Any output patched into any
//!   mult jack appears at all four; multiple outputs into the mult sum.
//!   The bus resolves on previous-sample values, so any mult path —
//!   including feedback through the mult — is stable with a one-sample
//!   delay. A mult jack patched into another mult jack is a no-op.
//!
//! Interaction: click an output jack (or a mult jack, with nothing
//! selected) to select it, then click an input jack (or a mult jack) to
//! patch. Clicking an input with no source selected removes its most
//! recent cable. Clicking a selected source again deselects it.

/// Output jacks. The first 11 are the original set, in order; the rest
/// are appended so existing indices stay stable.
pub const OUT_JACKS: [&str; 19] = [
    "KB CV", "GATE", "LFO", "OSC1 SAW", "OSC1 PULSE", "OSC2 SAW", "OSC2 TRI", "NOISE",
    "FILT EG", "AMP EG", "VCF OUT", "KB VEL", "S/H OUT", "OSC1 WAVE", "OSC2 WAVE",
    "MIXER OUT", "HP OUT", "ATT OUT", "-ENV OUT",
];

/// Input jacks. The first 9 are the original set, in order.
pub const IN_JACKS: [&str; 20] = [
    "OSC1 PITCH",
    "OSC2 PITCH",
    "OSC1 PW",
    "OSC2 PW",
    "VCF IN",
    "VCF CUTOFF",
    "VCA CV",
    "GATE IN",
    "LFO RATE",
    "SYNC IN",
    "LIN FM IN",
    "MIX OSC1",
    "MIX OSC2",
    "MIX NOISE",
    "HP IN",
    "ATT IN",
    "ENV AMT",
    "TRIG IN",
    "VCA IN",
    "REVERB IN",
];

/// Number of parallel-wired mult jacks.
pub const MULT_JACKS: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InKind {
    /// Patching breaks the normalized connection.
    Switched,
    /// Patching adds to the normalized modulation.
    Additive,
}

pub const IN_KINDS: [InKind; 20] = [
    InKind::Switched, // OSC1 PITCH <- KB CV
    InKind::Switched, // OSC2 PITCH <- KB CV
    InKind::Switched, // OSC1 PW <- none
    InKind::Switched, // OSC2 PW <- none
    InKind::Switched, // VCF IN <- mixer
    InKind::Additive, // VCF CUTOFF (EG + KB track keep working)
    InKind::Additive, // VCA CV (amp EG keeps working)
    InKind::Switched, // GATE IN <- KB gate
    InKind::Switched, // LFO RATE <- none
    InKind::Switched, // SYNC IN (edge trigger, no normal)
    InKind::Additive, // LIN FM IN (adds linear Hz to OSC2)
    InKind::Switched, // MIX OSC1 <- OSC1 wave
    InKind::Switched, // MIX OSC2 <- OSC2 wave
    InKind::Switched, // MIX NOISE <- noise
    InKind::Switched, // HP IN <- none (utility filter)
    InKind::Switched, // ATT IN <- none (utility attenuator)
    InKind::Additive, // ENV AMT (adds to the ENV AMT knob)
    InKind::Switched, // TRIG IN <- KB gate
    InKind::Switched, // VCA IN <- filter output
    InKind::Switched, // REVERB IN <- VCA output
];

/// Input jack indices, for the voice engine.
pub mod inp {
    pub const OSC1_PITCH: usize = 0;
    pub const OSC2_PITCH: usize = 1;
    pub const OSC1_PW: usize = 2;
    pub const OSC2_PW: usize = 3;
    pub const VCF_IN: usize = 4;
    pub const VCF_CUTOFF: usize = 5;
    pub const VCA_CV: usize = 6;
    pub const GATE_IN: usize = 7;
    pub const LFO_RATE: usize = 8;
    pub const SYNC_IN: usize = 9;
    pub const LIN_FM_IN: usize = 10;
    pub const MIX_OSC1: usize = 11;
    pub const MIX_OSC2: usize = 12;
    pub const MIX_NOISE: usize = 13;
    pub const HP_IN: usize = 14;
    pub const ATT_IN: usize = 15;
    pub const ENV_AMT: usize = 16;
    pub const TRIG_IN: usize = 17;
    pub const VCA_IN: usize = 18;
    pub const REVERB_IN: usize = 19;
}

/// Output jack indices, for the voice engine.
pub mod out {
    pub const KB_CV: usize = 0;
    pub const GATE: usize = 1;
    pub const LFO: usize = 2;
    pub const OSC1_SAW: usize = 3;
    pub const OSC1_PULSE: usize = 4;
    pub const OSC2_SAW: usize = 5;
    pub const OSC2_TRI: usize = 6;
    pub const NOISE: usize = 7;
    pub const FILT_EG: usize = 8;
    pub const AMP_EG: usize = 9;
    pub const VCF_OUT: usize = 10;
    pub const KB_VEL: usize = 11;
    pub const SH_OUT: usize = 12;
    pub const OSC1_WAVE: usize = 13;
    pub const OSC2_WAVE: usize = 14;
    pub const MIXER_OUT: usize = 15;
    pub const HP_OUT: usize = 16;
    pub const ATT_OUT: usize = 17;
    pub const NEG_ENV: usize = 18;
}

/// A cable source: a real output, or the shared mult bus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Src {
    Out(usize),
    Mult(usize),
}

/// A cable destination: a real input, or a mult jack (feeding the bus).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dst {
    In(usize),
    Mult(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cable {
    pub src: Src,
    pub dst: Dst,
}

#[derive(Clone, Debug, Default)]
pub struct PatchBay {
    pub cables: Vec<Cable>,
    pub selected: Option<Src>,
}

impl PatchBay {
    pub fn new() -> Self {
        PatchBay {
            cables: Vec::new(),
            selected: None,
        }
    }

    pub fn clear(&mut self) {
        self.cables.clear();
        self.selected = None;
    }

    /// Is any cable patched into this input?
    pub fn patched(&self, inp: usize) -> bool {
        self.cables.iter().any(|c| c.dst == Dst::In(inp))
    }

    /// Sources of all cables into this input, in patch order.
    pub fn sources_into(&self, inp: usize) -> Vec<Src> {
        self.cables
            .iter()
            .filter(|c| c.dst == Dst::In(inp))
            .map(|c| c.src)
            .collect()
    }

    /// Sources of all cables into any mult jack, in patch order. Only
    /// real outputs feed the bus; mult-to-mult cables are no-ops.
    pub fn mult_sources(&self) -> Vec<usize> {
        self.cables
            .iter()
            .filter_map(|c| match (c.src, c.dst) {
                (Src::Out(o), Dst::Mult(_)) => Some(o),
                _ => None,
            })
            .collect()
    }

    fn toggle(&mut self, src: Src, dst: Dst) {
        if let Some(pos) = self.cables.iter().position(|c| c.src == src && c.dst == dst) {
            self.cables.remove(pos);
        } else {
            self.cables.push(Cable { src, dst });
        }
    }

    /// Click an output jack: select it, or deselect if already selected.
    pub fn click_out(&mut self, out: usize) {
        let src = Src::Out(out);
        if self.selected == Some(src) {
            self.selected = None;
        } else {
            self.selected = Some(src);
        }
    }

    /// Click a mult jack: with an output selected, patch into the mult
    /// (or unpatch that exact cable). With a mult selected, mult-to-mult
    /// is a no-op, so just deselect. With nothing selected, select the
    /// mult as a source.
    pub fn click_mult(&mut self, m: usize) {
        match self.selected.take() {
            Some(Src::Out(o)) => self.toggle(Src::Out(o), Dst::Mult(m)),
            Some(Src::Mult(_)) => {}
            None => self.selected = Some(Src::Mult(m)),
        }
    }

    /// Click an input jack: with a source selected, patch (or unpatch if
    /// that exact cable exists). With nothing selected, remove the most
    /// recent cable into this input, if any.
    pub fn click_in(&mut self, inp: usize) {
        if let Some(s) = self.selected.take() {
            self.toggle(s, Dst::In(inp));
        } else if let Some(pos) = self.cables.iter().rposition(|c| c.dst == Dst::In(inp)) {
            self.cables.remove(pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_unpatch_and_pop() {
        let mut p = PatchBay::new();
        p.click_out(2); // LFO
        p.click_in(5); // VCF CUTOFF
        assert_eq!(
            p.cables,
            vec![Cable {
                src: Src::Out(2),
                dst: Dst::In(5)
            }]
        );
        assert!(p.patched(5));
        // Clicking the same pair again unpatches.
        p.click_out(2);
        p.click_in(5);
        assert!(p.cables.is_empty());
        // Two cables into one input; unselected click pops the last.
        p.click_out(2);
        p.click_in(5);
        p.click_out(8);
        p.click_in(5);
        assert_eq!(p.sources_into(5), vec![Src::Out(2), Src::Out(8)]);
        p.click_in(5);
        assert_eq!(p.sources_into(5), vec![Src::Out(2)]);
        p.clear();
        assert!(p.cables.is_empty() && p.selected.is_none());
    }

    #[test]
    fn out_click_toggles_selection() {
        let mut p = PatchBay::new();
        p.click_out(3);
        assert_eq!(p.selected, Some(Src::Out(3)));
        p.click_out(3);
        assert_eq!(p.selected, None);
        p.click_out(3);
        p.click_out(4);
        assert_eq!(p.selected, Some(Src::Out(4)));
    }

    #[test]
    fn mult_patch_flow() {
        let mut p = PatchBay::new();
        // Out -> mult, then mult -> input.
        p.click_out(3); // OSC1 SAW
        p.click_mult(1);
        assert_eq!(
            p.cables,
            vec![Cable {
                src: Src::Out(3),
                dst: Dst::Mult(1)
            }]
        );
        assert!(p.mult_sources() == vec![3]);
        p.click_mult(0); // select mult 0 as source
        assert_eq!(p.selected, Some(Src::Mult(0)));
        p.click_in(5);
        assert_eq!(
            p.cables[1],
            Cable {
                src: Src::Mult(0),
                dst: Dst::In(5)
            }
        );
        // Same pair again unpatches.
        p.click_mult(0);
        p.click_in(5);
        assert_eq!(p.cables.len(), 1);
        // Mult-to-mult is a no-op that just deselects.
        p.click_mult(2);
        p.click_mult(3);
        assert_eq!(p.cables.len(), 1);
        assert_eq!(p.selected, None);
        // Out -> mult toggles off the same way.
        p.click_out(3);
        p.click_mult(1);
        assert!(p.cables.is_empty());
    }
}

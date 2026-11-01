// Vendored from grandmother (see README: vendored engines).
//! 16-step sequencer: the hardware's other half, in miniature (the
//! hardware holds 256-note sequences in 3 slots; this one holds 16
//! steps, edited on the faceplate).
//!
//! Each step has a note, a gate, and an accent. Playback drives the
//! voice directly and overrides held keys and the arpeggiator while it
//! runs; a rest step silences the voice rather than falling through to
//! held keys. Accented steps play at full velocity, others at 0.7.
//!
//! Editing: click a step to select it; GATE / ACCENT buttons toggle the
//! selected step; REC arms note entry — played notes (keyboard, screen
//! keys, or MIDI) fill steps from step 0 and advance, switching off
//! after step 16. Editing the gate/accent works while playing; note
//! entry only happens while stopped and REC is armed, so casual playing
//! never overwrites steps.

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SeqStep {
    pub note: u8,
    pub gate: bool,
    pub accent: bool,
}

impl Default for SeqStep {
    fn default() -> Self {
        SeqStep {
            note: 60,
            gate: true,
            accent: false,
        }
    }
}

pub const STEPS: usize = 16;

#[derive(Clone, Debug)]
pub struct Seq {
    pub steps: [SeqStep; STEPS],
    pub playing: bool,
    pub rec: bool,
    pub selected: usize,
    pub current: usize,
    samples: u32,
}

impl Default for Seq {
    fn default() -> Self {
        let mut steps = [SeqStep::default(); STEPS];
        // Four-on-the-floor accents on an otherwise plain C4 pulse.
        for s in [0, 4, 8, 12] {
            steps[s].accent = true;
        }
        Seq {
            steps,
            playing: false,
            rec: false,
            selected: 0,
            current: 0,
            samples: 0,
        }
    }
}

impl Seq {
    pub fn new() -> Self {
        Seq::default()
    }

    /// The note/velocity the current step produces, if its gate is open.
    pub fn apply_current(&self) -> (Option<u8>, f32) {
        let s = self.steps[self.current];
        if s.gate {
            (Some(s.note), if s.accent { 1.0 } else { 0.7 })
        } else {
            (None, 0.0)
        }
    }

    /// Start playback from step 0. Returns the first step's output.
    pub fn play(&mut self) -> (Option<u8>, f32) {
        self.playing = true;
        self.rec = false;
        self.current = 0;
        self.samples = 0;
        self.apply_current()
    }

    pub fn stop(&mut self) {
        self.playing = false;
        self.rec = false;
    }

    /// Arm note entry from step 0. If playing, stop first, then arm.
    pub fn toggle_rec(&mut self) {
        if self.playing {
            self.stop();
        }
        self.rec = !self.rec;
        if self.rec {
            self.selected = 0;
        }
    }

    /// Advance one step. Returns the new step's output.
    pub fn advance(&mut self) -> (Option<u8>, f32) {
        self.current = (self.current + 1) % STEPS;
        self.samples = 0;
        self.apply_current()
    }

    /// Sample-clock tick. Returns Some output when a step boundary is
    /// crossed, else None. `rate` is steps per second.
    pub fn tick(&mut self, rate: f32, sr: f32) -> Option<(Option<u8>, f32)> {
        if !self.playing {
            return None;
        }
        self.samples += 1;
        let interval = (sr / rate.max(0.1)) as u32;
        if self.samples >= interval.max(1) {
            Some(self.advance())
        } else {
            None
        }
    }

    pub fn select(&mut self, i: usize) {
        self.selected = i.min(STEPS - 1);
    }

    pub fn toggle_gate(&mut self) {
        let s = &mut self.steps[self.selected];
        s.gate = !s.gate;
    }

    pub fn toggle_accent(&mut self) {
        let s = &mut self.steps[self.selected];
        s.accent = !s.accent;
    }

    pub fn clear(&mut self) {
        for s in self.steps.iter_mut() {
            s.gate = false;
            s.accent = false;
        }
    }

    /// Record one played note into the selected step (entry only while
    /// stopped and REC is armed). Advances; wraps off REC after step 16.
    pub fn record_note(&mut self, note: u8) {
        if !self.rec || self.playing {
            return;
        }
        self.steps[self.selected] = SeqStep {
            note,
            gate: true,
            accent: false,
        };
        self.selected += 1;
        if self.selected >= STEPS {
            self.selected = 0;
            self.rec = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_cycles_and_accents() {
        let mut s = Seq::new();
        assert_eq!(s.apply_current(), (Some(60), 1.0)); // step 0 accented
        let (n, v) = s.advance();
        assert_eq!((n, v), (Some(60), 0.7)); // step 1 plain
        for _ in 0..14 {
            s.advance();
        }
        assert_eq!(s.current, 15);
        s.advance();
        assert_eq!(s.current, 0);
    }

    #[test]
    fn rest_step_silences() {
        let mut s = Seq::new();
        s.steps[0].gate = false;
        assert_eq!(s.apply_current(), (None, 0.0));
    }

    #[test]
    fn record_fills_and_wraps_off() {
        let mut s = Seq::new();
        s.toggle_rec();
        assert!(s.rec && s.selected == 0);
        s.record_note(65);
        assert_eq!(s.steps[0].note, 65);
        assert!(s.steps[0].gate);
        assert_eq!(s.selected, 1);
        for n in 0..15u8 {
            s.record_note(60 + n);
        }
        assert!(!s.rec, "REC should switch off after step 16");
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn record_ignored_unless_armed() {
        let mut s = Seq::new();
        s.record_note(65);
        assert_ne!(s.steps[0].note, 65);
    }

    #[test]
    fn gate_and_accent_toggle() {
        let mut s = Seq::new();
        s.select(3);
        s.toggle_gate();
        assert!(!s.steps[3].gate);
        s.toggle_accent();
        assert!(s.steps[3].accent);
    }
}

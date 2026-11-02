//! Real-time audio plumbing: the UI thread and the audio thread
//! share no locks. The UI sends commands down one channel; the
//! audio thread owns the Rack, drains commands between blocks,
//! renders, and sends a UI snapshot back up a second channel.
//! Both directions are non-blocking on the audio side (an
//! unbounded send never blocks, try_recv never blocks), so the
//! UI can stall, decode a sample, or write a file without ever
//! glitching audio. The SDL callback owns the AudioEngine, so
//! no Arc<Mutex> sits on the render path at all.

use crate::arp;
use crate::gm;
use crate::panel::KnobId;
use crate::project;
use crate::rack::{CvIn, FilterIn, GateIn, Rack};
use crate::sp::decode::AudioData;
use crate::sp::dsp::FilterKind;
use crate::sp::engine::{BPM_MAX, BPM_MIN, MAX_STEPS, N_PADS};
use std::sync::mpsc::{Receiver, Sender};

/// One UI action, applied by the audio thread between blocks.
/// Step commands (BpmStep, SwingStep, OctaveStep, KnobStep,
/// GateNoteStep) carry deltas rather than absolute values so
/// the UI never has to read-modify-write through a stale
/// snapshot: two wheel ticks in one frame must both land.
pub enum Cmd {
    Transport,
    BpmStep(f64),
    SwingStep(f64),
    SelectPad(usize),
    TriggerPad(usize),
    ToggleStep(usize),
    CycleSpFilter,
    SetKnobT(KnobId, f32),
    KnobStep(KnobId, f32),
    CycleFilterIn(usize),
    NoteOn { dev: usize, midi: u8 },
    NoteOff { dev: usize, midi: u8 },
    ToggleLoop,
    ToggleMute(usize),
    SelectSynth(usize),
    OctaveStep { dev: usize, dir: i32 },
    ToggleGate { synth: usize, step: usize },
    GateNote { synth: usize, step: usize, note: i8 },
    GateNoteStep { synth: usize, step: usize, dir: i8 },
    SetFilterIn { synth: usize, value: FilterIn },
    SetGateIn { synth: usize, value: GateIn },
    SetCvIn { synth: usize, value: CvIn },
    LoadSample { data: AudioData, name: String, path: Option<String> },
    LoadProject(Rack),
    SaveRequest(Sender<String>),
}

/// One pad's UI-visible scalars. The sample audio itself never
/// crosses to the UI; only the label and knob values do.
pub struct PadView {
    pub filter_kind: FilterKind,
    pub filter_cutoff: f32,
    pub filter_res: f32,
    pub lfo_rate: f32,
    pub lfo_cut: f32,
    pub lfo_res: f32,
    pub level: f32,
    pub tune_st: f32,
    pub label: Option<String>,
}

impl PadView {
    fn from_pad(p: &crate::sp::engine::Pad) -> Self {
        PadView {
            filter_kind: p.filter_kind,
            filter_cutoff: p.filter_cutoff,
            filter_res: p.filter_res,
            lfo_rate: p.lfo_rate,
            lfo_cut: p.lfo_cut,
            lfo_res: p.lfo_res,
            level: p.level,
            tune_st: p.tune_st,
            label: p.label.clone(),
        }
    }
}

pub struct SpView {
    pub playing: bool,
    pub bpm: f64,
    pub swing: f64,
    pub last_step: u32,
    pub patterns: [[bool; MAX_STEPS]; N_PADS],
    pub pads: [PadView; 8],
}

pub struct GmView {
    pub params: gm::voice::Params,
}

pub struct ArpView {
    pub params: arp::voice::Params,
}

pub struct SampView {
    pub name: String,
    pub level: f32,
    pub tune_st: f32,
    pub start: f32,
    pub octave: i32,
    pub loop_: bool,
}

impl SampView {
    /// Base note for the sampler keyboard (C3 + octave).
    /// Mirrors Sampler::base_note so hit-testing agrees.
    pub fn base_note(&self) -> u8 {
        (48 + self.octave * 12).clamp(12, 96) as u8
    }
}

/// Everything the panels draw or hit-test, copied out of the
/// Rack once per block. Field names mirror Rack's so the panel
/// code reads unchanged; only the receiver type differs.
pub struct View {
    pub sp: SpView,
    pub gm: GmView,
    pub arp: ArpView,
    pub samp: SampView,
    pub level: [f32; 4],
    pub mute: [bool; 4],
    pub master: f32,
    pub gm_filter_in: FilterIn,
    pub arp_filter_in: FilterIn,
    pub gm_gate_in: GateIn,
    pub arp_gate_in: GateIn,
    pub gm_cv_in: CvIn,
    pub arp_cv_in: CvIn,
    pub gm_gate: [bool; 16],
    pub arp_gate: [bool; 16],
    pub gm_gate_notes: [i8; 16],
    pub arp_gate_notes: [i8; 16],
    pub gm_octave: i32,
    pub arp_octave: i32,
    pub selected: usize,
    pub sp_sel: usize,
    pub peak: [f32; 4],
    pub master_peak: f32,
}

impl View {
    pub fn from_rack(r: &Rack) -> Self {
        View {
            sp: SpView {
                playing: r.sp.playing,
                bpm: r.sp.bpm,
                swing: r.sp.swing,
                last_step: r.sp.last_step,
                patterns: r.sp.patterns,
                pads: std::array::from_fn(|i| PadView::from_pad(&r.sp.pads[i])),
            },
            gm: GmView { params: r.gm.params.clone() },
            arp: ArpView { params: r.arp.params.clone() },
            samp: SampView {
                name: r.samp.name.clone(),
                level: r.samp.level,
                tune_st: r.samp.tune_st,
                start: r.samp.start,
                octave: r.samp.octave,
                loop_: r.samp.loop_,
            },
            level: r.level,
            mute: r.mute,
            master: r.master,
            gm_filter_in: r.gm_filter_in,
            arp_filter_in: r.arp_filter_in,
            gm_gate_in: r.gm_gate_in,
            arp_gate_in: r.arp_gate_in,
            gm_cv_in: r.gm_cv_in,
            arp_cv_in: r.arp_cv_in,
            gm_gate: r.gm_gate,
            arp_gate: r.arp_gate,
            gm_gate_notes: r.gm_gate_notes,
            arp_gate_notes: r.arp_gate_notes,
            gm_octave: r.gm_octave,
            arp_octave: r.arp_octave,
            selected: r.selected,
            sp_sel: r.sp_sel,
            peak: r.peak,
            master_peak: r.master_peak,
        }
    }

    /// Base note for a device's keyboard and gate row (C3 + octave).
    /// Mirrors Rack::base_note so hit-testing agrees with audio.
    pub fn base_note(&self, synth: usize) -> u8 {
        match synth {
            0 => (48 + self.gm_octave * 12).clamp(12, 96) as u8,
            1 => (48 + self.arp_octave * 12).clamp(12, 96) as u8,
            _ => (48 + self.samp.octave * 12).clamp(12, 96) as u8,
        }
    }
}

impl Default for View {
    fn default() -> Self {
        Self::from_rack(&Rack::new())
    }
}

/// The audio thread's half: owns the Rack, drains commands,
/// renders, publishes a snapshot. The SDL callback owns this
/// struct, so nothing here is shared with the UI thread.
pub struct AudioEngine {
    rack: Rack,
    rx: Receiver<Cmd>,
    view_tx: Sender<View>,
}

impl AudioEngine {
    pub fn new(rack: Rack, rx: Receiver<Cmd>, view_tx: Sender<View>) -> Self {
        AudioEngine { rack, rx, view_tx }
    }

    pub fn render_block(&mut self, out: &mut [f32]) {
        // Drain commands, bounded per block so a UI flood can't
        // starve the render (human input never gets near the cap).
        for _ in 0..512 {
            match self.rx.try_recv() {
                Ok(cmd) => self.apply(cmd),
                Err(_) => break,
            }
        }
        self.rack.render_block(out);
        // Snapshot after the render (cutoffs already restored).
        // If the UI went away, the send fails silently and the
        // engine keeps rendering.
        let _ = self.view_tx.send(View::from_rack(&self.rack));
    }

    fn set_knob_t(&mut self, id: KnobId, t: f32) {
        let (lo, hi, log) = id.spec();
        let t = t.clamp(0.0, 1.0);
        let v = if log { lo * (hi / lo).powf(t) } else { lo + (hi - lo) * t };
        self.set_knob(id, v);
    }

    fn set_knob(&mut self, id: KnobId, v: f32) {
        use KnobId::*;
        let r = &mut self.rack;
        let p = r.sp_sel;
        match id {
            SpCutoff => {
                let k = r.sp.pads[p].filter_kind;
                r.sp.set_filter(p, k, v);
            }
            SpRes => r.sp.set_res(p, v),
            SpLfoRate => r.sp.set_lfo_rate(p, v),
            SpLfoCut => r.sp.set_lfo_cut(p, v),
            SpLfoRes => r.sp.set_lfo_res(p, v),
            SpLevel => r.sp.pads[p].level = v.clamp(0.0, 1.0),
            GmOsc1 => r.gm.params.osc1_level = v,
            GmOsc2 => r.gm.params.osc2_level = v,
            GmCutoff => r.gm.params.cutoff = v,
            GmReso => r.gm.params.reso = v,
            GmAttack => r.gm.params.a_attack = v,
            GmDecay => r.gm.params.a_decay = v,
            GmSustain => r.gm.params.a_sustain = v,
            GmRelease => r.gm.params.a_release = v,
            GmGlide => r.gm.params.glide = v,
            GmLfoRate => r.gm.params.lfo_rate = v,
            ArpOsc1 => r.arp.params.vco1_level = v,
            ArpOsc2 => r.arp.params.vco2_level = v,
            ArpOsc3 => r.arp.params.vco3_level = v,
            ArpCutoff => r.arp.params.vcf_cutoff = v,
            ArpReso => r.arp.params.vcf_reso = v,
            ArpAttack => r.arp.params.adsr_a = v,
            ArpDecay => r.arp.params.adsr_d = v,
            ArpSustain => r.arp.params.adsr_s = v,
            ArpRelease => r.arp.params.adsr_r = v,
            ArpGlide => r.arp.params.glide = v,
            MixLevel(i) => {
                if i < 4 {
                    r.level[i] = v;
                }
            }
            MixMaster => r.master = v,
            SpTune => r.sp.pads[p].tune_st = v.clamp(-24.0, 24.0),
            GmNoise => r.gm.params.noise_level = v,
            GmEnvAmt => r.gm.params.env_amt = v,
            GmKbTrack => r.gm.params.kb_track = v,
            GmFAttack => r.gm.params.f_attack = v,
            GmFDecay => r.gm.params.f_decay = v,
            GmFSustain => r.gm.params.f_sustain = v,
            GmFRelease => r.gm.params.f_release = v,
            GmReverb => r.gm.params.reverb_mix = v,
            ArpNoise => r.arp.params.noise_level = v,
            ArpRing => r.arp.params.ring_level = v,
            ArpShRate => r.arp.params.sh_rate = v,
            ArpLag => r.arp.params.lag_time = v,
            ArpVcaGain => r.arp.params.vca_gain = v,
            ArpArAttack => r.arp.params.ar_a = v,
            ArpArRelease => r.arp.params.ar_r = v,
            ArpReverb => r.arp.params.rev_mix = v,
            SampLevel => r.samp.level = v.clamp(0.0, 1.0),
            SampTune => r.samp.tune_st = v.clamp(-24.0, 24.0),
            SampStart => r.samp.start = v.clamp(0.0, 1.0),
        }
    }

    fn apply(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Transport => self.rack.toggle_transport(),
            Cmd::BpmStep(d) => {
                self.rack.sp.bpm = (self.rack.sp.bpm + d).clamp(BPM_MIN, BPM_MAX);
            }
            Cmd::SwingStep(d) => {
                self.rack.sp.swing = (self.rack.sp.swing + d).clamp(50.0, 75.0);
            }
            Cmd::SelectPad(i) => self.rack.sp_sel = i.min(7),
            Cmd::TriggerPad(i) => self.rack.sp.trigger(i),
            Cmd::ToggleStep(i) => {
                if i < 16 {
                    let p = self.rack.sp_sel;
                    self.rack.sp.patterns[p][i] = !self.rack.sp.patterns[p][i];
                }
            }
            Cmd::CycleSpFilter => {
                let p = self.rack.sp_sel;
                let k = self.rack.sp.pads[p].filter_kind.cycle();
                let c = self.rack.sp.pads[p].filter_cutoff;
                self.rack.sp.set_filter(p, k, c);
            }
            Cmd::SetKnobT(id, t) => self.set_knob_t(id, t),
            Cmd::KnobStep(id, dt) => {
                // Current position from the live Rack, via the
                // same View + t() the UI reads: no duplicated
                // get logic, and bursts of wheel ticks all land.
                let v = View::from_rack(&self.rack);
                self.set_knob_t(id, id.t(&v) + dt);
            }
            Cmd::CycleFilterIn(synth) => self.rack.cycle_filter_in(synth),
            Cmd::NoteOn { dev, midi } => match dev {
                0 => self.rack.gm.voice.note_on(midi),
                1 => self.rack.arp.note_on(midi as f32),
                _ => self.rack.samp.note_on(midi),
            },
            Cmd::NoteOff { dev, midi } => match dev {
                0 => self.rack.gm.voice.note_off(midi),
                1 => self.rack.arp.note_off(midi as f32),
                _ => self.rack.samp.note_off(midi),
            },
            Cmd::ToggleLoop => self.rack.samp.loop_ = !self.rack.samp.loop_,
            Cmd::ToggleMute(ch) => {
                if ch < 4 {
                    self.rack.mute[ch] = !self.rack.mute[ch];
                }
            }
            Cmd::SelectSynth(s) => self.rack.selected = s.min(2),
            Cmd::OctaveStep { dev, dir } => match dev {
                0 => self.rack.gm_octave = (self.rack.gm_octave + dir).clamp(-2, 2),
                1 => self.rack.arp_octave = (self.rack.arp_octave + dir).clamp(-2, 2),
                _ => self.rack.samp.octave = (self.rack.samp.octave + dir).clamp(-2, 2),
            },
            Cmd::ToggleGate { synth, step } => {
                if step < 16 {
                    if synth == 0 {
                        self.rack.gm_gate[step] = !self.rack.gm_gate[step];
                    } else {
                        self.rack.arp_gate[step] = !self.rack.arp_gate[step];
                    }
                }
            }
            Cmd::GateNote { synth, step, note } => {
                let note = note.clamp(-24, 24);
                if step < 16 {
                    if synth == 0 {
                        self.rack.gm_gate_notes[step] = note;
                    } else {
                        self.rack.arp_gate_notes[step] = note;
                    }
                }
            }
            Cmd::GateNoteStep { synth, step, dir } => {
                if step < 16 {
                    if synth == 0 {
                        let cur = self.rack.gm_gate_notes[step];
                        self.rack.gm_gate_notes[step] = (cur + dir).clamp(-24, 24);
                    } else {
                        let cur = self.rack.arp_gate_notes[step];
                        self.rack.arp_gate_notes[step] = (cur + dir).clamp(-24, 24);
                    }
                }
            }
            Cmd::SetFilterIn { synth, value } => {
                if synth == 0 {
                    self.rack.gm_filter_in = value;
                } else {
                    self.rack.arp_filter_in = value;
                }
            }
            Cmd::SetGateIn { synth, value } => {
                if synth == 0 {
                    self.rack.gm_gate_in = value;
                } else {
                    self.rack.arp_gate_in = value;
                }
            }
            Cmd::SetCvIn { synth, value } => {
                if synth == 0 {
                    self.rack.gm_cv_in = value;
                } else {
                    self.rack.arp_cv_in = value;
                }
            }
            Cmd::LoadSample { data, name, path } => {
                self.rack.samp.load_audio(data, name, path);
            }
            Cmd::LoadProject(rack) => self.rack = rack,
            Cmd::SaveRequest(reply) => {
                let _ = reply.send(project::serialize(&self.rack));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::KnobId;
    use std::sync::mpsc::channel;

    fn engine() -> (AudioEngine, Sender<Cmd>, Receiver<View>) {
        let (tx, rx) = channel();
        let (view_tx, view_rx) = channel();
        (AudioEngine::new(Rack::new(), rx, view_tx), tx, view_rx)
    }

    fn last_view(rx: &Receiver<View>) -> View {
        let mut last = None;
        while let Ok(v) = rx.try_recv() {
            last = Some(v);
        }
        last.expect("engine must publish a view per block")
    }

    #[test]
    fn commands_apply_and_view_publishes() {
        let (mut e, tx, view_rx) = engine();
        tx.send(Cmd::Transport).unwrap();
        tx.send(Cmd::BpmStep(5.0)).unwrap();
        tx.send(Cmd::NoteOn { dev: 0, midi: 48 }).unwrap();
        let mut blk = vec![0.0f32; 256];
        e.render_block(&mut blk);
        assert!(blk.iter().any(|v| *v != 0.0), "note + drums must sound");
        assert!(blk.iter().all(|v| v.is_finite()));
        let v = last_view(&view_rx);
        assert!(v.sp.playing, "transport command must apply");
        assert!((v.sp.bpm - 125.0).abs() < 1e-9, "bpm step must apply: {}", v.sp.bpm);
    }

    #[test]
    fn every_knob_round_trips_through_view() {
        use KnobId::*;
        let ids = vec![
            SpCutoff, SpRes, SpLfoRate, SpLfoCut, SpLfoRes, SpLevel, GmOsc1, GmOsc2, GmCutoff,
            GmReso, GmAttack, GmDecay, GmSustain, GmRelease, GmGlide, GmLfoRate, ArpOsc1,
            ArpOsc2, ArpOsc3, ArpCutoff, ArpReso, ArpAttack, ArpDecay, ArpSustain, ArpRelease,
            ArpGlide, MixLevel(0), MixLevel(1), MixLevel(2), MixLevel(3), MixMaster, SpTune,
            GmNoise, GmEnvAmt, GmKbTrack, GmFAttack, GmFDecay, GmFSustain, GmFRelease, GmReverb,
            ArpNoise, ArpRing, ArpShRate, ArpLag, ArpVcaGain, ArpArAttack, ArpArRelease, ArpReverb,
            SampLevel, SampTune, SampStart,
        ];
        let (mut e, tx, view_rx) = engine();
        for id in ids {
            tx.send(Cmd::SetKnobT(id, 0.37)).unwrap();
            let mut blk = vec![0.0f32; 64];
            e.render_block(&mut blk);
            let v = last_view(&view_rx);
            let back = id.t(&v);
            assert!(
                (back - 0.37).abs() < 1e-3,
                "knob {id:?} must round-trip through set + view: got {back}"
            );
        }
        // Spot-check absolute values behind the normalized positions
        // (the cases the old panel tests pinned).
        let (mut e, tx, view_rx) = engine();
        tx.send(Cmd::SetKnobT(GmCutoff, 0.5)).unwrap();
        tx.send(Cmd::SetKnobT(SpTune, 0.5)).unwrap();
        tx.send(Cmd::SetKnobT(SampTune, 0.75)).unwrap();
        tx.send(Cmd::SetKnobT(ArpShRate, 0.5)).unwrap();
        let mut blk = vec![0.0f32; 64];
        e.render_block(&mut blk);
        let v = last_view(&view_rx);
        assert!((v.gm.params.cutoff - 632.0).abs() < 2.0, "log knob midpoint: {}", v.gm.params.cutoff);
        assert_eq!(v.sp.pads[0].tune_st, 0.0);
        assert_eq!(v.samp.tune_st, 12.0);
        assert!((v.arp.params.sh_rate - 2.236).abs() < 0.01, "sh rate: {}", v.arp.params.sh_rate);
    }

    #[test]
    fn step_commands_accumulate_without_a_fresh_view() {
        // Two wheel ticks in one frame must both land, even
        // though the UI reads the same stale snapshot for both.
        let (mut e, tx, view_rx) = engine();
        tx.send(Cmd::GateNoteStep { synth: 0, step: 3, dir: 1 }).unwrap();
        tx.send(Cmd::GateNoteStep { synth: 0, step: 3, dir: 1 }).unwrap();
        tx.send(Cmd::KnobStep(KnobId::GmCutoff, 0.02)).unwrap();
        tx.send(Cmd::KnobStep(KnobId::GmCutoff, 0.02)).unwrap();
        let mut blk = vec![0.0f32; 64];
        e.render_block(&mut blk);
        let v = last_view(&view_rx);
        assert_eq!(v.gm_gate_notes[3], 2, "both gate steps must land");
        let base = KnobId::GmCutoff.t(&View::default());
        let after = KnobId::GmCutoff.t(&v);
        assert!(
            (after - base - 0.04).abs() < 1e-3,
            "both knob steps must land: {base} -> {after}"
        );
    }

    #[test]
    fn save_request_returns_serialized_project() {
        let (mut e, tx, view_rx) = engine();
        let (reply_tx, reply_rx) = channel();
        tx.send(Cmd::SaveRequest(reply_tx)).unwrap();
        let mut blk = vec![0.0f32; 256];
        e.render_block(&mut blk);
        let text = reply_rx.try_recv().expect("save reply must arrive");
        assert!(text.contains("sp_patterns"), "serialized project must carry patterns");
        let _ = last_view(&view_rx);
    }

    #[test]
    fn render_headroom_at_256_samples() {
        let (mut e, _tx, _view_rx) = engine();
        e.rack.sp.trigger(0);
        e.rack.gm.voice.note_on(48);
        e.rack.arp.note_on(48.0);
        e.rack.samp.note_on(60);
        let blocks = 172; // ~1 s of audio at 256 samples.
        let mut peak = 0.0f32;
        let t0 = std::time::Instant::now();
        for _ in 0..blocks {
            let mut blk = vec![0.0f32; 256];
            e.render_block(&mut blk);
            for v in blk.iter() {
                assert!(v.is_finite());
                peak = peak.max(v.abs());
            }
        }
        let el = t0.elapsed();
        let per_block = el.as_secs_f64() * 1000.0 / blocks as f64;
        println!("HEADROOM 256-sample blocks: {per_block:.3} ms/block, peak {peak:.3}");
        assert!(peak > 0.0, "sounding rack must not be silent");
        assert!(
            per_block < 3.0,
            "a 256-sample block (5.8 ms) must render in under 3 ms: {per_block:.3}"
        );
    }
}

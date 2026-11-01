// Vendored from grandmother (see README: vendored engines).
//! The monophonic voice: keyboard/arp -> glide -> VCOs -> mixer -> ladder
//! filter -> VCA -> spring-style reverb -> master. The patch bay is
//! evaluated per sample, so audio-rate patching (oscillator into filter
//! cutoff, noise into pitch) works, with a one-sample delay only on
//! feedback paths (VCF OUT into VCF IN, LFO into its own rate, EG into
//! pitch) to keep the graph computable. The mult bus resolves on
//! previous-sample values for the same reason, so any mult path —
//! including feedback through the mult — is stable.
//!
//! New in the expanded bay: mixer channel inputs (switched), a patchable
//! 1-pole high-pass filter and a bipolar attenuator (utilities), an
//! S/H output sampling noise at each LFO cycle start, keyboard velocity,
//! oscillator wave outputs, mixer output, inverted envelope, linear FM
//! into OSC2, LFO sync, envelope trigger (which replaces the keyboard
//! gate, as on the hardware), a switched VCA audio input, and a switched
//! reverb audio input.

use crate::gm::arp::{Arp, ArpMode};
use crate::gm::env::Adsr;
use crate::gm::filter::{Ladder, OnePoleHp};
use crate::gm::lfo::{Lfo, LfoWave};
use crate::gm::osc::{cv_to_freq, midi_to_cv, Noise, Osc, Wave};
use crate::gm::patch::{inp, out, Dst, InKind, PatchBay, Src, IN_KINDS};
use crate::gm::reverb::Reverb;
use crate::gm::seq::Seq;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModDest {
    Pitch,
    Filter,
    Pw,
}

impl ModDest {
    pub fn next(self) -> ModDest {
        match self {
            ModDest::Pitch => ModDest::Filter,
            ModDest::Filter => ModDest::Pw,
            ModDest::Pw => ModDest::Pitch,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ModDest::Pitch => "PITCH",
            ModDest::Filter => "FILTER",
            ModDest::Pw => "PULSE W",
        }
    }
}

/// Knob parameters (buttons like wave select are set directly on Params).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamId {
    Osc1Fine,
    Osc1Pw,
    Osc1Level,
    Osc2Fine,
    Osc2Pw,
    Osc2Level,
    NoiseLevel,
    Glide,
    Cutoff,
    Reso,
    EnvAmt,
    KbTrack,
    FAttack,
    FDecay,
    FSustain,
    FRelease,
    AAttack,
    ADecay,
    ASustain,
    ARelease,
    LfoRate,
    ModAmt,
    ReverbMix,
    Master,
    ArpRate,
    Att,
    HpCutoff,
    SeqRate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub osc1_wave: Wave,
    pub osc1_oct: i32, // semitones: -24, -12, 0, 12
    pub osc1_fine: f32, // semitones
    pub osc1_pw: f32,
    pub osc1_level: f32,
    pub osc2_wave: Wave,
    pub osc2_oct: i32,
    pub osc2_fine: f32,
    pub osc2_pw: f32,
    pub osc2_level: f32,
    pub noise_level: f32,
    pub glide: f32, // seconds
    pub cutoff: f32, // Hz
    pub reso: f32,
    pub env_amt: f32, // -1..1
    pub kb_track: f32,
    pub f_attack: f32,
    pub f_decay: f32,
    pub f_sustain: f32,
    pub f_release: f32,
    pub a_attack: f32,
    pub a_decay: f32,
    pub a_sustain: f32,
    pub a_release: f32,
    pub lfo_rate: f32, // Hz
    pub lfo_wave: LfoWave,
    pub mod_amt: f32,
    pub mod_dest: ModDest,
    pub reverb_mix: f32,
    pub master: f32,
    pub arp_enabled: bool,
    pub arp_mode: ArpMode,
    pub arp_rate: f32, // steps per second
    pub arp_oct: usize,
    pub att: f32,      // bipolar attenuator: -1..1, 0 = silence
    pub hp_cutoff: f32, // Hz, utility high-pass filter
    pub seq_rate: f32, // steps per second
}

impl Default for Params {
    fn default() -> Self {
        Params {
            osc1_wave: Wave::Saw,
            osc1_oct: 0,
            osc1_fine: 0.0,
            osc1_pw: 0.5,
            osc1_level: 0.8,
            osc2_wave: Wave::Saw,
            osc2_oct: 0,
            osc2_fine: 0.0,
            osc2_pw: 0.5,
            osc2_level: 0.6,
            noise_level: 0.0,
            glide: 0.0,
            cutoff: 2000.0,
            reso: 0.2,
            env_amt: 0.5,
            kb_track: 0.5,
            f_attack: 0.01,
            f_decay: 0.3,
            f_sustain: 0.5,
            f_release: 0.2,
            a_attack: 0.01,
            a_decay: 0.2,
            a_sustain: 0.8,
            a_release: 0.2,
            lfo_rate: 2.0,
            lfo_wave: LfoWave::Triangle,
            mod_amt: 0.0,
            mod_dest: ModDest::Filter,
            reverb_mix: 0.15,
            master: 0.8,
            arp_enabled: false,
            arp_mode: ArpMode::Up,
            arp_rate: 8.0,
            arp_oct: 1,
            att: 0.0,
            hp_cutoff: 500.0,
            seq_rate: 8.0,
        }
    }
}

impl Params {
    pub fn get(&self, id: ParamId) -> f32 {
        match id {
            ParamId::Osc1Fine => self.osc1_fine,
            ParamId::Osc1Pw => self.osc1_pw,
            ParamId::Osc1Level => self.osc1_level,
            ParamId::Osc2Fine => self.osc2_fine,
            ParamId::Osc2Pw => self.osc2_pw,
            ParamId::Osc2Level => self.osc2_level,
            ParamId::NoiseLevel => self.noise_level,
            ParamId::Glide => self.glide,
            ParamId::Cutoff => self.cutoff,
            ParamId::Reso => self.reso,
            ParamId::EnvAmt => self.env_amt,
            ParamId::KbTrack => self.kb_track,
            ParamId::FAttack => self.f_attack,
            ParamId::FDecay => self.f_decay,
            ParamId::FSustain => self.f_sustain,
            ParamId::FRelease => self.f_release,
            ParamId::AAttack => self.a_attack,
            ParamId::ADecay => self.a_decay,
            ParamId::ASustain => self.a_sustain,
            ParamId::ARelease => self.a_release,
            ParamId::LfoRate => self.lfo_rate,
            ParamId::ModAmt => self.mod_amt,
            ParamId::ReverbMix => self.reverb_mix,
            ParamId::Master => self.master,
            ParamId::ArpRate => self.arp_rate,
            ParamId::Att => self.att,
            ParamId::HpCutoff => self.hp_cutoff,
            ParamId::SeqRate => self.seq_rate,
        }
    }

    pub fn set(&mut self, id: ParamId, v: f32) {
        match id {
            ParamId::Osc1Fine => self.osc1_fine = v,
            ParamId::Osc1Pw => self.osc1_pw = v,
            ParamId::Osc1Level => self.osc1_level = v,
            ParamId::Osc2Fine => self.osc2_fine = v,
            ParamId::Osc2Pw => self.osc2_pw = v,
            ParamId::Osc2Level => self.osc2_level = v,
            ParamId::NoiseLevel => self.noise_level = v,
            ParamId::Glide => self.glide = v,
            ParamId::Cutoff => self.cutoff = v,
            ParamId::Reso => self.reso = v,
            ParamId::EnvAmt => self.env_amt = v,
            ParamId::KbTrack => self.kb_track = v,
            ParamId::FAttack => self.f_attack = v,
            ParamId::FDecay => self.f_decay = v,
            ParamId::FSustain => self.f_sustain = v,
            ParamId::FRelease => self.f_release = v,
            ParamId::AAttack => self.a_attack = v,
            ParamId::ADecay => self.a_decay = v,
            ParamId::ASustain => self.a_sustain = v,
            ParamId::ARelease => self.a_release = v,
            ParamId::LfoRate => self.lfo_rate = v,
            ParamId::ModAmt => self.mod_amt = v,
            ParamId::ReverbMix => self.reverb_mix = v,
            ParamId::Master => self.master = v,
            ParamId::ArpRate => self.arp_rate = v,
            ParamId::Att => self.att = v,
            ParamId::HpCutoff => self.hp_cutoff = v,
            ParamId::SeqRate => self.seq_rate = v,
        }
    }
}

/// Current-sample signals, threaded through one sample's evaluation.
/// Fields for signals not yet computed hold previous-sample values.
struct Cur {
    kb_cv: f32,
    kb_gate: f32,
    kb_vel: f32,
    lfo: f32,
    sh: f32,
    feg: f32,
    aeg: f32,
    neg_env: f32,
    o1saw: f32,
    o1pulse: f32,
    o1wave: f32,
    o2saw: f32,
    o2tri: f32,
    o2wave: f32,
    noise: f32,
    mixer: f32,
    hp: f32,
    att: f32,
    mult: f32,
}

pub struct Voice {
    osc1: Osc,
    osc2: Osc,
    noise: Noise,
    filter: Ladder,
    hp: OnePoleHp,
    feg: Adsr,
    aeg: Adsr,
    lfo: Lfo,
    reverb: Reverb,
    arp: Arp,
    pub seq: Seq,
    seq_note: Option<u8>,
    cur_vel: f32,
    bend_semi: f32,
    mod_wheel: f32,
    held: Vec<u8>, // press order
    arp_note: Option<u8>,
    arp_samples: u32,
    arp_retrigger: bool,
    glide_cv1: f32,
    glide_cv2: f32,
    glide_snap: bool,
    last_cv: f32,
    sh_val: f32,
    prev_sync: f32,
    prev_gate: bool,
    prev_note: Option<u8>,
    prev_gate_f32: f32,
    prev_lfo: f32,
    prev_feg: f32,
    prev_aeg: f32,
    prev_neg_env: f32,
    prev_kbvel: f32,
    prev_sh: f32,
    prev_o1saw: f32,
    prev_o1pulse: f32,
    prev_o1wave: f32,
    prev_o2saw: f32,
    prev_o2tri: f32,
    prev_o2wave: f32,
    prev_noise: f32,
    prev_mixer: f32,
    prev_hp: f32,
    prev_att: f32,
    prev_vcf: f32,
    sr: f32,
}

impl Voice {
    pub fn new(sr: f32) -> Self {
        Voice {
            osc1: Osc::new(),
            osc2: Osc::new(),
            noise: Noise::new(),
            filter: Ladder::new(sr),
            hp: OnePoleHp::new(),
            feg: Adsr::new(sr),
            aeg: Adsr::new(sr),
            lfo: Lfo::new(),
            reverb: Reverb::new(sr),
            arp: Arp::new(),
            seq: Seq::new(),
            seq_note: None,
            cur_vel: 1.0,
            bend_semi: 0.0,
            mod_wheel: 1.0,
            held: Vec::new(),
            arp_note: None,
            arp_samples: 0,
            arp_retrigger: false,
            glide_cv1: 0.0,
            glide_cv2: 0.0,
            glide_snap: false,
            last_cv: 2.0,
            sh_val: 0.0,
            prev_sync: 0.0,
            prev_gate: false,
            prev_note: None,
            prev_gate_f32: 0.0,
            prev_lfo: 0.0,
            prev_feg: 0.0,
            prev_aeg: 0.0,
            prev_neg_env: 0.0,
            prev_kbvel: 0.0,
            prev_sh: 0.0,
            prev_o1saw: 0.0,
            prev_o1pulse: 0.0,
            prev_o1wave: 0.0,
            prev_o2saw: 0.0,
            prev_o2tri: 0.0,
            prev_o2wave: 0.0,
            prev_noise: 0.0,
            prev_mixer: 0.0,
            prev_hp: 0.0,
            prev_att: 0.0,
            prev_vcf: 0.0,
            sr,
        }
    }

    pub fn note_on(&mut self, midi: u8) {
        self.note_on_vel(midi, 1.0);
    }

    /// Note on with velocity (MIDI, sequencer accents). While REC is
    /// armed and the sequencer is stopped, the note is also written
    /// into the selected step.
    pub fn note_on_vel(&mut self, midi: u8, vel: f32) {
        self.seq.record_note(midi);
        if !self.held.contains(&midi) {
            self.held.push(midi);
        }
        self.cur_vel = vel.clamp(0.0, 1.0);
        self.arp.set_held(&self.held);
    }

    /// Pitch bend in semitones (MIDI pitch wheel, ±2 by default).
    pub fn set_bend(&mut self, semi: f32) {
        self.bend_semi = semi.clamp(-12.0, 12.0);
    }

    /// Mod wheel 0..1: scales all LFO modulation depths.
    pub fn set_mod_wheel(&mut self, v: f32) {
        self.mod_wheel = v.clamp(0.0, 1.0);
    }

    /// Start sequencer playback from step 0.
    pub fn seq_play(&mut self) {
        let (note, vel) = self.seq.play();
        self.seq_note = note;
        self.cur_vel = vel;
    }

    pub fn seq_stop(&mut self) {
        self.seq.stop();
        self.seq_note = None;
    }

    pub fn note_off(&mut self, midi: u8) {
        self.held.retain(|&n| n != midi);
        self.arp.set_held(&self.held);
    }

    pub fn held_notes(&self) -> &[u8] {
        &self.held
    }

    pub fn current_note(&self) -> Option<u8> {
        // While the sequencer runs it owns the voice outright — a rest
        // step silences it rather than falling through to held keys.
        if self.seq.playing {
            self.seq_note
        } else {
            self.arp_note.or_else(|| self.held.last().copied())
        }
    }

    /// Previous-sample value of a real output jack (feedback-safe).
    fn prev_out(&self, o: usize) -> f32 {
        match o {
            out::KB_CV => self.last_cv,
            out::GATE => self.prev_gate_f32,
            out::LFO => self.prev_lfo,
            out::OSC1_SAW => self.prev_o1saw,
            out::OSC1_PULSE => self.prev_o1pulse,
            out::OSC2_SAW => self.prev_o2saw,
            out::OSC2_TRI => self.prev_o2tri,
            out::NOISE => self.prev_noise,
            out::FILT_EG => self.prev_feg,
            out::AMP_EG => self.prev_aeg,
            out::VCF_OUT => self.prev_vcf,
            out::KB_VEL => self.prev_kbvel,
            out::SH_OUT => self.prev_sh,
            out::OSC1_WAVE => self.prev_o1wave,
            out::OSC2_WAVE => self.prev_o2wave,
            out::MIXER_OUT => self.prev_mixer,
            out::HP_OUT => self.prev_hp,
            out::ATT_OUT => self.prev_att,
            out::NEG_ENV => self.prev_neg_env,
            _ => 0.0,
        }
    }

    /// Current-sample value where already computed, previous otherwise.
    /// VCF OUT always reads previous: the filter runs late in the sample.
    fn cur_out(&self, o: usize, cur: &Cur) -> f32 {
        match o {
            out::KB_CV => cur.kb_cv,
            out::GATE => cur.kb_gate,
            out::LFO => cur.lfo,
            out::OSC1_SAW => cur.o1saw,
            out::OSC1_PULSE => cur.o1pulse,
            out::OSC2_SAW => cur.o2saw,
            out::OSC2_TRI => cur.o2tri,
            out::NOISE => cur.noise,
            out::FILT_EG => cur.feg,
            out::AMP_EG => cur.aeg,
            out::VCF_OUT => self.prev_vcf,
            out::KB_VEL => cur.kb_vel,
            out::SH_OUT => cur.sh,
            out::OSC1_WAVE => cur.o1wave,
            out::OSC2_WAVE => cur.o2wave,
            out::MIXER_OUT => cur.mixer,
            out::HP_OUT => cur.hp,
            out::ATT_OUT => cur.att,
            out::NEG_ENV => cur.neg_env,
            _ => 0.0,
        }
    }

    fn resolve_prev(&self, src: Src, bus: f32) -> f32 {
        match src {
            Src::Out(o) => self.prev_out(o),
            Src::Mult(_) => bus,
        }
    }

    fn resolve_cur(&self, src: Src, cur: &Cur) -> f32 {
        match src {
            Src::Out(o) => self.cur_out(o, cur),
            Src::Mult(_) => cur.mult,
        }
    }

    /// Sum of all cables into `dst`, resolving each source with `f`.
    fn sum_into<F: Fn(Src) -> f32>(&self, patch: &PatchBay, dst: Dst, f: F) -> f32 {
        let srcs = match dst {
            Dst::In(i) => patch.sources_into(i),
            Dst::Mult(m) => patch
                .cables
                .iter()
                .filter(|c| c.dst == Dst::Mult(m))
                .map(|c| c.src)
                .collect(),
        };
        srcs.iter().map(|&s| f(s)).sum()
    }

    pub fn render(&mut self, params: &Params, patch: &PatchBay, out: &mut [f32]) {
        self.feg.set(
            params.f_attack,
            params.f_decay,
            params.f_sustain,
            params.f_release,
        );
        self.aeg.set(
            params.a_attack,
            params.a_decay,
            params.a_sustain,
            params.a_release,
        );
        self.lfo.wave = params.lfo_wave;
        self.filter.reso = params.reso.clamp(0.0, 1.0);
        self.arp.mode = params.arp_mode;
        self.arp.octaves = params.arp_oct.clamp(1, 3);
        for s in out.iter_mut() {
            // Arpeggiator clock.
            if params.arp_enabled && !self.held.is_empty() {
                self.arp_samples += 1;
                let interval = (self.sr / params.arp_rate.max(0.1)) as u32;
                if self.arp_samples >= interval.max(1) {
                    self.arp_samples = 0;
                    self.arp_note = self.arp.step();
                    self.arp_retrigger = true;
                }
            } else {
                self.arp_note = None;
                self.arp_samples = 0;
            }
            // Sequencer clock.
            if let Some((note, vel)) = self.seq.tick(params.seq_rate, self.sr) {
                self.seq_note = note;
                self.cur_vel = vel;
            }
            *s = self.sample(params, patch);
        }
    }

    fn sample(&mut self, params: &Params, patch: &PatchBay) -> f32 {
        // (a) Note and keyboard CV/gate/velocity. KB VEL is the velocity
        // of the sounding note (keys 1.0, MIDI as played, sequencer
        // accents 1.0 / plain steps 0.7), else 0.
        let cur_note = self.current_note();
        let kb_gate = cur_note.is_some();
        let kb_vel = if kb_gate { self.cur_vel } else { 0.0 };
        let kb_cv = match cur_note {
            Some(n) => {
                let cv = midi_to_cv(n);
                self.last_cv = cv;
                cv
            }
            None => self.last_cv,
        };

        // Noise is generated early so the S/H output can sample it at
        // the LFO cycle start below.
        let noise_val = self.noise.next();

        // Mult bus: previous-sample values of every output patched into
        // any mult jack, summed. One shared bus for all four jacks.
        let mult_bus: f32 = patch
            .mult_sources()
            .iter()
            .map(|&o| self.prev_out(o))
            .sum();

        // (b) LFO rate (patched CV, previous-sample values so LFO-into-
        // its-own-rate is stable), sync (rising edge resets the phase),
        // and the S/H sample at each cycle start.
        let rate_cv = self.sum_into(patch, Dst::In(inp::LFO_RATE), |s| {
            self.resolve_prev(s, mult_bus)
        });
        self.lfo.rate = (params.lfo_rate * 2.0f32.powf(rate_cv)).clamp(0.01, 200.0);
        let sync = self.sum_into(patch, Dst::In(inp::SYNC_IN), |s| {
            self.resolve_prev(s, mult_bus)
        });
        if sync >= 0.5 && self.prev_sync < 0.5 {
            self.lfo.reset();
        }
        self.prev_sync = sync;
        let lfo_val = self.lfo.next(self.sr);
        if self.lfo.wrapped() {
            self.sh_val = noise_val;
        }

        // Threaded current-sample signals; not-yet-computed fields hold
        // previous-sample values.
        let mut cur = Cur {
            kb_cv,
            kb_gate: if kb_gate { 1.0 } else { 0.0 },
            kb_vel,
            lfo: lfo_val,
            sh: self.sh_val,
            feg: self.prev_feg,
            aeg: self.prev_aeg,
            neg_env: self.prev_neg_env,
            o1saw: self.prev_o1saw,
            o1pulse: self.prev_o1pulse,
            o1wave: self.prev_o1wave,
            o2saw: self.prev_o2saw,
            o2tri: self.prev_o2tri,
            o2wave: self.prev_o2wave,
            noise: noise_val,
            mixer: self.prev_mixer,
            hp: self.prev_hp,
            att: self.prev_att,
            mult: mult_bus,
        };

        // (c) Effective gate, precedence: TRIG IN (patched trigger
        // replaces the keyboard gate, as on the hardware) > GATE IN
        // (variant override) > keyboard gate.
        let gate = if patch.patched(inp::TRIG_IN) {
            self.sum_into(patch, Dst::In(inp::TRIG_IN), |s| {
                self.resolve_cur(s, &cur)
            }) > 0.5
        } else if patch.patched(inp::GATE_IN) {
            self.sum_into(patch, Dst::In(inp::GATE_IN), |s| {
                self.resolve_cur(s, &cur)
            }) > 0.5
        } else {
            kb_gate
        };

        // (d) Envelopes: gate edges, note-change retrigger, arp retrigger.
        let retrigger = self.arp_retrigger
            || (gate && cur_note != self.prev_note && self.prev_gate);
        if gate && !self.prev_gate {
            self.feg.gate_on();
            self.aeg.gate_on();
            self.glide_snap = true;
        }
        if !gate && self.prev_gate {
            self.feg.gate_off();
            self.aeg.gate_off();
        }
        if retrigger {
            self.feg.gate_on();
            self.aeg.gate_on();
        }
        self.arp_retrigger = false;
        let feg_val = self.feg.next();
        let aeg_val = self.aeg.next();
        cur.feg = feg_val;
        cur.aeg = aeg_val;
        cur.neg_env = -aeg_val;

        // (e) Oscillator pitch CVs (patched CV replaces the KB normal
        // for switched inputs), glide, modulation, and linear FM.
        let pitch_cv = |this: &Voice, osc: usize| -> f32 {
            if patch.patched(osc) {
                this.sum_into(patch, Dst::In(osc), |s| this.resolve_cur(s, &cur))
            } else {
                kb_cv
            }
        };
        let target1 = pitch_cv(self, inp::OSC1_PITCH);
        let target2 = pitch_cv(self, inp::OSC2_PITCH);
        if self.glide_snap {
            self.glide_cv1 = target1;
            self.glide_cv2 = target2;
            self.glide_snap = false;
        } else if params.glide > 0.0 {
            let k = 1.0 - (-1.0 / (params.glide * self.sr)).exp();
            self.glide_cv1 += (target1 - self.glide_cv1) * k;
            self.glide_cv2 += (target2 - self.glide_cv2) * k;
        } else {
            self.glide_cv1 = target1;
            self.glide_cv2 = target2;
        }
        let mod_pitch = if params.mod_dest == ModDest::Pitch {
            lfo_val * params.mod_amt * self.mod_wheel
        } else {
            0.0
        };
        let freq1 = cv_to_freq(
            self.glide_cv1
                + (params.osc1_oct as f32 + params.osc1_fine) / 12.0
                + mod_pitch
                + self.bend_semi / 12.0,
        );
        let lin_fm = if IN_KINDS[inp::LIN_FM_IN] == InKind::Additive {
            self.sum_into(patch, Dst::In(inp::LIN_FM_IN), |s| {
                self.resolve_cur(s, &cur)
            })
        } else {
            0.0
        };
        let freq2 = (cv_to_freq(
            self.glide_cv2
                + (params.osc2_oct as f32 + params.osc2_fine) / 12.0
                + mod_pitch
                + self.bend_semi / 12.0,
        ) + lin_fm * 100.0)
            .max(1.0);

        // (f) Pulse widths, oscillator advance, wave outputs.
        let pw_cv = |this: &Voice, inp_idx: usize| -> f32 {
            if patch.patched(inp_idx) {
                this.sum_into(patch, Dst::In(inp_idx), |s| this.resolve_cur(s, &cur))
            } else {
                0.0
            }
        };
        let mod_pw = if params.mod_dest == ModDest::Pw {
            lfo_val * params.mod_amt * 0.4 * self.mod_wheel
        } else {
            0.0
        };
        let pw1 = (params.osc1_pw + pw_cv(self, inp::OSC1_PW) * 0.5 + mod_pw).clamp(0.05, 0.95);
        let pw2 = (params.osc2_pw + pw_cv(self, inp::OSC2_PW) * 0.5 + mod_pw).clamp(0.05, 0.95);
        self.osc1.advance(freq1, self.sr);
        self.osc2.advance(freq2, self.sr);
        let o1saw = crate::gm::osc::saw(self.osc1.phase);
        let o1pulse = crate::gm::osc::pulse(self.osc1.phase, pw1);
        let o2saw = crate::gm::osc::saw(self.osc2.phase);
        let o2tri = crate::gm::osc::triangle(self.osc2.phase);
        let o1sel = self.osc1.output(params.osc1_wave, pw1);
        let o2sel = self.osc2.output(params.osc2_wave, pw2);
        cur.o1saw = o1saw;
        cur.o1pulse = o1pulse;
        cur.o1wave = o1sel;
        cur.o2saw = o2saw;
        cur.o2tri = o2tri;
        cur.o2wave = o2sel;

        // (g) Mixer: each channel's input is switched — a patched cable
        // replaces the normalled source, the level knob still scales it.
        let ch1 = if patch.patched(inp::MIX_OSC1) {
            self.sum_into(patch, Dst::In(inp::MIX_OSC1), |s| {
                self.resolve_cur(s, &cur)
            })
        } else {
            o1sel
        };
        let ch2 = if patch.patched(inp::MIX_OSC2) {
            self.sum_into(patch, Dst::In(inp::MIX_OSC2), |s| {
                self.resolve_cur(s, &cur)
            })
        } else {
            o2sel
        };
        let ch3 = if patch.patched(inp::MIX_NOISE) {
            self.sum_into(patch, Dst::In(inp::MIX_NOISE), |s| {
                self.resolve_cur(s, &cur)
            })
        } else {
            noise_val
        };
        let mixer_out = ch1 * params.osc1_level + ch2 * params.osc2_level + ch3 * params.noise_level;
        cur.mixer = mixer_out;

        // (g2) Utilities: high-pass filter and bipolar attenuator. Both
        // are standalone — no normals — so an unpatched input yields 0.
        let hp_in = self.sum_into(patch, Dst::In(inp::HP_IN), |s| {
            self.resolve_cur(s, &cur)
        });
        let hp_out = self.hp.process(hp_in, params.hp_cutoff, self.sr);
        let att_in = self.sum_into(patch, Dst::In(inp::ATT_IN), |s| {
            self.resolve_cur(s, &cur)
        });
        let att_out = att_in * params.att;
        cur.hp = hp_out;
        cur.att = att_out;

        // (h) VCF input (patched audio replaces the mixer normal).
        let vcf_in = if patch.patched(inp::VCF_IN) {
            self.sum_into(patch, Dst::In(inp::VCF_IN), |s| {
                self.resolve_cur(s, &cur)
            })
        } else {
            mixer_out
        };

        // Cutoff: base x KB track x filter EG x additive CV x mod.
        let mod_cut = if params.mod_dest == ModDest::Filter {
            lfo_val * params.mod_amt * 2.0 * self.mod_wheel
        } else {
            0.0
        };
        let add_cut = if IN_KINDS[inp::VCF_CUTOFF] == InKind::Additive && patch.patched(inp::VCF_CUTOFF) {
            self.sum_into(patch, Dst::In(inp::VCF_CUTOFF), |s| {
                self.resolve_cur(s, &cur)
            })
        } else {
            0.0
        };
        let env_amt_eff = if IN_KINDS[inp::ENV_AMT] == InKind::Additive && patch.patched(inp::ENV_AMT) {
            (params.env_amt
                + self.sum_into(patch, Dst::In(inp::ENV_AMT), |s| {
                    self.resolve_cur(s, &cur)
                }))
            .clamp(-1.0, 1.0)
        } else {
            params.env_amt
        };
        let cutoff = (params.cutoff
            * 2.0f32.powf(params.kb_track * kb_cv)
            * 2.0f32.powf(feg_val * env_amt_eff * 2.0)
            * 2.0f32.powf(add_cut)
            * 2.0f32.powf(mod_cut))
        .clamp(20.0, 18000.0);
        self.filter.cutoff = cutoff;
        let vcf_out = self.filter.process(vcf_in);

        // (i) VCA: switched audio input (filter normal), amp EG plus
        // additive CV, clamped.
        let vca_input = if patch.patched(inp::VCA_IN) {
            self.sum_into(patch, Dst::In(inp::VCA_IN), |s| {
                self.resolve_cur(s, &cur)
            })
        } else {
            vcf_out
        };
        let add_vca = if IN_KINDS[inp::VCA_CV] == InKind::Additive && patch.patched(inp::VCA_CV) {
            self.sum_into(patch, Dst::In(inp::VCA_CV), |s| {
                self.resolve_cur(s, &cur)
            })
        } else {
            0.0
        };
        let vca_level = (aeg_val + add_vca).clamp(0.0, 1.0);
        let vca = vca_input * vca_level;

        // (j) Reverb: switched audio input (VCA normal) + master.
        let reverb_input = if patch.patched(inp::REVERB_IN) {
            self.sum_into(patch, Dst::In(inp::REVERB_IN), |s| {
                self.resolve_cur(s, &cur)
            })
        } else {
            vca
        };
        let wet = self.reverb.process(reverb_input);
        let mixed = vca * (1.0 - params.reverb_mix) + wet * params.reverb_mix;
        let out_sample = (mixed * params.master * 1.5).tanh() / 0.9051;

        // (k) Store previous-sample signals and gate/note state.
        self.prev_lfo = lfo_val;
        self.prev_feg = feg_val;
        self.prev_aeg = aeg_val;
        self.prev_neg_env = -aeg_val;
        self.prev_kbvel = kb_vel;
        self.prev_sh = self.sh_val;
        self.prev_o1saw = o1saw;
        self.prev_o1pulse = o1pulse;
        self.prev_o1wave = o1sel;
        self.prev_o2saw = o2saw;
        self.prev_o2tri = o2tri;
        self.prev_o2wave = o2sel;
        self.prev_noise = noise_val;
        self.prev_mixer = mixer_out;
        self.prev_hp = hp_out;
        self.prev_att = att_out;
        self.prev_vcf = vcf_out;
        self.prev_gate = gate;
        self.prev_gate_f32 = if gate { 1.0 } else { 0.0 };
        self.prev_note = cur_note;

        out_sample
    }
}

/// The whole instrument: parameters + patch bay + voice. The audio
/// callback and the UI share one of these behind a mutex.
pub struct Synth {
    pub params: Params,
    pub patch: PatchBay,
    pub voice: Voice,
}

impl Synth {
    pub fn new(sr: f32) -> Self {
        Synth {
            params: Params::default(),
            patch: PatchBay::new(),
            voice: Voice::new(sr),
        }
    }

    pub fn render(&mut self, out: &mut [f32]) {
        let params = self.params.clone();
        self.voice.render(&params, &self.patch, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gm::patch::{out as o, Cable, Dst, PatchBay, Src};

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn max_diff(a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max)
    }

    fn render_note(params: &Params, patch: &PatchBay, note: u8, secs: f32) -> Vec<f32> {
        let mut v = Voice::new(44100.0);
        v.note_on(note);
        let n = (44100.0 * secs) as usize;
        let mut buf = vec![0.0; n];
        v.render(params, patch, &mut buf);
        buf
    }

    #[test]
    fn silence_without_notes() {
        let p = Params::default();
        let patch = PatchBay::new();
        let buf = render_note(&p, &patch, 60, 0.0); // renders nothing
        assert!(buf.is_empty());
        let mut v = Voice::new(44100.0);
        let mut buf = vec![0.0; 22050];
        v.render(&p, &patch, &mut buf);
        assert!(buf.iter().all(|&x| x == 0.0), "no note -> silence");
    }

    #[test]
    fn note_sounds_and_releases() {
        let mut p = Params::default();
        p.reverb_mix = 0.0;
        let patch = PatchBay::new();
        let buf = render_note(&p, &patch, 60, 0.5);
        assert!(rms(&buf) > 0.05, "note should sound, rms {}", rms(&buf));
        assert!(buf.iter().all(|x| x.is_finite()));
        // Release: note off, then the tail must die.
        let mut v = Voice::new(44100.0);
        v.note_on(60);
        let mut on = vec![0.0; 13230];
        v.render(&p, &patch, &mut on);
        v.note_off(60);
        let mut off = vec![0.0; 44100];
        v.render(&p, &patch, &mut off);
        assert!(rms(&off[35280..]) < 0.01, "tail rms {}", rms(&off[35280..]));
    }

    #[test]
    fn patch_changes_sound() {
        let p = Params::default();
        let plain = render_note(&p, &PatchBay::new(), 60, 0.5);
        let mut patch = PatchBay::new();
        patch.cables.push(Cable {
            src: Src::Out(o::LFO),
            dst: Dst::In(inp::VCF_CUTOFF),
        });
        let mut p2 = Params::default();
        p2.lfo_rate = 5.0;
        p2.mod_amt = 0.0;
        let patched = render_note(&p2, &patch, 60, 0.5);
        assert!(
            max_diff(&plain, &patched) > 0.01,
            "LFO->cutoff should change the sound"
        );
    }

    #[test]
    fn lfo_into_gate_triggers_without_keys() {
        let mut p = Params::default();
        p.lfo_wave = LfoWave::Square;
        p.lfo_rate = 5.0;
        let mut patch = PatchBay::new();
        patch.cables.push(Cable {
            src: Src::Out(o::LFO),
            dst: Dst::In(inp::GATE_IN),
        });
        let mut v = Voice::new(44100.0);
        let mut buf = vec![0.0; 44100];
        v.render(&p, &patch, &mut buf);
        assert!(rms(&buf) > 0.01, "patched gate should trigger, rms {}", rms(&buf));
    }

    #[test]
    fn mult_routes_signal() {
        let plain = render_note(&Params::default(), &PatchBay::new(), 60, 0.5);
        let mut patch = PatchBay::new();
        patch.cables.push(Cable {
            src: Src::Out(o::LFO),
            dst: Dst::Mult(0),
        });
        patch.cables.push(Cable {
            src: Src::Mult(2),
            dst: Dst::In(inp::VCF_CUTOFF),
        });
        let mut p = Params::default();
        p.lfo_rate = 5.0;
        p.mod_amt = 0.0;
        let routed = render_note(&p, &patch, 60, 0.5);
        assert!(
            max_diff(&plain, &routed) > 0.01,
            "LFO->mult->cutoff should change the sound"
        );
    }

    #[test]
    fn trigger_in_drives_envelopes_without_keys() {
        let mut p = Params::default();
        p.lfo_wave = LfoWave::Square;
        p.lfo_rate = 5.0;
        let mut patch = PatchBay::new();
        patch.cables.push(Cable {
            src: Src::Out(o::LFO),
            dst: Dst::In(inp::TRIG_IN),
        });
        let mut v = Voice::new(44100.0);
        let mut buf = vec![0.0; 44100];
        v.render(&p, &patch, &mut buf);
        assert!(rms(&buf) > 0.01, "patched trigger should sound, rms {}", rms(&buf));
    }

    #[test]
    fn mixer_channel_in_replaces_normal() {
        let mut p = Params::default();
        p.reverb_mix = 0.0;
        p.osc1_level = 1.0;
        p.osc2_level = 0.0;
        p.noise_level = 0.0;
        let plain = render_note(&p, &PatchBay::new(), 60, 0.5);
        let mut patch = PatchBay::new();
        patch.cables.push(Cable {
            src: Src::Out(o::NOISE),
            dst: Dst::In(inp::MIX_OSC1),
        });
        let replaced = render_note(&p, &patch, 60, 0.5);
        assert!(
            max_diff(&plain, &replaced) > 0.05,
            "noise into MIX OSC1 should replace the saw"
        );
    }

    #[test]
    fn vca_in_replaces_filter_output() {
        let mut p = Params::default();
        p.reverb_mix = 0.0;
        let plain = render_note(&p, &PatchBay::new(), 60, 0.5);
        let mut patch = PatchBay::new();
        patch.cables.push(Cable {
            src: Src::Out(o::NOISE),
            dst: Dst::In(inp::VCA_IN),
        });
        let replaced = render_note(&p, &patch, 60, 0.5);
        assert!(
            max_diff(&plain, &replaced) > 0.05,
            "noise into VCA IN should replace the filtered saw"
        );
    }

    #[test]
    fn attenuator_scales_and_mutes() {
        let mut p = Params::default();
        p.lfo_rate = 5.0;
        p.mod_amt = 0.0;
        let plain = render_note(&p, &PatchBay::new(), 60, 0.5);
        let mut patch = PatchBay::new();
        patch.cables.push(Cable {
            src: Src::Out(o::LFO),
            dst: Dst::In(inp::ATT_IN),
        });
        patch.cables.push(Cable {
            src: Src::Out(o::ATT_OUT),
            dst: Dst::In(inp::VCF_CUTOFF),
        });
        // Attenuator at 0: the LFO never reaches the cutoff.
        let mut p0 = p.clone();
        p0.att = 0.0;
        let muted = render_note(&p0, &patch, 60, 0.5);
        assert!(
            max_diff(&plain, &muted) < 0.001,
            "att 0 should pass nothing, diff {}",
            max_diff(&plain, &muted)
        );
        // Attenuator open: the LFO wobbles the cutoff.
        let mut p1 = p.clone();
        p1.att = 1.0;
        let open = render_note(&p1, &patch, 60, 0.5);
        assert!(
            max_diff(&plain, &open) > 0.01,
            "att 1 should pass the LFO to the cutoff"
        );
    }

    #[test]
    fn sh_out_steps_the_cutoff() {
        let plain = render_note(&Params::default(), &PatchBay::new(), 60, 0.5);
        let mut patch = PatchBay::new();
        patch.cables.push(Cable {
            src: Src::Out(o::SH_OUT),
            dst: Dst::In(inp::VCF_CUTOFF),
        });
        let mut p = Params::default();
        p.lfo_rate = 5.0;
        p.mod_amt = 0.0;
        let stepped = render_note(&p, &patch, 60, 0.5);
        assert!(
            max_diff(&plain, &stepped) > 0.01,
            "S/H into cutoff should change the sound"
        );
    }

    #[test]
    fn sequencer_playback_drives_notes() {
        let p = Params::default();
        let patch = PatchBay::new();
        let mut v = Voice::new(44100.0);
        v.seq_play();
        let mut buf = vec![0.0; 22050];
        v.render(&p, &patch, &mut buf);
        assert!(rms(&buf) > 0.01, "playing sequencer should sound");
    }

    #[test]
    fn sequencer_rests_silence_and_override_held_keys() {
        let p = Params::default();
        let patch = PatchBay::new();
        let mut v = Voice::new(44100.0);
        v.seq.clear();
        v.note_on(60); // held key must NOT leak through a rest pattern
        v.seq_play();
        let mut buf = vec![0.0; 22050];
        v.render(&p, &patch, &mut buf);
        assert!(
            buf.iter().all(|&x| x == 0.0),
            "rest steps must silence the voice, even with held keys"
        );
    }

    #[test]
    fn bend_shifts_pitch() {
        let p = Params::default();
        let patch = PatchBay::new();
        let plain = render_note(&p, &patch, 60, 0.5);
        let mut v = Voice::new(44100.0);
        v.note_on(60);
        v.set_bend(2.0);
        let mut buf = vec![0.0; 22050];
        v.render(&p, &patch, &mut buf);
        assert!(
            max_diff(&plain, &buf) > 0.05,
            "a 2-semitone bend should change the sound"
        );
    }

    #[test]
    fn rec_entry_writes_steps_from_played_notes() {
        let mut v = Voice::new(44100.0);
        v.seq.toggle_rec();
        v.note_on(65);
        v.note_on(69);
        assert_eq!(v.seq.steps[0].note, 65);
        assert_eq!(v.seq.steps[1].note, 69);
        assert!(v.seq.steps[0].gate && v.seq.steps[1].gate);
        assert_eq!(v.seq.selected, 2);
    }
}

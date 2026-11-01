//! Rack state: four engines, one mixer, fixed routings, shared clock.
//!
//! Audio runs at 44.1 kHz mono. The SP-1200 engine renders at its native
//! 26.04 kHz and is linearly resampled into the device block (the same
//! pattern its own audio callback uses); the two synths and the sampler
//! render natively at device rate. The SP transport is the rack clock:
//! its step fires each synth's gate row, and each synth's filter can
//! take an external input (drums, the other synth, or the sampler)
//! through a dedicated ladder instance that mirrors the strip's
//! cutoff/resonance knobs.

use crate::arp;
use crate::gm;
use crate::samp::Sampler;
use crate::sp;

pub const SR: f32 = 44100.0;

/// Filter-in source for a synth strip. A strip never takes itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterIn {
    Off,
    Sp,
    Gm,
    Arp,
    Samp,
}

impl FilterIn {
    pub fn label(self) -> &'static str {
        match self {
            FilterIn::Off => "OFF",
            FilterIn::Sp => "DRUMS",
            FilterIn::Gm => "GRAND",
            FilterIn::Arp => "ARP",
            FilterIn::Samp => "SAMP",
        }
    }
}

/// Gate-pattern source for a synth strip. Off means the strip's own
/// gate row; anything else borrows another row's pattern while the
/// strip keeps its own per-step notes. A strip never takes itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateIn {
    Off,
    Sp,
    Gm,
    Arp,
}

impl GateIn {
    pub fn label(self) -> &'static str {
        match self {
            GateIn::Off => "OWN ROW",
            GateIn::Sp => "DRUMS",
            GateIn::Gm => "GRAND",
            GateIn::Arp => "ARP",
        }
    }
}

/// Cutoff-CV source for a synth strip. Only the grandmother's LFO
/// exists as a CV source in this rack; it modulates the strip's
/// cutoff by up to +-1 octave, updated once per render block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CvIn {
    Off,
    Lfo,
}

impl CvIn {
    pub fn label(self) -> &'static str {
        match self {
            CvIn::Off => "OPEN",
            CvIn::Lfo => "GM LFO",
        }
    }
}

/// The note a gate step plays, if its pattern row has a hit there:
/// the strip's base note plus the step's semitone offset.
fn gate_trigger(pattern: &[bool], notes: &[i8; 16], base: u8, st: usize) -> Option<u8> {
    if pattern[st] {
        Some((base as i32 + notes[st] as i32).clamp(0, 127) as u8)
    } else {
        None
    }
}

pub struct Rack {
    pub sp: sp::engine::Engine,
    pub gm: gm::voice::Synth,
    pub arp: arp::voice::Synth,
    pub samp: Sampler,
    /// Mixer levels: [SP, GM, ARP, SAMP].
    pub level: [f32; 4],
    pub mute: [bool; 4],
    pub master: f32,
    pub gm_filter_in: FilterIn,
    pub arp_filter_in: FilterIn,
    pub gm_gate_in: GateIn,
    pub arp_gate_in: GateIn,
    pub gm_cv_in: CvIn,
    pub arp_cv_in: CvIn,
    gm_ext: gm::filter::Ladder,
    arp_ext: arp::filter::Ladder,
    /// 16-step gate rows, fired by the SP clock (step % 16).
    pub gm_gate: [bool; 16],
    pub arp_gate: [bool; 16],
    /// Per-step semitone offsets for the gate rows (-24..+24, 0 = base).
    pub gm_gate_notes: [i8; 16],
    pub arp_gate_notes: [i8; 16],
    /// Notes currently held by the gate rows (for clean release).
    gate_held: [Option<u8>; 2],
    /// Keyboard octave offset per device; base note = 48 + octave * 12.
    pub gm_octave: i32,
    pub arp_octave: i32,
    /// Selected device for computer-key notes: 0 = grandmother,
    /// 1 = arp, 2 = sampler.
    pub selected: usize,
    /// Selected SP pad for the step row and filter/LFO knobs.
    pub sp_sel: usize,
    prev_step: u32,
    sp_buf: Vec<f32>,
    sp_pos: f64,
    /// Block peaks for the meters; the UI decays them per frame.
    pub peak: [f32; 4],
    pub master_peak: f32,
}

impl Rack {
    pub fn new() -> Self {
        let mut sp = sp::engine::Engine::new();
        sp.bpm = 120.0;
        sp.master = 1.0;
        let mut gm = gm::voice::Synth::new(SR);
        gm.params.master = 1.0;
        let arp = arp::voice::Synth::new(SR);
        Rack {
            sp,
            gm,
            arp,
            samp: Sampler::new(),
            level: [0.8, 0.8, 0.8, 0.8],
            mute: [false, false, false, false],
            master: 0.9,
            gm_filter_in: FilterIn::Off,
            arp_filter_in: FilterIn::Off,
            gm_gate_in: GateIn::Off,
            arp_gate_in: GateIn::Off,
            gm_cv_in: CvIn::Off,
            arp_cv_in: CvIn::Off,
            gm_ext: gm::filter::Ladder::new(SR),
            arp_ext: arp::filter::Ladder::new(),
            gm_gate: [false; 16],
            arp_gate: [false; 16],
            gm_gate_notes: [0; 16],
            arp_gate_notes: [0; 16],
            gate_held: [None, None],
            gm_octave: 0,
            arp_octave: 0,
            selected: 0,
            sp_sel: 0,
            prev_step: 0,
            sp_buf: Vec::with_capacity(8192),
            sp_pos: 0.0,
            peak: [0.0; 4],
            master_peak: 0.0,
        }
    }

    /// Base note for a device's keyboard and gate row (C3 + octave).
    pub fn base_note(&self, synth: usize) -> u8 {
        match synth {
            0 => (48 + self.gm_octave * 12).clamp(12, 96) as u8,
            1 => (48 + self.arp_octave * 12).clamp(12, 96) as u8,
            _ => self.samp.base_note(),
        }
    }

    /// Transport toggle: stopping also releases any sounding gate notes
    /// so nothing hangs when the clock parks.
    pub fn toggle_transport(&mut self) {
        self.sp.playing = !self.sp.playing;
        if !self.sp.playing {
            for synth in 0..2 {
                if let Some(n) = self.gate_held[synth].take() {
                    if synth == 0 {
                        self.gm.voice.note_off(n);
                    } else {
                        self.arp.note_off(n as f32);
                    }
                }
            }
        }
    }

    pub fn cycle_filter_in(&mut self, synth: usize) {
        if synth == 0 {
            self.gm_filter_in = match self.gm_filter_in {
                FilterIn::Off => FilterIn::Sp,
                FilterIn::Sp => FilterIn::Arp,
                FilterIn::Arp => FilterIn::Samp,
                _ => FilterIn::Off,
            };
        } else {
            self.arp_filter_in = match self.arp_filter_in {
                FilterIn::Off => FilterIn::Sp,
                FilterIn::Sp => FilterIn::Gm,
                FilterIn::Gm => FilterIn::Samp,
                _ => FilterIn::Off,
            };
        }
    }

    /// Render one device-rate block into `out` (mono, -1..1).
    pub fn render_block(&mut self, out: &mut [f32]) {
        let n = out.len();
        // --- SP bus: render at engine rate, resample linearly. ---
        let ratio = sp::dsp::SP_RATE / SR as f64;
        let need_end = (self.sp_pos + n as f64 * ratio).ceil() as usize + 2;
        if need_end > self.sp_buf.len() {
            let old = self.sp_buf.len();
            self.sp_buf.resize(need_end, 0.0);
            self.sp.render(&mut self.sp_buf[old..need_end]);
        }
        let mut sp_bus = vec![0.0f32; n];
        let mut pos = self.sp_pos;
        for s in sp_bus.iter_mut() {
            let i0 = pos.floor() as usize;
            let f = (pos - i0 as f64) as f32;
            *s = self.sp_buf[i0] + (self.sp_buf[i0 + 1] - self.sp_buf[i0]) * f;
            pos += ratio;
        }
        self.sp_pos = pos;
        let drop = (self.sp_pos as usize).saturating_sub(2);
        if drop > 0 {
            self.sp_buf.drain(..drop);
            self.sp_pos -= drop as f64;
        }

        // --- Clock gates: a newly fired SP step triggers gate rows.
        // A patched strip borrows another row's pattern but keeps its
        // own per-step notes; the held note is tracked so release
        // targets the note that actually sounded, not the base.
        if self.sp.playing && self.sp.last_step != self.prev_step {
            self.prev_step = self.sp.last_step;
            let st = (self.sp.last_step % 16) as usize;
            for synth in 0..2 {
                let note = {
                    let (pattern, notes): (&[bool], &[i8; 16]) = match synth {
                        0 => match self.gm_gate_in {
                            GateIn::Off => (&self.gm_gate, &self.gm_gate_notes),
                            GateIn::Sp => (&self.sp.patterns[self.sp_sel][..16], &self.gm_gate_notes),
                            GateIn::Gm => (&self.gm_gate, &self.gm_gate_notes),
                            GateIn::Arp => (&self.arp_gate, &self.gm_gate_notes),
                        },
                        _ => match self.arp_gate_in {
                            GateIn::Off => (&self.arp_gate, &self.arp_gate_notes),
                            GateIn::Sp => (&self.sp.patterns[self.sp_sel][..16], &self.arp_gate_notes),
                            GateIn::Gm => (&self.gm_gate, &self.arp_gate_notes),
                            GateIn::Arp => (&self.arp_gate, &self.arp_gate_notes),
                        },
                    };
                    gate_trigger(pattern, notes, self.base_note(synth), st)
                };
                match note {
                    Some(note) => {
                        if synth == 0 {
                            self.gm.voice.note_on(note);
                        } else {
                            self.arp.note_on(note as f32);
                        }
                        self.gate_held[synth] = Some(note);
                    }
                    None => {
                        if let Some(n) = self.gate_held[synth].take() {
                            if synth == 0 {
                                self.gm.voice.note_off(n);
                            } else {
                                self.arp.note_off(n as f32);
                            }
                        }
                    }
                }
            }
        }

        // --- CV: the grandmother's LFO bends a strip's cutoff by up
        // to +-1 octave. Applied per block, restored right after the
        // render, so knob reads always see the base value (the whole
        // block holds the Rack lock, so the UI never sees it bent).
        let lfo = self.gm.lfo_value();
        let gm_base_cut = self.gm.params.cutoff;
        let arp_base_cut = self.arp.params.vcf_cutoff;
        if self.gm_cv_in == CvIn::Lfo {
            self.gm.params.cutoff = (gm_base_cut * 2.0f32.powf(lfo)).clamp(20.0, 20000.0);
        }
        if self.arp_cv_in == CvIn::Lfo {
            self.arp.params.vcf_cutoff = (arp_base_cut * 2.0f32.powf(lfo)).clamp(20.0, 20000.0);
        }

        // --- Synth voices + sampler at device rate. ---
        let mut gm_buf = vec![0.0f32; n];
        let mut arp_buf = vec![0.0f32; n];
        let mut samp_buf = vec![0.0f32; n];
        self.gm.render(&mut gm_buf);
        self.arp.render(&mut arp_buf);
        self.samp.render(&mut samp_buf);
        self.gm.params.cutoff = gm_base_cut;
        self.arp.params.vcf_cutoff = arp_base_cut;

        // --- Filter-in: external audio through a dedicated ladder that
        // mirrors the strip's cutoff/resonance knobs. Sources are the
        // raw voice buses, so two strips feeding each other can't loop.
        let mut gm_ext_out = vec![0.0f32; n];
        match self.gm_filter_in {
            FilterIn::Sp | FilterIn::Arp | FilterIn::Samp => {
                self.gm_ext.cutoff = self.gm.params.cutoff;
                self.gm_ext.reso = self.gm.params.reso;
                let src = match self.gm_filter_in {
                    FilterIn::Sp => &sp_bus,
                    FilterIn::Arp => &arp_buf,
                    _ => &samp_buf,
                };
                for (i, s) in src.iter().enumerate() {
                    gm_ext_out[i] = self.gm_ext.process(*s);
                }
            }
            _ => {}
        }
        let mut arp_ext_out = vec![0.0f32; n];
        match self.arp_filter_in {
            FilterIn::Sp | FilterIn::Gm | FilterIn::Samp => {
                self.arp_ext.cutoff = self.arp.params.vcf_cutoff;
                self.arp_ext.resonance = self.arp.params.vcf_reso;
                let src = match self.arp_filter_in {
                    FilterIn::Sp => &sp_bus,
                    FilterIn::Gm => &gm_buf,
                    _ => &samp_buf,
                };
                for (i, s) in src.iter().enumerate() {
                    arp_ext_out[i] = self.arp_ext.next(*s, SR);
                }
            }
            _ => {}
        }

        // --- Mixer. ---
        let mut pk = [0.0f32; 4];
        let mut mpk = 0.0f32;
        for i in 0..n {
            let ch = [
                sp_bus[i],
                gm_buf[i] + gm_ext_out[i],
                arp_buf[i] + arp_ext_out[i],
                samp_buf[i],
            ];
            let mut mix = 0.0f32;
            for c in 0..4 {
                let v = if self.mute[c] { 0.0 } else { ch[c] * self.level[c] };
                pk[c] = pk[c].max(v.abs());
                mix += v;
            }
            let v = (mix * self.master).clamp(-1.0, 1.0);
            mpk = mpk.max(v.abs());
            out[i] = v;
        }
        self.peak = pk;
        self.master_peak = mpk;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sounding_rack() -> Rack {
        let mut r = Rack::new();
        r.sp.trigger(0);
        r.gm.voice.note_on(48);
        r.arp.note_on(48.0);
        r
    }

    #[test]
    fn mixer_mute_silences_only_its_channel() {
        let mut r = sounding_rack();
        let mut a = vec![0.0f32; 1024];
        r.render_block(&mut a);
        assert!(a.iter().any(|v| *v != 0.0), "rack should sound");
        for v in a.iter() {
            assert!(v.is_finite());
        }
        // Mute everything: silence, even with notes held.
        r.mute = [true, true, true, true];
        let mut b = vec![0.0f32; 1024];
        r.render_block(&mut b);
        assert!(b.iter().all(|v| *v == 0.0), "muted rack must be silent");
    }

    #[test]
    fn gate_row_fires_on_sp_step() {
        let mut r = Rack::new();
        r.sp.playing = true;
        // Gate the next step for grandmother only.
        let target = ((r.sp.last_step + 1) % 16) as usize;
        r.gm_gate[target] = true;
        for _ in 0..40 {
            let mut blk = vec![0.0f32; 1024];
            r.render_block(&mut blk);
            if r.sp.last_step as usize % 16 == target {
                break;
            }
        }
        assert_eq!(r.sp.last_step as usize % 16, target);
        // SP muted, GM live: a gated note must now sound.
        r.mute[0] = true;
        let mut blk = vec![0.0f32; 1024];
        r.render_block(&mut blk);
        assert!(blk.iter().any(|v| *v != 0.0), "gated note should sound");
        // Control: no gates -> GM silent with SP muted.
        let mut r2 = Rack::new();
        r2.sp.playing = true;
        r2.mute[0] = true;
        for _ in 0..40 {
            let mut blk = vec![0.0f32; 1024];
            r2.render_block(&mut blk);
        }
        let mut blk = vec![0.0f32; 1024];
        r2.render_block(&mut blk);
        assert!(blk.iter().all(|v| *v == 0.0), "ungated GM must stay silent");
    }

    #[test]
    fn filter_in_adds_filtered_source_to_channel() {
        let mut plain = sounding_rack();
        let mut a = vec![0.0f32; 1024];
        plain.render_block(&mut a);
        let mut routed = sounding_rack();
        routed.arp_filter_in = FilterIn::Sp;
        let mut b = vec![0.0f32; 1024];
        routed.render_block(&mut b);
        assert_ne!(a, b, "drums through the arp filter must change the mix");
        for v in b.iter() {
            assert!(v.is_finite());
        }
    }

    #[test]
    fn gate_trigger_applies_step_offsets() {
        let mut pat = [false; 16];
        pat[3] = true;
        let mut notes = [0i8; 16];
        notes[3] = 12;
        assert_eq!(gate_trigger(&pat, &notes, 48, 3), Some(60));
        notes[3] = -24;
        assert_eq!(gate_trigger(&pat, &notes, 48, 3), Some(24));
        notes[3] = -48;
        assert_eq!(gate_trigger(&pat, &notes, 48, 3), Some(0), "clamped at MIDI 0");
        assert_eq!(gate_trigger(&pat, &notes, 48, 4), None, "no hit, no note");
    }

    #[test]
    fn gate_patch_borrows_sp_row_but_keeps_own_notes() {
        let mut r = Rack::new();
        r.sp.playing = true;
        // SP row hits the next step; GM's own row is empty.
        let target = ((r.sp.last_step + 1) % 16) as usize;
        r.sp.patterns[r.sp_sel][target] = true;
        r.gm_gate_in = GateIn::Sp;
        r.gm_gate_notes[target] = 7;
        for _ in 0..40 {
            let mut blk = vec![0.0f32; 1024];
            r.render_block(&mut blk);
            if r.sp.last_step as usize % 16 == target {
                break;
            }
        }
        assert_eq!(r.sp.last_step as usize % 16, target);
        r.mute[0] = true;
        let mut blk = vec![0.0f32; 1024];
        r.render_block(&mut blk);
        assert!(blk.iter().any(|v| *v != 0.0), "patched gate must sound on the SP hit");
        // Control: same row, no patch -> GM silent.
        let mut r2 = Rack::new();
        r2.sp.playing = true;
        r2.sp.patterns[r2.sp_sel][target] = true;
        for _ in 0..40 {
            let mut blk = vec![0.0f32; 1024];
            r2.render_block(&mut blk);
        }
        r2.mute[0] = true;
        let mut blk = vec![0.0f32; 1024];
        r2.render_block(&mut blk);
        assert!(blk.iter().all(|v| *v == 0.0), "unpatched GM must ignore the SP row");
    }

    #[test]
    fn filter_in_samp_routes_sampler_through_filter() {
        let mut plain = sounding_rack();
        plain.samp.note_on(60);
        let mut a = vec![0.0f32; 1024];
        plain.render_block(&mut a);
        let mut routed = sounding_rack();
        routed.samp.note_on(60);
        routed.arp_filter_in = FilterIn::Samp;
        let mut b = vec![0.0f32; 1024];
        routed.render_block(&mut b);
        assert_ne!(a, b, "sampler through the arp filter must change the mix");
    }

    #[test]
    fn cv_lfo_bends_cutoff_and_restores_base() {
        let mut r = Rack::new();
        r.gm.voice.note_on(48);
        r.mute = [true, false, true, true];
        let base = r.gm.params.cutoff;
        let mut plain = vec![0.0f32; 1024];
        for _ in 0..40 {
            r.render_block(&mut plain);
        }
        assert!(plain.iter().any(|v| *v != 0.0), "GM alone must sound");
        let mut r2 = Rack::new();
        r2.gm.voice.note_on(48);
        r2.mute = [true, false, true, true];
        r2.gm_cv_in = CvIn::Lfo;
        let mut bent = vec![0.0f32; 1024];
        for _ in 0..40 {
            r2.render_block(&mut bent);
        }
        assert_ne!(plain, bent, "LFO on cutoff must change the GM voice");
        assert_eq!(r2.gm.params.cutoff, base, "base cutoff must be restored after render");
    }

    #[test]
    fn cycle_filter_in_reaches_samp() {
        let mut r = Rack::new();
        r.cycle_filter_in(0);
        assert_eq!(r.gm_filter_in, FilterIn::Sp);
        r.cycle_filter_in(0);
        assert_eq!(r.gm_filter_in, FilterIn::Arp);
        r.cycle_filter_in(0);
        assert_eq!(r.gm_filter_in, FilterIn::Samp);
        r.cycle_filter_in(0);
        assert_eq!(r.gm_filter_in, FilterIn::Off);
    }

    #[test]
    fn transport_stop_releases_gate_notes() {
        let mut r = Rack::new();
        r.sp.playing = true;
        r.gm_gate = [true; 16];
        for _ in 0..40 {
            let mut blk = vec![0.0f32; 1024];
            r.render_block(&mut blk);
        }
        r.mute[0] = true;
        let mut blk = vec![0.0f32; 1024];
        r.render_block(&mut blk);
        assert!(blk.iter().any(|v| *v != 0.0), "gate note should sound first");
        r.toggle_transport();
        assert!(!r.sp.playing);
        // Render past any release tail; the gate note must die out.
        for _ in 0..200 {
            let mut blk = vec![0.0f32; 1024];
            r.render_block(&mut blk);
        }
        let mut blk = vec![0.0f32; 1024];
        r.render_block(&mut blk);
        assert!(
            blk.iter().map(|v| v.abs()).fold(0.0f32, f32::max) < 1e-3,
            "stopped gate note must release to silence"
        );
    }
}


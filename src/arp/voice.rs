// Vendored from arp2600 (see README: vendored engines).
//! Voice: three VCOs, voice bus, ladder VCF, VCA, ADSR + AR, ring
//! modulator, noise, sample & hold, lag, utility mixer, spring reverb.
//! Patch reads use the previous sample's jack values (see patch.rs),
//! so any feedback patch is stable. The default (unpatched) signal
//! path is the classic subtractive voice: keyboard CV -> VCOs ->
//! voice bus -> VCF -> VCA, ADSR -> VCA CV, AR -> VCF CV.

use crate::arp::dsp::{ring, EnvFollower, Lag, PinkNoise, SampleHold, WhiteNoise};
use crate::arp::env::{Adsr, Ar};
use crate::arp::filter::Ladder;
use crate::arp::osc::{Vco, Wave};
use crate::arp::patch::{inp, out, Patch};
use crate::arp::reverb::SpringReverb;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoiseType {
    White,
    Pink,
}

#[derive(Clone)]
pub struct Params {
    pub vco1_wave: Wave,
    pub vco1_coarse: f32,
    pub vco1_fine: f32,
    pub vco1_fm_amt: f32,
    pub vco1_level: f32,
    pub vco2_wave: Wave,
    pub vco2_coarse: f32,
    pub vco2_fine: f32,
    pub vco2_pw: f32,
    pub vco2_fm_amt: f32,
    pub vco2_level: f32,
    pub vco3_wave: Wave,
    pub vco3_coarse: f32,
    pub vco3_fine: f32,
    pub vco3_pw: f32,
    pub vco3_fm_amt: f32,
    pub vco3_level: f32,
    pub noise_type: NoiseType,
    pub noise_level: f32,
    pub ring_level: f32,
    pub sh_rate: f32,
    pub lag_time: f32,
    pub glide: f32,
    pub vcf_cutoff: f32,
    pub vcf_reso: f32,
    pub vcf_keytrack: f32,
    pub vcf_cv1_amt: f32,
    pub vcf_cv2_amt: f32,
    pub vca_gain: f32,
    pub vca_cv1_amt: f32,
    pub vca_cv2_amt: f32,
    pub adsr_a: f32,
    pub adsr_d: f32,
    pub adsr_s: f32,
    pub adsr_r: f32,
    pub ar_a: f32,
    pub ar_r: f32,
    pub mix_levels: [f32; 4],
    pub rev_mix: f32,
    pub pre_gain: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            vco1_wave: Wave::Saw,
            vco1_coarse: 0.0,
            vco1_fine: 0.0,
            vco1_fm_amt: 0.0,
            vco1_level: 0.8,
            vco2_wave: Wave::Saw,
            vco2_coarse: 0.0,
            vco2_fine: 0.0,
            vco2_pw: 0.5,
            vco2_fm_amt: 0.0,
            vco2_level: 0.0,
            vco3_wave: Wave::Saw,
            vco3_coarse: 0.0,
            vco3_fine: 0.0,
            vco3_pw: 0.5,
            vco3_fm_amt: 0.0,
            vco3_level: 0.0,
            noise_type: NoiseType::White,
            noise_level: 0.0,
            ring_level: 0.0,
            sh_rate: 4.0,
            lag_time: 0.05,
            glide: 0.0,
            vcf_cutoff: 4000.0,
            vcf_reso: 0.2,
            vcf_keytrack: 0.0,
            vcf_cv1_amt: 1.0,
            vcf_cv2_amt: 0.0,
            vca_gain: 0.0,
            vca_cv1_amt: 1.0,
            vca_cv2_amt: 0.0,
            adsr_a: 0.01,
            adsr_d: 0.15,
            adsr_s: 0.6,
            adsr_r: 0.25,
            ar_a: 0.01,
            ar_r: 0.3,
            mix_levels: [0.8, 0.8, 0.8, 0.8],
            rev_mix: 0.25,
            pre_gain: 1.0,
        }
    }
}

pub struct Synth {
    pub params: Params,
    pub patch: Patch,
    pub sr: f32,
    vco1: Vco,
    vco2: Vco,
    vco3: Vco,
    vcf: Ladder,
    adsr: Adsr,
    ar: Ar,
    white: WhiteNoise,
    pink: PinkNoise,
    sh: SampleHold,
    lag: Lag,
    glide_lag: Lag,
    follower: EnvFollower,
    reverb: SpringReverb,
    // Note state.
    note_midi: f32,
    gate: bool,
    pedal: bool,
    pending_off: bool,
    bend: f32, // semitones
    prev_sh_clk: bool,
    outs_cur: [f32; out::COUNT],
    outs_last: [f32; out::COUNT],
}

impl Synth {
    pub fn new(sr: f32) -> Self {
        Self {
            params: Params::default(),
            patch: Patch::new(),
            sr,
            vco1: Vco::new(Wave::Saw),
            vco2: Vco::new(Wave::Saw),
            vco3: Vco::new(Wave::Saw),
            vcf: Ladder::new(),
            adsr: Adsr::new(),
            ar: Ar::new(),
            white: WhiteNoise::new(0x1234567),
            pink: PinkNoise::new(0x7654321),
            sh: SampleHold::new(),
            lag: Lag::new(),
            glide_lag: Lag::new(),
            follower: EnvFollower::new(sr),
            reverb: SpringReverb::new(sr),
            note_midi: 60.0,
            gate: false,
            pedal: false,
            pending_off: false,
            bend: 0.0,
            prev_sh_clk: false,
            outs_cur: [0.0; out::COUNT],
            outs_last: [0.0; out::COUNT],
        }
    }

    pub fn note_on(&mut self, midi: f32) {
        self.note_midi = midi;
        self.gate = true;
        self.pending_off = false;
        self.adsr.gate_on();
        self.ar.gate_on();
    }

    pub fn note_off(&mut self, midi: f32) {
        if (midi - self.note_midi).abs() < 0.5 {
            if self.pedal {
                self.pending_off = true;
            } else {
                self.gate = false;
                self.adsr.gate_off();
                self.ar.gate_off();
            }
        }
    }

    pub fn pedal(&mut self, down: bool) {
        self.pedal = down;
        if !down && self.pending_off {
            self.pending_off = false;
            self.gate = false;
            self.adsr.gate_off();
            self.ar.gate_off();
        }
    }

    pub fn bend(&mut self, semitones: f32) {
        self.bend = semitones.clamp(-2.0, 2.0);
    }

    pub fn all_notes_off(&mut self) {
        self.gate = false;
        self.pending_off = false;
        self.adsr.gate_off();
        self.ar.gate_off();
    }

    pub fn gate(&self) -> bool {
        self.gate
    }

    pub fn current_midi(&self) -> f32 {
        self.note_midi
    }

    /// Last published value of an output jack (tests only).
    #[cfg(test)]
    pub fn jack(&self, o: usize) -> f32 {
        self.outs_cur[o]
    }

    fn src(&self, input: usize) -> f32 {
        match self.patch.source_for(input) {
            Some(o) => self.outs_last[o],
            None => 0.0,
        }
    }

    fn sync_vco(v: &mut Vco, wave: Wave, coarse: f32, fine: f32, pw: f32, fm_amt: f32) {
        v.wave = wave;
        v.coarse = coarse;
        v.fine = fine;
        v.pulse_width = pw;
        v.fm_amount = fm_amt;
    }

    /// One mono sample; returns (left, right).
    fn step(&mut self) -> (f32, f32) {
        let p = &self.params;
        let dt = 1.0 / self.sr;

        // Envelopes advance first; their outputs publish below.
        let adsr_v = self.adsr.next(dt);
        let ar_v = self.ar.next(dt);

        // Glide on the note CV, then keyboard CV in volts.
        let target_midi = self.note_midi + self.bend;
        let glided = self.glide_lag.next(target_midi, p.glide, self.sr);
        let kb_cv = (glided - 69.0) / 12.0;

        // Noise first: S&H defaults to white noise.
        let noise_w = self.white.next();
        let noise_p = self.pink.next();

        // Lag processor (default input: keyboard CV).
        let lag_out = self.lag.next(self.src(inp::LAG_IN), p.lag_time, self.sr);

        // Sample & hold (default input: white noise).
        let sh_clk = self.src(inp::SH_CLK) > 0.5;
        let sh_out = self.sh.next(
            self.src(inp::SH_IN),
            p.sh_rate,
            sh_clk,
            self.prev_sh_clk,
            self.sr,
        );
        self.prev_sh_clk = sh_clk;

        // VCOs. CV inputs read last-sample values (patch rule); FM too.
        Self::sync_vco(
            &mut self.vco1,
            p.vco1_wave,
            p.vco1_coarse,
            p.vco1_fine,
            0.5,
            p.vco1_fm_amt,
        );
        Self::sync_vco(
            &mut self.vco2,
            p.vco2_wave,
            p.vco2_coarse,
            p.vco2_fine,
            p.vco2_pw,
            p.vco2_fm_amt,
        );
        Self::sync_vco(
            &mut self.vco3,
            p.vco3_wave,
            p.vco3_coarse,
            p.vco3_fine,
            p.vco3_pw,
            p.vco3_fm_amt,
        );
        // PWM jacks override the panel width when patched (PWM has no
        // normalization, so a source means a cable).
        let pw2 = if self.patch.source_for(inp::VCO2_PWM).is_some() {
            0.5 + 0.45 * self.src(inp::VCO2_PWM).clamp(-1.0, 1.0)
        } else {
            p.vco2_pw
        };
        let pw3 = if self.patch.source_for(inp::VCO3_PWM).is_some() {
            0.5 + 0.45 * self.src(inp::VCO3_PWM).clamp(-1.0, 1.0)
        } else {
            p.vco3_pw
        };
        self.vco2.pulse_width = pw2;
        self.vco3.pulse_width = pw3;

        self.vco1.cv_input = self.src(inp::VCO1_CV);
        self.vco2.cv_input = self.src(inp::VCO2_CV);
        self.vco3.cv_input = self.src(inp::VCO3_CV);
        self.vco1.fm_input = self.src(inp::VCO1_FM1) + self.src(inp::VCO1_FM2);
        self.vco2.fm_input = self.src(inp::VCO2_FM1) + self.src(inp::VCO2_FM2);
        self.vco3.fm_input = self.src(inp::VCO3_FM1) + self.src(inp::VCO3_FM2);

        // Advance each VCO once, then read every waveform from the
        // shared phase so all waveform jacks stay live. The pitch
        // reference is A440 (MIDI 69): the note itself arrives ONLY
        // through the VCO CV jacks (normalized to keyboard CV). Using
        // the note as the base AND adding CV would count it twice and
        // pitch every note except A440 flat by its distance from 69.
        let sr = self.sr;
        for v in [&mut self.vco1, &mut self.vco2, &mut self.vco3] {
            let f = v.freq(69.0);
            v.advance(f, sr);
        }
        let v1_saw = self.vco1.eval(Wave::Saw);
        let v1_sqr = self.vco1.eval(Wave::Square);
        let v2_saw = self.vco2.eval(Wave::Saw);
        let v2_pls = self.vco2.eval(Wave::Pulse);
        let v2_sin = self.vco2.eval(Wave::Sine);
        let v2_tri = self.vco2.eval(Wave::Triangle);
        let v3_saw = self.vco3.eval(Wave::Saw);
        let v3_pls = self.vco3.eval(Wave::Pulse);
        let v3_sin = self.vco3.eval(Wave::Sine);
        let v3_tri = self.vco3.eval(Wave::Triangle);

        let sel = |wave: Wave, saw: f32, sqr: f32, pls: f32, sin: f32, tri: f32| -> f32 {
            match wave {
                Wave::Saw => saw,
                Wave::Square => sqr,
                Wave::Pulse => pls,
                Wave::Sine => sin,
                Wave::Triangle => tri,
            }
        };
        let v1_sel = sel(p.vco1_wave, v1_saw, v1_sqr, v1_saw, v1_saw, v1_saw);
        let v2_sel = sel(p.vco2_wave, v2_saw, v2_saw, v2_pls, v2_sin, v2_tri);
        let v3_sel = sel(p.vco3_wave, v3_saw, v3_saw, v3_pls, v3_sin, v3_tri);

        // Ring modulator (defaults: VCO1 saw x VCO2 sine).
        let ring_out = ring(self.src(inp::RING_IN1), self.src(inp::RING_IN2));

        // Voice bus: VCOs + noise + ring at panel levels.
        let noise_sel = match p.noise_type {
            NoiseType::White => noise_w,
            NoiseType::Pink => noise_p,
        };
        let voice = v1_sel * p.vco1_level
            + v2_sel * p.vco2_level
            + v3_sel * p.vco3_level
            + noise_sel * p.noise_level
            + ring_out * p.ring_level;

        // Utility mixer (defaults: VCO saws + white noise).
        let mixer = self.src(inp::MIX_IN1) * p.mix_levels[0]
            + self.src(inp::MIX_IN2) * p.mix_levels[1]
            + self.src(inp::MIX_IN3) * p.mix_levels[2]
            + self.src(inp::MIX_IN4) * p.mix_levels[3];

        // VCF (default audio: voice bus; default CV1: AR).
        self.vcf.cutoff = p.vcf_cutoff;
        self.vcf.resonance = p.vcf_reso;
        self.vcf.key_track = p.vcf_keytrack;
        self.vcf.key_midi = glided;
        self.vcf.cv_amount = 1.0;
        self.vcf.cv_input =
            self.src(inp::VCF_CV1) * p.vcf_cv1_amt + self.src(inp::VCF_CV2) * p.vcf_cv2_amt;
        let vcf_in = self.src(inp::VCF_IN1) + self.src(inp::VCF_IN2);
        let vcf_out = self.vcf.next(vcf_in, self.sr);

        // VCA (default audio: VCF; default CV1: ADSR).
        let vca_cv = p.vca_gain
            + self.src(inp::VCA_CV1) * p.vca_cv1_amt
            + self.src(inp::VCA_CV2) * p.vca_cv2_amt;
        let vca_cv = vca_cv.clamp(0.0, 1.2);
        let vca_out = self.src(inp::VCA_IN) * vca_cv;

        // Reverb (default input: VCA).
        let rev_in = self.src(inp::REV_IN1) + self.src(inp::REV_IN2);
        let (rev_l, rev_r) = self.reverb.next(rev_in);

        // Preamp + envelope follower (default input: VCA output, so
        // with nothing patched the follower tracks playing dynamics).
        let pre_out = self.src(inp::PRE_IN) * p.pre_gain;
        let env_out = self.follower.next(pre_out);

        // Publish all jack values for the next sample.
        let mut cur = [0.0f32; out::COUNT];
        cur[out::VCO1_SAW] = v1_saw;
        cur[out::VCO1_SQR] = v1_sqr;
        cur[out::VCO2_SAW] = v2_saw;
        cur[out::VCO2_PLS] = v2_pls;
        cur[out::VCO2_SIN] = v2_sin;
        cur[out::VCO2_TRI] = v2_tri;
        cur[out::VCO3_SAW] = v3_saw;
        cur[out::VCO3_PLS] = v3_pls;
        cur[out::VCO3_SIN] = v3_sin;
        cur[out::VCO3_TRI] = v3_tri;
        cur[out::NOISE_W] = noise_w;
        cur[out::NOISE_P] = noise_p;
        cur[out::RING] = ring_out;
        cur[out::SH] = sh_out;
        cur[out::VCF] = vcf_out;
        cur[out::VCA] = vca_out;
        cur[out::ADSR] = adsr_v;
        cur[out::AR] = ar_v;
        cur[out::KB_CV] = kb_cv;
        cur[out::GATE] = if self.gate { 1.0 } else { 0.0 };
        cur[out::LAG] = lag_out;
        cur[out::MIXER] = mixer;
        cur[out::REVERB] = (rev_l + rev_r) * 0.5;
        cur[out::VOICE] = voice;
        cur[out::PRE] = pre_out;
        cur[out::ENV] = env_out;
        self.outs_last = self.outs_cur;
        self.outs_cur = cur;

        // Master: dry VCA + wet reverb, soft-clipped.
        let wet = p.rev_mix;
        let l = (vca_out * 0.9 + rev_l * wet).tanh();
        let r = (vca_out * 0.9 + rev_r * wet).tanh();
        (l, r)
    }

    /// Render interleaved stereo into `out`.
    pub fn render(&mut self, out: &mut [f32]) {
        for frame in out.chunks_mut(2) {
            if frame.len() < 2 {
                break;
            }
            let (l, r) = self.step();
            frame[0] = l;
            frame[1] = r;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_patch_sounds() {
        let mut s = Synth::new(44100.0);
        s.note_on(69.0);
        let mut buf = vec![0.0; 44100 * 2];
        s.render(&mut buf);
        let peak = buf.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.05, "peak={peak}");
        assert!(buf.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn gate_off_silences() {
        let mut s = Synth::new(44100.0);
        s.note_on(69.0);
        let mut buf = vec![0.0; 4410 * 2];
        s.render(&mut buf);
        s.note_off(69.0);
        let mut buf2 = vec![0.0; 44100 * 2];
        s.render(&mut buf2);
        let tail = buf2[40000 * 2..]
            .iter()
            .fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(tail < 0.01, "tail={tail}");
    }

    #[test]
    fn patching_vcf_in_breaks_voice_bus() {
        // Patch VCF IN1 from LAG: with note 69 held, KB CV is 0, so the
        // lag output is ~0 and the voice bus is cut from the filter.
        let mut s = Synth::new(44100.0);
        s.patch.connect(out::LAG, inp::VCF_IN1);
        s.note_on(69.0);
        let mut buf = vec![0.0; 44100 * 2];
        s.render(&mut buf);
        let peak = buf.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak < 0.01, "peak={peak}");
    }

    #[test]
    fn envelope_follower_tracks_playing() {
        let mut s = Synth::new(44100.0);
        s.note_on(69.0);
        let mut buf = vec![0.0; 44100 * 2];
        s.render(&mut buf);
        let env = s.jack(out::ENV);
        assert!(env > 0.05, "env={env} while a note plays");
        s.note_off(69.0);
        let mut buf2 = vec![0.0; 44100 * 2];
        s.render(&mut buf2);
        let env = s.jack(out::ENV);
        assert!(env < 0.01, "env={env} after a second of silence");
        // Zero gain starves the follower even while a note plays.
        let mut s = Synth::new(44100.0);
        s.params.pre_gain = 0.0;
        s.note_on(69.0);
        let mut buf3 = vec![0.0; 44100 * 2];
        s.render(&mut buf3);
        assert!(s.jack(out::ENV) < 0.01, "env={}", s.jack(out::ENV));
    }

    #[test]
    fn voice_pitch_tracks_note_not_69() {
        // Regression: the note must arrive ONLY through the CV jacks.
        // Note 60 (C4, 261.6 Hz) rendered with a wide-open filter and
        // no reverb must cross upward ~105 times in 0.4 s. Counting
        // the note twice (base + CV) would give 155.6 Hz (~62).
        let mut s = Synth::new(44100.0);
        s.params.vcf_cutoff = 20000.0;
        s.params.rev_mix = 0.0;
        s.note_on(60.0);
        let mut buf = vec![0.0; 44100 * 2];
        s.render(&mut buf);
        // Settle 0.1 s, then count upward zero crossings over 0.4 s.
        let left: Vec<f32> = buf.iter().step_by(2).copied().collect();
        let mut crossings = 0;
        for w in left[4410..4410 + 17640].windows(2) {
            if w[0] < 0.0 && w[1] >= 0.0 {
                crossings += 1;
            }
        }
        assert!(
            (90..=120).contains(&crossings),
            "crossings={crossings}, expected ~105 for 261.6 Hz"
        );
    }
}

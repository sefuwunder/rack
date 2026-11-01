//! Rack sampler: one pitched sample voice, mono, last-note-wins.
//!
//! Samples load from .wav/.mp3 (via the SP engine's decoder) and are
//! resampled to device rate on load, so playback is a plain phase
//! accumulator. The sample root is C4 (MIDI 60): playing C4 plays the
//! sample at its recorded pitch, other notes resample by semitone.
//! The default voice is a 1s 440 Hz sine, so the strip sounds before
//! any file is dropped on it.

use crate::sp::decode::{decode_file, AudioData};
use crate::rack::SR;

pub struct Sampler {
    buf: Vec<f32>,
    /// Display name (file name, or "SINE (DEFAULT)").
    pub name: String,
    /// Source file path, when loaded from disk (saved in projects).
    pub path: Option<String>,
    pub level: f32,
    pub tune_st: f32,
    /// Playback start as a fraction of the sample, 0..1.
    pub start: f32,
    /// Loop while the note is held (otherwise one shot, then silence).
    pub loop_: bool,
    /// Keyboard octave offset; base note = 48 + octave * 12.
    pub octave: i32,
    pos: f32,
    playing: bool,
    held: Option<u8>,
}

fn sine_default() -> Vec<f32> {
    let n = SR as usize;
    let fade = (SR * 0.01) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            let v = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.8;
            let env = if i < fade {
                i as f32 / fade as f32
            } else if i >= n - fade {
                (n - i) as f32 / fade as f32
            } else {
                1.0
            };
            v * env
        })
        .collect()
}

impl Sampler {
    pub fn new() -> Self {
        Sampler {
            buf: sine_default(),
            name: "SINE (DEFAULT)".to_string(),
            path: None,
            level: 0.8,
            tune_st: 0.0,
            start: 0.0,
            loop_: false,
            octave: 0,
            pos: 0.0,
            playing: false,
            held: None,
        }
    }

    pub fn base_note(&self) -> u8 {
        (48 + self.octave * 12).clamp(12, 96) as u8
    }

    /// Load a file, resampling to device rate. Keeps the old sample on
    /// failure (the error names the file, never its contents).
    pub fn load_file(&mut self, path: &str) -> Result<(), String> {
        let data = decode_file(path)?;
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
        self.load_audio(data, name, Some(path.to_string()));
        Ok(())
    }

    fn load_audio(&mut self, data: AudioData, name: String, path: Option<String>) {
        let sr = data.sample_rate.max(1) as f32;
        let ratio = sr / SR;
        let out_len = ((data.samples.len() as f32 / ratio) as usize).max(1);
        let mut buf = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let p = i as f32 * ratio;
            let i0 = (p.floor() as usize).min(data.samples.len().saturating_sub(1));
            let i1 = (i0 + 1).min(data.samples.len().saturating_sub(1));
            let f = p - p.floor();
            buf.push(data.samples[i0] + (data.samples[i1] - data.samples[i0]) * f);
        }
        self.buf = buf;
        self.name = name;
        self.path = path;
        self.playing = false;
        self.held = None;
    }

    pub fn note_on(&mut self, midi: u8) {
        if self.buf.is_empty() {
            return;
        }
        self.pos = self.start.clamp(0.0, 1.0) * self.buf.len() as f32;
        self.playing = true;
        self.held = Some(midi);
    }

    pub fn note_off(&mut self, midi: u8) {
        if self.held == Some(midi) {
            self.playing = false;
            self.held = None;
        }
    }

    pub fn render(&mut self, out: &mut [f32]) {
        if !self.playing || self.buf.is_empty() {
            for x in out.iter_mut() {
                *x = 0.0;
            }
            return;
        }
        let midi = self.held.unwrap_or(60) as f32;
        let rate = 2.0f32.powf((midi - 60.0 + self.tune_st) / 12.0);
        let len = self.buf.len() as f32;
        for x in out.iter_mut() {
            if self.pos >= len {
                if self.loop_ {
                    self.pos %= len;
                } else {
                    self.playing = false;
                    self.held = None;
                    *x = 0.0;
                    continue;
                }
            }
            let i0 = self.pos.floor() as usize;
            let i1 = (i0 + 1).min(self.buf.len() - 1);
            let f = self.pos - self.pos.floor();
            *x = (self.buf[i0] + (self.buf[i1] - self.buf[i0]) * f) * self.level;
            self.pos += rate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_voice() -> Sampler {
        let mut s = Sampler::new();
        s.level = 1.0;
        s
    }

    #[test]
    fn note_on_sounds_and_note_off_silences() {
        let mut s = sine_voice();
        s.note_on(60);
        let mut blk = vec![0.0f32; 1024];
        s.render(&mut blk);
        assert!(blk.iter().any(|v| *v != 0.0), "held sample must sound");
        s.note_off(60);
        let mut blk = vec![0.0f32; 1024];
        s.render(&mut blk);
        assert!(blk.iter().all(|v| *v == 0.0), "released sample must stop");
    }

    #[test]
    fn octave_up_advances_twice_as_fast() {
        // Count zero crossings: an octave up doubles them.
        let mut count = |midi: u8| -> usize {
            let mut s = sine_voice();
            s.note_on(midi);
            let mut blk = vec![0.0f32; 4410];
            s.render(&mut blk);
            blk.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
        };
        let lo = count(60);
        let hi = count(72);
        assert!(lo > 20, "base note must cross zero regularly: {lo}");
        assert!((hi as f32 / lo as f32 - 2.0).abs() < 0.15, "octave up must double crossings: {lo} vs {hi}");
    }

    #[test]
    fn load_file_reads_a_wav_and_replaces_default() {
        // Minimal PCM16 mono WAV, 0.1s of 440 Hz at 22050 Hz.
        let sr = 22050u32;
        let n = (sr / 10) as usize;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&((36 + n * 2) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&sr.to_le_bytes());
        bytes.extend_from_slice(&(sr * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&((n * 2) as u32).to_le_bytes());
        for i in 0..n {
            let v = ((2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin() * 12000.0) as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let path = "/tmp/rack-samp-test.wav";
        std::fs::write(path, bytes).unwrap();
        let mut s = Sampler::new();
        assert_eq!(s.name, "SINE (DEFAULT)");
        s.load_file(path).unwrap();
        assert_eq!(s.name, "rack-samp-test.wav");
        assert_eq!(s.path.as_deref(), Some(path));
        // Resampled to 44100: ~0.1s * 44100 samples.
        assert!((s.buf.len() as i32 - 4410).abs() < 10, "resampled length: {}", s.buf.len());
        s.note_on(60);
        let mut blk = vec![0.0f32; 1024];
        s.render(&mut blk);
        assert!(blk.iter().any(|v| *v != 0.0), "loaded file must sound");
        assert!(s.load_file("/tmp/rack-samp-missing.wav").is_err());
        assert_eq!(s.name, "rack-samp-test.wav", "failed load keeps the old sample");
    }

    #[test]
    fn one_shot_ends_and_loop_wraps() {
        let mut s = sine_voice();
        s.buf = vec![0.5; 100];
        s.note_on(60);
        let mut blk = vec![0.0f32; 1024];
        s.render(&mut blk);
        assert!(blk[200..].iter().all(|v| *v == 0.0), "one-shot must end at the sample end");
        let mut s = sine_voice();
        s.buf = vec![0.5; 100];
        s.loop_ = true;
        s.note_on(60);
        let mut blk = vec![0.0f32; 1024];
        s.render(&mut blk);
        assert!(blk[200..].iter().all(|v| *v == 0.5), "loop must wrap while held");
    }
}

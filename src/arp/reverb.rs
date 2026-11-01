// Vendored from arp2600 (see README: vendored engines).
//! Spring-style reverb: a short, bright Schroeder network (series
//! allpasses into parallel feedback combs) with two decorrelated
//! channels. Tuned metallic and lively rather than hall-like.

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
    g: f32,
}

impl Allpass {
    fn new(delay: usize, g: f32) -> Self {
        Self {
            buf: vec![0.0; delay.max(1)],
            idx: 0,
            g,
        }
    }

    fn next(&mut self, x: f32) -> f32 {
        let d = self.buf[self.idx];
        let y = -self.g * x + d;
        self.buf[self.idx] = x + self.g * d;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
}

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    fb: f32,
}

impl Comb {
    fn new(delay: usize, fb: f32) -> Self {
        Self {
            buf: vec![0.0; delay.max(1)],
            idx: 0,
            fb,
        }
    }

    fn next(&mut self, x: f32) -> f32 {
        let d = self.buf[self.idx];
        self.buf[self.idx] = x + self.fb * d;
        self.idx = (self.idx + 1) % self.buf.len();
        d
    }
}

pub struct SpringReverb {
    ap_l: Vec<Allpass>,
    ap_r: Vec<Allpass>,
    comb_l: Vec<Comb>,
    comb_r: Vec<Comb>,
}

impl SpringReverb {
    pub fn new(sr: f32) -> Self {
        // Short delays, bright feedback: spring character.
        let ap_delays = [0.005, 0.0017];
        let comb_delays = [0.0297, 0.0317, 0.0341, 0.0367];
        let mk = |d: &[f32], g: f32, scale: f32| -> Vec<Allpass> {
            d.iter()
                .map(|s| Allpass::new((s * sr * scale) as usize, g))
                .collect()
        };
        let mkc = |d: &[f32], fb: f32, scale: f32| -> Vec<Comb> {
            d.iter()
                .map(|s| Comb::new((s * sr * scale) as usize, fb))
                .collect()
        };
        Self {
            ap_l: mk(&ap_delays, 0.7, 1.0),
            ap_r: mk(&ap_delays, 0.7, 1.03),
            comb_l: mkc(&comb_delays, 0.72, 1.0),
            comb_r: mkc(&comb_delays, 0.72, 1.07),
        }
    }

    pub fn next(&mut self, x: f32) -> (f32, f32) {
        let mut l = x;
        for ap in self.ap_l.iter_mut() {
            l = ap.next(l);
        }
        let mut r = x;
        for ap in self.ap_r.iter_mut() {
            r = ap.next(r);
        }
        let mut sl = 0.0;
        for c in self.comb_l.iter_mut() {
            sl += c.next(l);
        }
        let mut sr_ = 0.0;
        for c in self.comb_r.iter_mut() {
            sr_ += c.next(r);
        }
        (sl * 0.25, sr_ * 0.25)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_decays_finite() {
        let mut r = SpringReverb::new(44100.0);
        let mut peak = 0.0f32;
        let mut tail = 0.0f32;
        for i in 0..44100 {
            let (l, rr) = r.next(if i == 0 { 1.0 } else { 0.0 });
            assert!(l.is_finite() && rr.is_finite());
            peak = peak.max(l.abs());
            if i > 40000 {
                tail = tail.max(l.abs());
            }
        }
        assert!(peak > 0.01);
        assert!(tail < peak * 0.5, "tail={tail} peak={peak}");
    }
}

// Vendored from grandmother (see README: vendored engines).
//! Spring-tank-style reverb: a Freeverb-style network (parallel combs with
//! damping plus series allpasses), tuned dark like a spring tank. Mono.

#[derive(Debug)]
struct Comb {
    buf: Vec<f32>,
    idx: usize,
    fb: f32,
    damp: f32,
    store: f32,
}

impl Comb {
    fn new(len: usize, fb: f32, damp: f32) -> Self {
        Comb {
            buf: vec![0.0; len],
            idx: 0,
            fb,
            damp,
            store: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * (1.0 - self.damp) + self.store * self.damp;
        self.buf[self.idx] = x + self.store * self.fb;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

#[derive(Debug)]
struct Allpass {
    buf: Vec<f32>,
    idx: usize,
    fb: f32,
}

impl Allpass {
    fn new(len: usize, fb: f32) -> Self {
        Allpass {
            buf: vec![0.0; len],
            idx: 0,
            fb,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let bufout = self.buf[self.idx];
        let y = -x * self.fb + bufout;
        self.buf[self.idx] = x + bufout * self.fb;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
}

#[derive(Debug)]
pub struct Reverb {
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
}

impl Reverb {
    pub fn new(sr: f32) -> Self {
        // Freeverb comb lengths at 44.1 kHz, scaled to the sample rate.
        let scale = sr / 44100.0;
        let comb_lens = [1116, 1188, 1277, 1356];
        let ap_lens = [556, 441];
        Reverb {
            combs: comb_lens
                .iter()
                .map(|&l| Comb::new(((l as f32 * scale) as usize).max(16), 0.84, 0.2))
                .collect(),
            allpasses: ap_lens
                .iter()
                .map(|&l| Allpass::new(((l as f32 * scale) as usize).max(16), 0.5))
                .collect(),
        }
    }

    /// Wet-only output for one sample.
    pub fn process(&mut self, x: f32) -> f32 {
        let mut sum = 0.0;
        for c in self.combs.iter_mut() {
            sum += c.process(x);
        }
        sum *= 0.25;
        for a in self.allpasses.iter_mut() {
            sum = a.process(sum);
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_decays_and_stays_bounded() {
        let mut r = Reverb::new(44100.0);
        let mut peak_after = 0.0f32;
        let mut max = 0.0f32;
        for i in 0..44100 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let y = r.process(x);
            assert!(y.is_finite());
            max = max.max(y.abs());
            if i > 22050 {
                peak_after = peak_after.max(y.abs());
            }
        }
        assert!(max > 0.0, "reverb should ring after an impulse");
        assert!(peak_after < max * 0.5, "tail should decay: {peak_after} vs {max}");
    }
}

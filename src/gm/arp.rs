// Vendored from grandmother (see README: vendored engines).
//! Arpeggiator: cycles held notes, expanded across octaves.
//! Pattern order is Up / Down / ping-pong UpDown / Random.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArpMode {
    Up,
    Down,
    UpDown,
    Random,
}

impl ArpMode {
    pub fn next(self) -> ArpMode {
        match self {
            ArpMode::Up => ArpMode::Down,
            ArpMode::Down => ArpMode::UpDown,
            ArpMode::UpDown => ArpMode::Random,
            ArpMode::Random => ArpMode::Up,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ArpMode::Up => "UP",
            ArpMode::Down => "DOWN",
            ArpMode::UpDown => "UP/DN",
            ArpMode::Random => "RAND",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Arp {
    pub mode: ArpMode,
    pub octaves: usize, // 1..=3
    held: Vec<u8>,
    /// Index of the last returned pattern note (-1 = none yet).
    cur: i32,
    dir: i32,
    rand: u32,
}

impl Arp {
    pub fn new() -> Self {
        Arp {
            mode: ArpMode::Up,
            octaves: 1,
            held: Vec::new(),
            cur: -1,
            dir: 1,
            rand: 0x1234_5678,
        }
    }

    pub fn set_held(&mut self, held: &[u8]) {
        let mut h = held.to_vec();
        h.sort_unstable();
        h.dedup();
        self.held = h;
        self.cur = -1;
        self.dir = 1;
    }

    fn pattern(&self) -> Vec<u8> {
        let mut p = Vec::new();
        for oct in 0..self.octaves {
            for &n in &self.held {
                let v = n as i32 + 12 * oct as i32;
                if v <= 127 {
                    p.push(v as u8);
                }
            }
        }
        p
    }

    /// Advance one step and return the note, or None when nothing is held.
    pub fn step(&mut self) -> Option<u8> {
        let p = self.pattern();
        let n = p.len() as i32;
        if n == 0 {
            return None;
        }
        match self.mode {
            ArpMode::Up => {
                self.cur = if self.cur < 0 { 0 } else { (self.cur + 1) % n };
                Some(p[self.cur as usize])
            }
            ArpMode::Down => {
                self.cur = if self.cur < 0 {
                    n - 1
                } else {
                    (self.cur + n - 1) % n
                };
                Some(p[self.cur as usize])
            }
            ArpMode::UpDown => {
                if n == 1 {
                    self.cur = 0;
                    return Some(p[0]);
                }
                if self.cur < 0 {
                    self.cur = 0;
                    self.dir = 1;
                    return Some(p[0]);
                }
                let mut next = self.cur + self.dir;
                if next >= n {
                    self.dir = -1;
                    next = n - 2;
                } else if next < 0 {
                    self.dir = 1;
                    next = 1;
                }
                self.cur = next;
                Some(p[self.cur as usize])
            }
            ArpMode::Random => {
                // xorshift32
                let mut x = self.rand;
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                self.rand = x;
                let idx = (x % n as u32) as usize;
                self.cur = idx as i32;
                Some(p[idx])
            }
        }
    }
}

impl Default for Arp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_down_updown_orders() {
        let mut a = Arp::new();
        a.set_held(&[67, 60, 64]); // unsorted input sorts to 60,64,67
        a.mode = ArpMode::Up;
        assert_eq!(
            (0..4).map(|_| a.step().unwrap()).collect::<Vec<_>>(),
            vec![60, 64, 67, 60]
        );
        a.set_held(&[60, 64, 67]);
        a.mode = ArpMode::Down;
        assert_eq!(
            (0..4).map(|_| a.step().unwrap()).collect::<Vec<_>>(),
            vec![67, 64, 60, 67]
        );
        a.set_held(&[60, 64, 67]);
        a.mode = ArpMode::UpDown;
        assert_eq!(
            (0..6).map(|_| a.step().unwrap()).collect::<Vec<_>>(),
            vec![60, 64, 67, 64, 60, 64]
        );
    }

    #[test]
    fn octave_expansion_and_empty() {
        let mut a = Arp::new();
        a.octaves = 2;
        a.set_held(&[60, 64]);
        a.mode = ArpMode::Up;
        assert_eq!(
            (0..5).map(|_| a.step().unwrap()).collect::<Vec<_>>(),
            vec![60, 64, 72, 76, 60]
        );
        a.set_held(&[]);
        assert_eq!(a.step(), None);
    }

    #[test]
    fn random_stays_in_pattern() {
        let mut a = Arp::new();
        a.mode = ArpMode::Random;
        a.set_held(&[60, 64, 67]);
        for _ in 0..50 {
            let n = a.step().unwrap();
            assert!([60, 64, 67].contains(&n));
        }
    }
}

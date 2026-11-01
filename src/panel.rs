//! Rack panels: layout, drawing, hit-testing, knob specs.
//!
//! One fixed 1280x960 layout, no scrolling: header (transport), SP-1200
//! strip, two synth strips, mixer. Knobs drag vertically; toggles click.
//! All drawing is generic over the canvas target so --shot can render to
//! a surface with the same code the window uses.

use crate::font;
use crate::rack::{FilterIn, Rack};
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::{Canvas, RenderTarget};

pub const W: u32 = 1280;
pub const H: u32 = 960;

// Layout (y bands).
const SP_Y: i32 = 48;
const GM_Y: i32 = 348;
const ARP_Y: i32 = 584;
const MIX_Y: i32 = 820;

const BG: Color = Color::RGB(24, 24, 28);
const RAIL: Color = Color::RGB(10, 10, 12);
const SP_BG: Color = Color::RGB(58, 58, 64);
const GM_BG: Color = Color::RGB(232, 220, 192);
const ARP_BG: Color = Color::RGB(28, 28, 30);
const MIX_BG: Color = Color::RGB(40, 40, 46);
const INK: Color = Color::RGB(230, 230, 230);
const DARK_INK: Color = Color::RGB(30, 30, 30);
const DIM: Color = Color::RGB(140, 140, 150);
const SEL: Color = Color::RGB(255, 200, 80);
const STEP_ON: Color = Color::RGB(255, 170, 60);
const GATE_ON: Color = Color::RGB(90, 200, 120);
const MUTE_RED: Color = Color::RGB(220, 70, 70);
const BTN: Color = Color::RGB(70, 70, 80);
const KEY_WHITE: Color = Color::RGB(240, 240, 235);
const KEY_BLACK: Color = Color::RGB(20, 20, 20);

/// Transient UI state owned by the event loop (never the audio thread).
#[derive(Default)]
pub struct UiState {
    pub drag: Option<(KnobId, i32, f32)>,
    pub mouse_note: Option<(usize, u8)>,
}

/// Every draggable knob in the rack, with its range and scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KnobId {
    SpCutoff,
    SpRes,
    SpLfoRate,
    SpLfoCut,
    SpLfoRes,
    SpLevel,
    GmOsc1,
    GmOsc2,
    GmCutoff,
    GmReso,
    GmAttack,
    GmDecay,
    GmSustain,
    GmRelease,
    GmGlide,
    GmLfoRate,
    ArpOsc1,
    ArpOsc2,
    ArpOsc3,
    ArpCutoff,
    ArpReso,
    ArpAttack,
    ArpDecay,
    ArpSustain,
    ArpRelease,
    ArpGlide,
    MixLevel(usize),
    MixMaster,
}

impl KnobId {
    /// (min, max, log scale).
    fn spec(self) -> (f32, f32, bool) {
        use KnobId::*;
        match self {
            SpCutoff => (80.0, 12000.0, true),
            SpRes => (0.1, 12.0, true),
            SpLfoRate => (0.0, 20.0, false),
            SpLfoCut => (-48.0, 48.0, false),
            SpLfoRes => (-12.0, 12.0, false),
            SpLevel => (0.0, 1.0, false),
            GmOsc1 | GmOsc2 | GmReso | GmSustain => (0.0, 1.0, false),
            GmCutoff => (20.0, 20000.0, true),
            GmAttack | GmDecay | GmRelease => (0.001, 2.0, true),
            GmGlide => (0.0, 1.0, false),
            GmLfoRate => (0.05, 50.0, true),
            ArpOsc1 | ArpOsc2 | ArpOsc3 | ArpReso | ArpSustain => (0.0, 1.0, false),
            ArpCutoff => (20.0, 20000.0, true),
            ArpAttack | ArpDecay | ArpRelease => (0.001, 2.0, true),
            ArpGlide => (0.0, 1.0, false),
            MixLevel(_) | MixMaster => (0.0, 1.0, false),
        }
    }

    fn label(self) -> &'static str {
        use KnobId::*;
        match self {
            SpCutoff => "CUTOFF",
            SpRes => "RES",
            SpLfoRate => "LFO RATE",
            SpLfoCut => "LFO CUT",
            SpLfoRes => "LFO RES",
            SpLevel => "LEVEL",
            GmOsc1 => "OSC1",
            GmOsc2 => "OSC2",
            GmCutoff => "CUTOFF",
            GmReso => "RESO",
            GmAttack => "ATTACK",
            GmDecay => "DECAY",
            GmSustain => "SUSTAIN",
            GmRelease => "RELEASE",
            GmGlide => "GLIDE",
            GmLfoRate => "LFO RATE",
            ArpOsc1 => "OSC1",
            ArpOsc2 => "OSC2",
            ArpOsc3 => "OSC3",
            ArpCutoff => "CUTOFF",
            ArpReso => "RESO",
            ArpAttack => "ATTACK",
            ArpDecay => "DECAY",
            ArpSustain => "SUSTAIN",
            ArpRelease => "RELEASE",
            ArpGlide => "GLIDE",
            MixLevel(_) => "LEVEL",
            MixMaster => "MASTER",
        }
    }

    fn get(self, r: &Rack) -> f32 {
        use KnobId::*;
        let p = r.sp_sel;
        match self {
            SpCutoff => r.sp.pads[p].filter_cutoff,
            SpRes => r.sp.pads[p].filter_res,
            SpLfoRate => r.sp.pads[p].lfo_rate,
            SpLfoCut => r.sp.pads[p].lfo_cut,
            SpLfoRes => r.sp.pads[p].lfo_res,
            SpLevel => r.sp.pads[p].level,
            GmOsc1 => r.gm.params.osc1_level,
            GmOsc2 => r.gm.params.osc2_level,
            GmCutoff => r.gm.params.cutoff,
            GmReso => r.gm.params.reso,
            GmAttack => r.gm.params.a_attack,
            GmDecay => r.gm.params.a_decay,
            GmSustain => r.gm.params.a_sustain,
            GmRelease => r.gm.params.a_release,
            GmGlide => r.gm.params.glide,
            GmLfoRate => r.gm.params.lfo_rate,
            ArpOsc1 => r.arp.params.vco1_level,
            ArpOsc2 => r.arp.params.vco2_level,
            ArpOsc3 => r.arp.params.vco3_level,
            ArpCutoff => r.arp.params.vcf_cutoff,
            ArpReso => r.arp.params.vcf_reso,
            ArpAttack => r.arp.params.adsr_a,
            ArpDecay => r.arp.params.adsr_d,
            ArpSustain => r.arp.params.adsr_s,
            ArpRelease => r.arp.params.adsr_r,
            ArpGlide => r.arp.params.glide,
            MixLevel(i) => r.level[i],
            MixMaster => r.master,
        }
    }

    fn set(self, r: &mut Rack, v: f32) {
        use KnobId::*;
        let p = r.sp_sel;
        match self {
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
            MixLevel(i) => r.level[i] = v,
            MixMaster => r.master = v,
        }
    }

    /// Normalized 0..1 position for the indicator.
    pub fn t(self, r: &Rack) -> f32 {
        let (lo, hi, log) = self.spec();
        let v = self.get(r).clamp(lo, hi);
        if log {
            (v.ln() - lo.ln()) / (hi.ln() - lo.ln())
        } else {
            (v - lo) / (hi - lo)
        }
    }

    pub fn set_t(self, r: &mut Rack, t: f32) {
        let (lo, hi, log) = self.spec();
        let t = t.clamp(0.0, 1.0);
        let v = if log { lo * (hi / lo).powf(t) } else { lo + (hi - lo) * t };
        self.set(r, v);
    }

    fn fmt_val(self, r: &Rack) -> String {
        use KnobId::*;
        let v = self.get(r);
        match self {
            SpCutoff | GmCutoff | ArpCutoff => format!("{v:.0} HZ"),
            SpRes => format!("Q{v:.2}"),
            GmReso | ArpReso => format!("{:.0}%", v * 100.0),
            SpLfoRate | GmLfoRate => format!("{v:.1} HZ"),
            SpLfoCut => format!("{v:+.0} ST"),
            SpLfoRes => format!("{v:+.2} Q"),
            GmAttack | GmDecay | GmRelease | ArpAttack | ArpDecay | ArpRelease => {
                if v < 0.1 {
                    format!("{v:.3} S")
                } else {
                    format!("{v:.2} S")
                }
            }
            GmGlide | ArpGlide => format!("{v:.2} S"),
            _ => format!("{:.0}%", v * 100.0),
        }
    }
}

/// What a click hit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Transport,
    BpmUp,
    BpmDown,
    SwingUp,
    SwingDown,
    Pad(usize),
    Step(usize),
    SpFilter,
    Knob(KnobId),
    GmGate(usize),
    ArpGate(usize),
    GmKey(u8),
    ArpKey(u8),
    GmFilterIn,
    ArpFilterIn,
    Mute(usize),
    SelectSynth(usize),
    None,
}

// Geometry helpers. Strips span x 8..1272.
fn rect(c: &mut Canvas<impl RenderTarget>, x: i32, y: i32, w: u32, h: u32, col: Color) {
    c.set_draw_color(col);
    let _ = c.fill_rect(Rect::new(x, y, w, h));
}

fn frame(c: &mut Canvas<impl RenderTarget>, x: i32, y: i32, w: u32, h: u32, col: Color) {
    c.set_draw_color(col);
    let _ = c.draw_rect(Rect::new(x, y, w, h));
}

fn text(c: &mut Canvas<impl RenderTarget>, x: i32, y: i32, s: &str, scale: u32, col: Color) {
    font::draw_text(c, x, y, s, scale, col);
}

/// 2px dot: the fill_rect primitive for lines and circles. This SDL
/// build's surface canvas ignores draw_point/draw_line (verified: zero
/// pixels), while fill_rect works, so all strokes rasterize to blocks.
fn dot(c: &mut Canvas<impl RenderTarget>, x: i32, y: i32, col: Color) {
    c.set_draw_color(col);
    let _ = c.fill_rect(Rect::new(x, y, 2, 2));
}

fn line(c: &mut Canvas<impl RenderTarget>, x0: i32, y0: i32, x1: i32, y1: i32, col: Color) {
    // Bresenham, rasterized as 2px blocks.
    let (mut x, mut y) = (x0, y0);
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        dot(c, x, y, col);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

fn circle(c: &mut Canvas<impl RenderTarget>, cx: i32, cy: i32, r: i32, col: Color) {
    // Midpoint circle, rasterized as 2px blocks.
    let mut x = 0;
    let mut y = r;
    let mut d = 1 - r;
    while x <= y {
        for (px, py) in [
            (cx + x, cy + y),
            (cx - x, cy + y),
            (cx + x, cy - y),
            (cx - x, cy - y),
            (cx + y, cy + x),
            (cx - y, cy + x),
            (cx + y, cy - x),
            (cx - y, cy - x),
        ] {
            dot(c, px, py, col);
        }
        if d < 0 {
            d += 2 * x + 3;
        } else {
            d += 2 * (x - y) + 5;
            y -= 1;
        }
        x += 1;
    }
}

fn knob<T: RenderTarget>(
    c: &mut Canvas<T>,
    x: i32,
    y: i32,
    id: KnobId,
    r: &Rack,
    ink: Color,
    sel: bool,
) {
    let cx = x + 40;
    let cy = y + 26;
    circle(c, cx, cy, 24, ink);
    if sel {
        circle(c, cx, cy, 28, SEL);
    }
    // Indicator: -135..+135 degrees from the normalized position.
    let t = id.t(r);
    let ang = (-135.0 + 270.0 * t as f64).to_radians();
    let x2 = cx + (ang.sin() * 20.0) as i32;
    let y2 = cy - (ang.cos() * 20.0) as i32;
    line(c, cx, cy, x2, y2, if sel { SEL } else { ink });
    text(c, x, y + 56, id.label(), 1, ink);
    text(c, x, y + 68, &id.fmt_val(r), 1, DIM);
}

/// White-key semitone offsets per octave, and which whites have a black
/// key above them (C D F G A).
const WHITE_ST: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
const HAS_BLACK: [bool; 7] = [true, true, false, true, true, true, false];

/// Keyboard geometry for a synth strip: 2 octaves from the base note.
fn key_rects(base: u8, x0: i32, y0: i32, w: i32, h: i32) -> (Vec<(Rect, u8)>, Vec<(Rect, u8)>) {
    let ww = w / 14;
    let mut whites = Vec::new();
    let mut blacks = Vec::new();
    for i in 0..14 {
        let oct = i / 7;
        let wi = i % 7;
        let note = base as i32 + oct as i32 * 12 + WHITE_ST[wi];
        whites.push((Rect::new(x0 + i as i32 * ww, y0, ww as u32 - 2, h as u32), note as u8));
        if HAS_BLACK[wi] && i < 13 {
            let bnote = note + 1;
            blacks.push((
                Rect::new(x0 + (i as i32 + 1) * ww - ww / 4, y0, (ww / 2) as u32, (h * 6 / 10) as u32),
                bnote as u8,
            ));
        }
    }
    (whites, blacks)
}

fn draw_keyboard<T: RenderTarget>(
    c: &mut Canvas<T>,
    base: u8,
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    held: Option<u8>,
) {
    let (whites, blacks) = key_rects(base, x0, y0, w, h);
    for (r, note) in &whites {
        let on = held == Some(*note);
        rect(c, r.x, r.y, r.width(), r.height(), if on { SEL } else { KEY_WHITE });
        frame(c, r.x, r.y, r.width(), r.height(), DIM);
    }
    for (r, note) in &blacks {
        let on = held == Some(*note);
        rect(c, r.x, r.y, r.width(), r.height(), if on { SEL } else { KEY_BLACK });
    }
}

fn draw_synth_strip<T: RenderTarget>(
    c: &mut Canvas<T>,
    r: &Rack,
    ui: &UiState,
    synth: usize,
    y0: i32,
    bg: Color,
    ink: Color,
    title: &str,
    knobs: &[KnobId],
    gate: &[bool; 16],
    filter_in: FilterIn,
) {
    rect(c, 8, y0, 1264, 236, bg);
    if r.selected == synth {
        frame(c, 8, y0, 1264, 236, SEL);
    }
    text(c, 20, y0 + 10, title, 2, ink);
    text(c, 320, y0 + 12, &format!("BASE {}", note_name(r.base_note(synth))), 1, ink);
    // Filter-in switch, right side.
    let fib = Rect::new(1080, y0 + 8, 176, 28);
    rect(c, fib.x, fib.y, fib.width(), fib.height(), BTN);
    text(c, fib.x + 10, fib.y + 10, &format!("FILTER IN {}", filter_in.label()), 1, INK);
    // Knobs row.
    for (i, id) in knobs.iter().enumerate() {
        let x = 20 + i as i32 * 122;
        let sel = ui.drag.map(|(d, _, _)| d) == Some(*id);
        knob(c, x, y0 + 44, *id, r, ink, sel);
    }
    // Keyboard row.
    let base = r.base_note(synth);
    let held = ui.mouse_note.filter(|(s, _)| *s == synth).map(|(_, n)| n);
    draw_keyboard(c, base, 20, y0 + 132, 700, 56, held);
    // Gate row + filter-in note.
    text(c, 760, y0 + 136, "GATE", 1, ink);
    for i in 0..16 {
        let x = 760 + i as i32 * 30;
        let y = y0 + 154;
        let on = gate[i];
        rect(c, x, y, 26, 26, if on { GATE_ON } else { BTN });
        if r.sp.playing && r.sp.last_step as usize % 16 == i {
            frame(c, x, y, 26, 26, SEL);
        }
    }
    text(c, 760, y0 + 190, "STEPS TRIGGER NOTES ON THE SP CLOCK", 1, DIM);
}

pub fn note_name(midi: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(midi % 12) as usize], midi / 12 - 1)
}

pub fn draw<T: RenderTarget>(c: &mut Canvas<T>, r: &Rack, ui: &UiState) {
    rect(c, 0, 0, W, H, BG);
    rect(c, 0, 0, 8, H, RAIL);
    rect(c, W as i32 - 8, 0, 8, H, RAIL);

    // Header: transport + clock.
    text(c, 20, 14, "RACK", 2, INK);
    let play = Rect::new(200, 8, 120, 32);
    rect(c, play.x, play.y, play.width(), play.height(), if r.sp.playing { GATE_ON } else { BTN });
    text(c, play.x + 22, play.y + 11, if r.sp.playing { "STOP" } else { "PLAY" }, 1, INK);
    text(c, 360, 16, "BPM", 1, DIM);
    rect(c, 400, 8, 28, 32, BTN);
    text(c, 408, 17, "-", 1, INK);
    text(c, 440, 14, &format!("{:.0}", r.sp.bpm), 2, INK);
    rect(c, 500, 8, 28, 32, BTN);
    text(c, 508, 17, "+", 1, INK);
    text(c, 560, 16, "SWING", 1, DIM);
    rect(c, 624, 8, 28, 32, BTN);
    text(c, 632, 17, "-", 1, INK);
    text(c, 664, 14, &format!("{:.0}%", r.sp.swing), 2, INK);
    rect(c, 724, 8, 28, 32, BTN);
    text(c, 732, 17, "+", 1, INK);
    text(c, 820, 16, "SPACE PLAY  1-8 PADS  LETTERS = KEYS  ESC QUIT", 1, DIM);

    // SP strip.
    rect(c, 8, SP_Y, 1264, 300, SP_BG);
    text(c, 20, SP_Y + 10, "SP-1200", 2, INK);
    text(c, 220, SP_Y + 12, &format!("PAD {}  FILTER {}", r.sp_sel + 1, r.sp.pads[r.sp_sel].filter_kind.abbrev()), 1, INK);
    // Pads.
    for i in 0..8 {
        let x = 20 + i as i32 * 128;
        let y = SP_Y + 40;
        let sel = r.sp_sel == i;
        rect(c, x, y, 120, 72, if sel { SEL } else { BTN });
        let name = r.sp.pads[i]
            .label
            .clone()
            .unwrap_or_else(|| crate::sp::dsp::DRUM_NAMES[i].to_string());
        text(c, x + 10, y + 12, &format!("{}", i + 1), 2, if sel { DARK_INK } else { INK });
        text(c, x + 10, y + 40, &name, 1, if sel { DARK_INK } else { INK });
    }
    // Steps for the selected pad.
    text(c, 20, SP_Y + 132, "STEPS", 1, INK);
    for i in 0..16 {
        let x = 120 + i as i32 * 70;
        let y = SP_Y + 124;
        let on = r.sp.patterns[r.sp_sel][i];
        rect(c, x, y, 64, 40, if on { STEP_ON } else { BTN });
        if r.sp.playing && r.sp.last_step as usize == i {
            frame(c, x, y, 64, 40, SEL);
        }
    }
    // Selected-pad edit row: filter kind button + knobs.
    let fb = Rect::new(20, SP_Y + 190, 140, 40);
    rect(c, fb.x, fb.y, fb.width(), fb.height(), BTN);
    text(c, fb.x + 10, fb.y + 13, &format!("FILTER {}", r.sp.pads[r.sp_sel].filter_kind.abbrev()), 1, INK);
    for (i, id) in [
        KnobId::SpCutoff,
        KnobId::SpRes,
        KnobId::SpLfoRate,
        KnobId::SpLfoCut,
        KnobId::SpLfoRes,
        KnobId::SpLevel,
    ]
    .iter()
    .enumerate()
    {
        let x = 200 + i as i32 * 168;
        let sel = ui.drag.map(|(d, _, _)| d) == Some(*id);
        knob(c, x, SP_Y + 186, *id, r, INK, sel);
    }
    text(c, 20, SP_Y + 268, "CLICK A PAD TO PLAY + SELECT  CLICK A STEP TO TOGGLE  DRAG KNOBS VERTICALLY", 1, DIM);

    // Synth strips.
    draw_synth_strip(
        c,
        r,
        ui,
        0,
        GM_Y,
        GM_BG,
        DARK_INK,
        "GRANDMOTHER",
        &[
            KnobId::GmOsc1,
            KnobId::GmOsc2,
            KnobId::GmCutoff,
            KnobId::GmReso,
            KnobId::GmAttack,
            KnobId::GmDecay,
            KnobId::GmSustain,
            KnobId::GmRelease,
            KnobId::GmGlide,
            KnobId::GmLfoRate,
        ],
        &r.gm_gate,
        r.gm_filter_in,
    );
    draw_synth_strip(
        c,
        r,
        ui,
        1,
        ARP_Y,
        ARP_BG,
        INK,
        "ARP 2600",
        &[
            KnobId::ArpOsc1,
            KnobId::ArpOsc2,
            KnobId::ArpOsc3,
            KnobId::ArpCutoff,
            KnobId::ArpReso,
            KnobId::ArpAttack,
            KnobId::ArpDecay,
            KnobId::ArpSustain,
            KnobId::ArpRelease,
            KnobId::ArpGlide,
        ],
        &r.arp_gate,
        r.arp_filter_in,
    );

    // Mixer strip.
    rect(c, 8, MIX_Y, 1264, 140, MIX_BG);
    text(c, 20, MIX_Y + 10, "MIXER", 2, INK);
    for ch in 0..3 {
        let x = 200 + ch as i32 * 300;
        let name = ["SP-1200", "GRANDMOTHER", "ARP 2600"][ch];
        text(c, x, MIX_Y + 14, name, 1, INK);
        let sel = ui.drag.map(|(d, _, _)| d) == Some(KnobId::MixLevel(ch));
        knob(c, x, MIX_Y + 36, KnobId::MixLevel(ch), r, INK, sel);
        let mb = Rect::new(x + 110, MIX_Y + 44, 80, 32);
        rect(c, mb.x, mb.y, mb.width(), mb.height(), if r.mute[ch] { MUTE_RED } else { BTN });
        text(c, mb.x + 18, mb.y + 11, if r.mute[ch] { "UNMUTE" } else { "MUTE" }, 1, INK);
        // Peak bar.
        let w = (r.peak[ch].clamp(0.0, 1.0) * 100.0) as u32;
        rect(c, x + 110, MIX_Y + 88, 100, 12, BTN);
        rect(c, x + 110, MIX_Y + 88, w, 12, if w > 90 { MUTE_RED } else { GATE_ON });
    }
    let sel = ui.drag.map(|(d, _, _)| d) == Some(KnobId::MixMaster);
    knob(c, 1120, MIX_Y + 36, KnobId::MixMaster, r, INK, sel);
    let w = (r.master_peak.clamp(0.0, 1.0) * 100.0) as u32;
    rect(c, 1120, MIX_Y + 124, 100, 12, BTN);
    rect(c, 1120, MIX_Y + 124, w, 12, if w > 90 { MUTE_RED } else { GATE_ON });
}

fn in_rect(x: i32, y: i32, rx: i32, ry: i32, w: i32, h: i32) -> bool {
    x >= rx && x < rx + w && y >= ry && y < ry + h
}

/// Knob cell rects, shared by draw and hit-test.
fn knob_cell(x: i32, y: i32) -> (i32, i32, i32, i32) {
    (x, y, 80, 84)
}

pub fn hit(r: &Rack, x: i32, y: i32) -> Action {
    // Header.
    if in_rect(x, y, 200, 8, 120, 32) {
        return Action::Transport;
    }
    if in_rect(x, y, 400, 8, 28, 32) {
        return Action::BpmDown;
    }
    if in_rect(x, y, 500, 8, 28, 32) {
        return Action::BpmUp;
    }
    if in_rect(x, y, 624, 8, 28, 32) {
        return Action::SwingDown;
    }
    if in_rect(x, y, 724, 8, 28, 32) {
        return Action::SwingUp;
    }
    // SP strip.
    if (SP_Y..SP_Y + 300).contains(&y) {
        for i in 0..8 {
            if in_rect(x, y, 20 + i as i32 * 128, SP_Y + 40, 120, 72) {
                return Action::Pad(i);
            }
        }
        for i in 0..16 {
            if in_rect(x, y, 120 + i as i32 * 70, SP_Y + 124, 64, 40) {
                return Action::Step(i);
            }
        }
        if in_rect(x, y, 20, SP_Y + 190, 140, 40) {
            return Action::SpFilter;
        }
        for (i, id) in [
            KnobId::SpCutoff,
            KnobId::SpRes,
            KnobId::SpLfoRate,
            KnobId::SpLfoCut,
            KnobId::SpLfoRes,
            KnobId::SpLevel,
        ]
        .iter()
        .enumerate()
        {
            let (rx, ry, w, h) = knob_cell(200 + i as i32 * 168, SP_Y + 186);
            if in_rect(x, y, rx, ry, w, h) {
                return Action::Knob(*id);
            }
        }
        return Action::None;
    }
    // Synth strips.
    for (synth, y0, knobs) in [
        (
            0usize,
            GM_Y,
            [
                KnobId::GmOsc1,
                KnobId::GmOsc2,
                KnobId::GmCutoff,
                KnobId::GmReso,
                KnobId::GmAttack,
                KnobId::GmDecay,
                KnobId::GmSustain,
                KnobId::GmRelease,
                KnobId::GmGlide,
                KnobId::GmLfoRate,
            ],
        ),
        (
            1usize,
            ARP_Y,
            [
                KnobId::ArpOsc1,
                KnobId::ArpOsc2,
                KnobId::ArpOsc3,
                KnobId::ArpCutoff,
                KnobId::ArpReso,
                KnobId::ArpAttack,
                KnobId::ArpDecay,
                KnobId::ArpSustain,
                KnobId::ArpRelease,
                KnobId::ArpGlide,
            ],
        ),
    ] {
        if (y0..y0 + 236).contains(&y) {
            // Filter-in button.
            if in_rect(x, y, 1080, y0 + 8, 176, 28) {
                return if synth == 0 { Action::GmFilterIn } else { Action::ArpFilterIn };
            }
            // Knobs.
            for (i, id) in knobs.iter().enumerate() {
                let (rx, ry, w, h) = knob_cell(20 + i as i32 * 122, y0 + 44);
                if in_rect(x, y, rx, ry, w, h) {
                    return Action::Knob(*id);
                }
            }
            // Keyboard: black keys first (they sit on top).
            let base = r.base_note(synth);
            let (whites, blacks) = key_rects(base, 20, y0 + 132, 700, 56);
            for (rc, note) in &blacks {
                if in_rect(x, y, rc.x, rc.y, rc.width() as i32, rc.height() as i32) {
                    return if synth == 0 { Action::GmKey(*note) } else { Action::ArpKey(*note) };
                }
            }
            for (rc, note) in &whites {
                if in_rect(x, y, rc.x, rc.y, rc.width() as i32, rc.height() as i32) {
                    return if synth == 0 { Action::GmKey(*note) } else { Action::ArpKey(*note) };
                }
            }
            // Gate row.
            for i in 0..16 {
                if in_rect(x, y, 760 + i as i32 * 30, y0 + 154, 26, 26) {
                    return if synth == 0 { Action::GmGate(i) } else { Action::ArpGate(i) };
                }
            }
            return Action::SelectSynth(synth);
        }
    }
    // Mixer strip.
    if (MIX_Y..MIX_Y + 140).contains(&y) {
        for ch in 0..3 {
            let x0 = 200 + ch as i32 * 300;
            let (rx, ry, w, h) = knob_cell(x0, MIX_Y + 36);
            if in_rect(x, y, rx, ry, w, h) {
                return Action::Knob(KnobId::MixLevel(ch));
            }
            if in_rect(x, y, x0 + 110, MIX_Y + 44, 80, 32) {
                return Action::Mute(ch);
            }
        }
        let (rx, ry, w, h) = knob_cell(1120, MIX_Y + 36);
        if in_rect(x, y, rx, ry, w, h) {
            return Action::Knob(KnobId::MixMaster);
        }
    }
    Action::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_spots_match_drawn_cells() {
        let r = Rack::new();
        assert_eq!(hit(&r, 210, 20), Action::Transport);
        assert_eq!(hit(&r, 30, SP_Y + 60), Action::Pad(0));
        assert_eq!(hit(&r, 130, SP_Y + 140), Action::Step(0));
        assert_eq!(hit(&r, 240, SP_Y + 210), Action::Knob(KnobId::SpCutoff));
        assert_eq!(hit(&r, 770, GM_Y + 165), Action::GmGate(0));
        assert_eq!(hit(&r, 1090, GM_Y + 20), Action::GmFilterIn);
        assert_eq!(hit(&r, 210, MIX_Y + 60), Action::Knob(KnobId::MixLevel(0)));
        assert_eq!(hit(&r, 1130, MIX_Y + 60), Action::Knob(KnobId::MixMaster));
        assert_eq!(hit(&r, 5, 5), Action::None);
    }

#[test]
fn primitives_draw_on_surface() {
    // Regression: this SDL build's surface canvas ignores draw_point /
    // draw_line, so panel strokes rasterize through fill_rect. A circle
    // east point and a line midpoint must land as lit pixels.
    use sdl2::pixels::PixelFormatEnum;
    use sdl2::surface::Surface;
    let surface = Surface::new(200, 200, PixelFormatEnum::RGB24).unwrap();
    let mut canvas = surface.into_canvas().unwrap();
    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    super::circle(&mut canvas, 100, 100, 24, Color::RGB(255, 255, 255));
    super::line(&mut canvas, 10, 10, 50, 10, Color::RGB(255, 255, 255));
    let surface = canvas.into_surface();
    let pitch = surface.pitch() as usize;
    surface.with_lock(|px| {
        let at = |x: usize, y: usize| px[y * pitch + x * 3];
        assert!(at(124, 100) > 200, "circle east point must be lit");
        assert!(at(30, 10) > 200, "line midpoint must be lit");
        assert_eq!(at(5, 5), 0, "background must stay dark");
    });
}

    #[test]
    fn knob_round_trips_through_t() {
        let mut r = Rack::new();
        let id = KnobId::GmCutoff;
        id.set_t(&mut r, 0.5);
        let v = id.get(&r);
        // 20 * (20000/20)^0.5 = 20 * sqrt(1000) ~= 632.
        assert!((v - 632.0).abs() < 2.0, "log knob midpoint: {v}");
        assert!((id.t(&r) - 0.5).abs() < 1e-4);
        id.set_t(&mut r, 0.0);
        assert_eq!(id.get(&r), 20.0);
        id.set_t(&mut r, 1.0);
        assert_eq!(id.get(&r), 20000.0);
    }
}

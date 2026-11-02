//! Rack panels: layout, drawing, hit-testing, knob specs.
//!
//! One fixed 1280x1196 layout, no scrolling: header (transport, save /
//! load), SP-1200 strip, two synth strips, sampler strip, mixer.
//! Knobs drag vertically; toggles click. All drawing is generic over
//! the canvas target so --shot can render to a surface with the same
//! code the window uses.

use crate::font;
use crate::rack::{CvIn, FilterIn, GateIn};
use crate::rt::View;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::{Canvas, RenderTarget};

pub const W: u32 = 1280;
pub const H: u32 = 1196;

// Layout (y bands).
const SP_Y: i32 = 48;
const GM_Y: i32 = 348;
const ARP_Y: i32 = 584;
const SAMP_Y: i32 = 820;
const MIX_Y: i32 = 1056;

// Back-view bands (same window; synth backs are taller for the jacks).
const BSP_Y: i32 = 48;
const BGM_Y: i32 = 348;
const BARP_Y: i32 = 648;
const BSAMP_Y: i32 = 948;

const BG: Color = Color::RGB(24, 24, 28);
const RAIL: Color = Color::RGB(10, 10, 12);
const SP_BG: Color = Color::RGB(58, 58, 64);
const GM_BG: Color = Color::RGB(232, 220, 192);
const ARP_BG: Color = Color::RGB(28, 28, 30);
const SAMP_BG: Color = Color::RGB(52, 64, 72);
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
    /// Back view visible (Tab / FLIP button toggles).
    pub flipped: bool,
    /// Output jack a cable is being dragged from, if any.
    pub cable_drag: Option<DragSrc>,
    /// Current mouse position while a cable drag is active.
    pub cable_to: Option<(i32, i32)>,
    /// Gate cell being pressed: (synth, step, start_y, start_offset,
    /// moved far enough to count as a note drag, not a toggle).
    pub gate_press: Option<(usize, usize, i32, i8, bool)>,
    /// Last save/load/sample status, shown in the header.
    pub status: String,
}

/// A patchable output jack on the back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutJack {
    Sp,
    Gm,
    Arp,
    Samp,
}

/// A patchable input jack on the back (a synth's filter input).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InJack {
    Gm,
    Arp,
}

/// A patchable gate output: a row's step pattern as triggers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateSrc {
    Sp,
    Gm,
    Arp,
}

/// A patchable gate input: a synth's gate-pattern source.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateDst {
    Gm,
    Arp,
}

/// A patchable CV output. Only the grandmother's LFO exists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CvSrc {
    Lfo,
}

/// A patchable CV input: a synth's cutoff-CV source.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CvDst {
    Gm,
    Arp,
}

/// Whatever a cable drag started from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragSrc {
    Audio(OutJack),
    Gate(GateSrc),
    Cv(CvSrc),
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
    // Back-panel knobs: deeper params the front macros skip.
    SpTune,
    GmNoise,
    GmEnvAmt,
    GmKbTrack,
    GmFAttack,
    GmFDecay,
    GmFSustain,
    GmFRelease,
    GmReverb,
    ArpNoise,
    ArpRing,
    ArpShRate,
    ArpLag,
    ArpVcaGain,
    ArpArAttack,
    ArpArRelease,
    ArpReverb,
    // Sampler strip knobs.
    SampLevel,
    SampTune,
    SampStart,
}

impl KnobId {
    /// (min, max, log scale).
    pub(crate) fn spec(self) -> (f32, f32, bool) {
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
            SpTune => (-24.0, 24.0, false),
            GmNoise | GmReverb | ArpNoise | ArpRing | ArpVcaGain | ArpReverb => (0.0, 1.0, false),
            GmEnvAmt => (-1.0, 1.0, false),
            GmKbTrack => (0.0, 1.0, false),
            GmFAttack | GmFDecay | GmFRelease | ArpArAttack | ArpArRelease => (0.001, 2.0, true),
            GmFSustain => (0.0, 1.0, false),
            ArpShRate => (0.1, 50.0, true),
            ArpLag => (0.0, 0.5, false),
            SampLevel | SampStart => (0.0, 1.0, false),
            SampTune => (-24.0, 24.0, false),
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
            SpTune => "TUNE",
            GmNoise => "NOISE",
            GmEnvAmt => "ENV AMT",
            GmKbTrack => "KB TRACK",
            GmFAttack => "F-ATK",
            GmFDecay => "F-DEC",
            GmFSustain => "F-SUS",
            GmFRelease => "F-REL",
            GmReverb => "REVERB",
            ArpNoise => "NOISE",
            ArpRing => "RING",
            ArpShRate => "SH RATE",
            ArpLag => "LAG",
            ArpVcaGain => "VCA GAIN",
            ArpArAttack => "AR-ATK",
            ArpArRelease => "AR-REL",
            ArpReverb => "REVERB",
            SampLevel => "LEVEL",
            SampTune => "TUNE",
            SampStart => "START",
        }
    }

    fn get(self, r: &View) -> f32 {
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
            SpTune => r.sp.pads[p].tune_st,
            GmNoise => r.gm.params.noise_level,
            GmEnvAmt => r.gm.params.env_amt,
            GmKbTrack => r.gm.params.kb_track,
            GmFAttack => r.gm.params.f_attack,
            GmFDecay => r.gm.params.f_decay,
            GmFSustain => r.gm.params.f_sustain,
            GmFRelease => r.gm.params.f_release,
            GmReverb => r.gm.params.reverb_mix,
            ArpNoise => r.arp.params.noise_level,
            ArpRing => r.arp.params.ring_level,
            ArpShRate => r.arp.params.sh_rate,
            ArpLag => r.arp.params.lag_time,
            ArpVcaGain => r.arp.params.vca_gain,
            ArpArAttack => r.arp.params.ar_a,
            ArpArRelease => r.arp.params.ar_r,
            ArpReverb => r.arp.params.rev_mix,
            SampLevel => r.samp.level,
            SampTune => r.samp.tune_st,
            SampStart => r.samp.start,
        }
    }

    /// Normalized 0..1 position for the indicator.
    pub fn t(self, r: &View) -> f32 {
        let (lo, hi, log) = self.spec();
        let v = self.get(r).clamp(lo, hi);
        if log {
            (v.ln() - lo.ln()) / (hi.ln() - lo.ln())
        } else {
            (v - lo) / (hi - lo)
        }
    }

    fn fmt_val(self, r: &View) -> String {
        use KnobId::*;
        let v = self.get(r);
        match self {
            SpCutoff | GmCutoff | ArpCutoff => format!("{v:.0} HZ"),
            SpRes => format!("Q{v:.2}"),
            GmReso | ArpReso => format!("{:.0}%", v * 100.0),
            SpLfoRate | GmLfoRate => format!("{v:.1} HZ"),
            SpLfoCut => format!("{v:+.0} ST"),
            SpLfoRes => format!("{v:+.2} Q"),
            GmAttack | GmDecay | GmRelease | ArpAttack | ArpDecay | ArpRelease | GmFAttack | GmFDecay | GmFRelease | ArpArAttack | ArpArRelease => {
                if v < 0.1 {
                    format!("{v:.3} S")
                } else {
                    format!("{v:.2} S")
                }
            }
            GmGlide | ArpGlide => format!("{v:.2} S"),
            SpTune | SampTune => format!("{v:+.0} ST"),
            GmEnvAmt => format!("{:+.0}%", v * 100.0),
            ArpShRate => format!("{v:.1} HZ"),
            ArpLag => format!("{v:.2} S"),
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
    GmKey(u8),
    ArpKey(u8),
    SampKey(u8),
    SampLoop,
    /// A gate cell press: the event loop decides toggle vs note drag.
    GateCell(usize, usize),
    GmFilterIn,
    ArpFilterIn,
    Mute(usize),
    SelectSynth(usize),
    SelectPad(usize),
    JackOut(OutJack),
    JackIn(InJack),
    GateOutJack(GateSrc),
    GateInJack(GateDst),
    CvOutJack(CvSrc),
    CvInJack(CvDst),
    Save,
    Load,
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
    r: &View,
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
    r: &View,
    ui: &UiState,
    synth: usize,
    y0: i32,
    bg: Color,
    ink: Color,
    title: &str,
    knobs: &[KnobId],
    gate: &[bool; 16],
    notes: &[i8; 16],
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
    // Gate row + filter-in note. A gated step shows its semitone
    // offset when it isn't the base note; drag a step vertically to
    // change it, wheel over it to nudge by one.
    text(c, 760, y0 + 136, "GATE", 1, ink);
    for i in 0..16 {
        let x = 760 + i as i32 * 30;
        let y = y0 + 154;
        let on = gate[i];
        rect(c, x, y, 26, 26, if on { GATE_ON } else { BTN });
        if on && notes[i] != 0 {
            text(c, x + 2, y + 9, &format!("{:+}", notes[i]), 1, ink);
        }
        if r.sp.playing && r.sp.last_step as usize % 16 == i {
            frame(c, x, y, 26, 26, SEL);
        }
    }
    text(c, 760, y0 + 190, "CLICK A STEP TO TOGGLE  DRAG IT TO CHANGE ITS NOTE", 1, DIM);
}

pub fn note_name(midi: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(midi % 12) as usize], midi / 12 - 1)
}

pub fn draw<T: RenderTarget>(c: &mut Canvas<T>, r: &View, ui: &UiState) {
    rect(c, 0, 0, W, H, BG);
    rect(c, 0, 0, 8, H, RAIL);
    rect(c, W as i32 - 8, 0, 8, H, RAIL);

    // Header: transport + clock + project.
    text(c, 20, 14, "RACK", 2, INK);
    let save = Rect::new(SAVE_RECT.0, SAVE_RECT.1, SAVE_RECT.2 as u32, SAVE_RECT.3 as u32);
    rect(c, save.x, save.y, save.width(), save.height(), BTN);
    text(c, save.x + 12, save.y + 11, "SAVE", 1, INK);
    let load = Rect::new(LOAD_RECT.0, LOAD_RECT.1, LOAD_RECT.2 as u32, LOAD_RECT.3 as u32);
    rect(c, load.x, load.y, load.width(), load.height(), BTN);
    text(c, load.x + 12, load.y + 11, "LOAD", 1, INK);
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
    let hint: String = if ui.status.is_empty() {
        "SPACE PLAY  TAB FLIP  ESC QUIT".to_string()
    } else {
        ui.status.chars().take(44).collect()
    };
    text(c, 820, 16, &hint, 1, if ui.status.is_empty() { DIM } else { SEL });
    let flip = Rect::new(FLIP_RECT.0, FLIP_RECT.1, FLIP_RECT.2 as u32, FLIP_RECT.3 as u32);
    rect(c, flip.x, flip.y, flip.width(), flip.height(), BTN);
    text(c, flip.x + 34, flip.y + 11, if ui.flipped { "FRONT" } else { "FLIP" }, 1, INK);
    if ui.flipped {
        draw_back(c, r, ui);
    } else {
        draw_front(c, r, ui);
    }
}

/// Save / Load button rects, shared by draw and the event loop.
pub const SAVE_RECT: (i32, i32, i32, i32) = (80, 8, 52, 32);
pub const LOAD_RECT: (i32, i32, i32, i32) = (136, 8, 52, 32);

/// Flip button rect, shared by draw and the event loop.
pub const FLIP_RECT: (i32, i32, i32, i32) = (1100, 8, 140, 32);

fn draw_front<T: RenderTarget>(c: &mut Canvas<T>, r: &View, ui: &UiState) {
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
        &r.gm_gate_notes,
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
        &r.arp_gate_notes,
        r.arp_filter_in,
    );

    // Sampler strip.
    rect(c, 8, SAMP_Y, 1264, 236, SAMP_BG);
    if r.selected == 2 {
        frame(c, 8, SAMP_Y, 1264, 236, SEL);
    }
    text(c, 20, SAMP_Y + 10, "SAMPLER", 2, INK);
    text(c, 220, SAMP_Y + 12, &format!("SAMPLE {}  ROOT C4", r.samp.name), 1, INK);
    for (i, id) in [KnobId::SampLevel, KnobId::SampTune, KnobId::SampStart].iter().enumerate() {
        let x = 20 + i as i32 * 122;
        let sel = ui.drag.map(|(d, _, _)| d) == Some(*id);
        knob(c, x, SAMP_Y + 44, *id, r, INK, sel);
    }
    let lb = Rect::new(1080, SAMP_Y + 8, 176, 28);
    rect(c, lb.x, lb.y, lb.width(), lb.height(), if r.samp.loop_ { GATE_ON } else { BTN });
    text(c, lb.x + 10, lb.y + 10, if r.samp.loop_ { "LOOP ON" } else { "LOOP OFF" }, 1, INK);
    let base = r.samp.base_note();
    let held = ui.mouse_note.filter(|(s, _)| *s == 2).map(|(_, n)| n);
    draw_keyboard(c, base, 20, SAMP_Y + 132, 700, 56, held);
    text(c, 760, SAMP_Y + 136, "DROP .WAV/.MP3 ON THE WINDOW OR --sample PATH", 1, DIM);
    text(c, 760, SAMP_Y + 190, "CLICK KEYS TO PLAY  LETTER KEYS WHEN SELECTED", 1, DIM);

    // Mixer strip.
    rect(c, 8, MIX_Y, 1264, 140, MIX_BG);
    text(c, 20, MIX_Y + 10, "MIXER", 2, INK);
    for ch in 0..4 {
        let x = 200 + ch as i32 * 220;
        let name = ["SP-1200", "GRANDMOTHER", "ARP 2600", "SAMPLER"][ch];
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

pub fn hit(r: &View, x: i32, y: i32) -> Action {
    // Header.
    if in_rect(x, y, SAVE_RECT.0, SAVE_RECT.1, SAVE_RECT.2, SAVE_RECT.3) {
        return Action::Save;
    }
    if in_rect(x, y, LOAD_RECT.0, LOAD_RECT.1, LOAD_RECT.2, LOAD_RECT.3) {
        return Action::Load;
    }
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
            // Gate row: press starts a toggle-or-note-drag.
            for i in 0..16 {
                if in_rect(x, y, 760 + i as i32 * 30, y0 + 154, 26, 26) {
                    return Action::GateCell(synth, i);
                }
            }
            return Action::SelectSynth(synth);
        }
    }
    // Sampler strip.
    if (SAMP_Y..SAMP_Y + 236).contains(&y) {
        if in_rect(x, y, 1080, SAMP_Y + 8, 176, 28) {
            return Action::SampLoop;
        }
        for (i, id) in [KnobId::SampLevel, KnobId::SampTune, KnobId::SampStart].iter().enumerate() {
            let (rx, ry, w, h) = knob_cell(20 + i as i32 * 122, SAMP_Y + 44);
            if in_rect(x, y, rx, ry, w, h) {
                return Action::Knob(*id);
            }
        }
        let base = r.samp.base_note();
        let (whites, blacks) = key_rects(base, 20, SAMP_Y + 132, 700, 56);
        for (rc, note) in &blacks {
            if in_rect(x, y, rc.x, rc.y, rc.width() as i32, rc.height() as i32) {
                return Action::SampKey(*note);
            }
        }
        for (rc, note) in &whites {
            if in_rect(x, y, rc.x, rc.y, rc.width() as i32, rc.height() as i32) {
                return Action::SampKey(*note);
            }
        }
        return Action::SelectSynth(2);
    }
    // Mixer strip.
    if (MIX_Y..MIX_Y + 140).contains(&y) {
        for ch in 0..4 {
            let x0 = 200 + ch as i32 * 220;
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

// ---- Back of the rack ----

const BACK_SP_BG: Color = Color::RGB(40, 40, 46);
const BACK_GM_BG: Color = Color::RGB(74, 66, 50);
const BACK_ARP_BG: Color = Color::RGB(20, 20, 22);
const BACK_SAMP_BG: Color = Color::RGB(38, 48, 54);
const CABLE_DIM: Color = Color::RGB(110, 110, 120);
const CABLE_HOT: Color = Color::RGB(255, 140, 40);
const CABLE_GATE: Color = Color::RGB(90, 200, 120);
const CABLE_CV: Color = Color::RGB(255, 200, 80);

/// Jack centers on the back, shared by draw, hit-test, and cables.
/// Back bands: SP 48..348, GM 348..648, ARP 648..948, SAMP 948..1056,
/// MIX 1056..1196. Jack rows sit at +70 (row 1) and +160 (row 2).
fn sp_out_pos() -> (i32, i32) {
    (60, BSP_Y + 70)
}
fn sp_gate_out_pos() -> (i32, i32) {
    (60, BSP_Y + 170)
}
fn gm_out_pos() -> (i32, i32) {
    (60, BGM_Y + 70)
}
fn gm_in_pos() -> (i32, i32) {
    (200, BGM_Y + 70)
}
fn gm_gate_out_pos() -> (i32, i32) {
    (340, BGM_Y + 70)
}
fn gm_gate_in_pos() -> (i32, i32) {
    (480, BGM_Y + 70)
}
fn gm_lfo_out_pos() -> (i32, i32) {
    (60, BGM_Y + 160)
}
fn gm_cv_in_pos() -> (i32, i32) {
    (200, BGM_Y + 160)
}
fn arp_out_pos() -> (i32, i32) {
    (60, BARP_Y + 70)
}
fn arp_in_pos() -> (i32, i32) {
    (200, BARP_Y + 70)
}
fn arp_gate_out_pos() -> (i32, i32) {
    (340, BARP_Y + 70)
}
fn arp_gate_in_pos() -> (i32, i32) {
    (480, BARP_Y + 70)
}
fn arp_cv_in_pos() -> (i32, i32) {
    (60, BARP_Y + 160)
}
fn samp_out_pos() -> (i32, i32) {
    (60, BSAMP_Y + 70)
}
fn mix_in_pos(ch: usize) -> (i32, i32) {
    (240 + ch as i32 * 240, MIX_Y + 70)
}
fn master_out_pos() -> (i32, i32) {
    (1150, MIX_Y + 70)
}

fn out_pos(j: OutJack) -> (i32, i32) {
    match j {
        OutJack::Sp => sp_out_pos(),
        OutJack::Gm => gm_out_pos(),
        OutJack::Arp => arp_out_pos(),
        OutJack::Samp => samp_out_pos(),
    }
}

fn gate_out_pos(j: GateSrc) -> (i32, i32) {
    match j {
        GateSrc::Sp => sp_gate_out_pos(),
        GateSrc::Gm => gm_gate_out_pos(),
        GateSrc::Arp => arp_gate_out_pos(),
    }
}

fn in_pos(j: InJack) -> (i32, i32) {
    match j {
        InJack::Gm => gm_in_pos(),
        InJack::Arp => arp_in_pos(),
    }
}

fn gate_in_pos(j: GateDst) -> (i32, i32) {
    match j {
        GateDst::Gm => gm_gate_in_pos(),
        GateDst::Arp => arp_gate_in_pos(),
    }
}

fn cv_out_pos() -> (i32, i32) {
    gm_lfo_out_pos()
}

fn cv_in_pos(j: CvDst) -> (i32, i32) {
    match j {
        CvDst::Gm => gm_cv_in_pos(),
        CvDst::Arp => arp_cv_in_pos(),
    }
}

fn drag_pos(d: DragSrc) -> (i32, i32) {
    match d {
        DragSrc::Audio(j) => out_pos(j),
        DragSrc::Gate(j) => gate_out_pos(j),
        DragSrc::Cv(_) => cv_out_pos(),
    }
}

fn draw_jack<T: RenderTarget>(c: &mut Canvas<T>, cx: i32, cy: i32, label: &str, ink: Color, hot: bool) {
    circle(c, cx, cy, 16, ink);
    rect(c, cx - 7, cy - 7, 14, 14, Color::RGB(8, 8, 10));
    if hot {
        circle(c, cx, cy, 20, SEL);
    }
    text(c, cx - 20, cy + 26, label, 1, ink);
}

/// Cubic cable with vertical tangents: hangs like a patch cord.
fn bezier<T: RenderTarget>(c: &mut Canvas<T>, x0: i32, y0: i32, x1: i32, y1: i32, col: Color) {
    let dy = (y1 - y0) as f64;
    let (x0f, y0f, x1f, y1f) = (x0 as f64, y0 as f64, x1 as f64, y1 as f64);
    let c1 = (x0f, y0f + dy * 0.5);
    let c2 = (x1f, y1f - dy * 0.5);
    let dist = ((x1f - x0f).powi(2) + (y1f - y0f).powi(2)).sqrt();
    let n = ((dist / 3.0) as usize).clamp(60, 400);
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let mt = 1.0 - t;
        let x = mt.powi(3) * x0f + 3.0 * mt * mt * t * c1.0 + 3.0 * mt * t * t * c2.0 + t.powi(3) * x1f;
        let y = mt.powi(3) * y0f + 3.0 * mt * mt * t * c1.1 + 3.0 * mt * t * t * c2.1 + t.powi(3) * y1f;
        dot(c, x as i32, y as i32, col);
    }
}

fn draw_synth_back<T: RenderTarget>(
    c: &mut Canvas<T>,
    r: &View,
    ui: &UiState,
    synth: usize,
    y0: i32,
    bg: Color,
    title: &str,
    knobs: &[KnobId],
    filter_in: FilterIn,
    gate_in: GateIn,
    cv_in: CvIn,
    has_lfo: bool,
) {
    rect(c, 8, y0, 1264, 300, bg);
    if r.selected == synth {
        frame(c, 8, y0, 1264, 300, SEL);
    }
    text(c, 20, y0 + 10, title, 2, INK);
    let oj = if synth == 0 { OutJack::Gm } else { OutJack::Arp };
    let gj = if synth == 0 { GateSrc::Gm } else { GateSrc::Arp };
    let gd = if synth == 0 { GateDst::Gm } else { GateDst::Arp };
    let ij = if synth == 0 { InJack::Gm } else { InJack::Arp };
    let cd = if synth == 0 { CvDst::Gm } else { CvDst::Arp };
    let op = out_pos(oj);
    let ip = in_pos(ij);
    let gop = gate_out_pos(gj);
    let gip = gate_in_pos(gd);
    draw_jack(c, op.0, op.1, "OUT", INK, ui.cable_drag == Some(DragSrc::Audio(oj)));
    draw_jack(c, ip.0, ip.1, "FILTER IN", INK, filter_in != FilterIn::Off);
    let fstate = if filter_in == FilterIn::Off {
        "OPEN".to_string()
    } else {
        format!("PATCHED: {}", filter_in.label())
    };
    text(c, 260, y0 + 64, &fstate, 1, INK);
    draw_jack(c, gop.0, gop.1, "GATE OUT", INK, ui.cable_drag == Some(DragSrc::Gate(gj)));
    draw_jack(c, gip.0, gip.1, "GATE IN", INK, gate_in != GateIn::Off);
    let gstate = if gate_in == GateIn::Off {
        "OWN ROW".to_string()
    } else {
        format!("PATCHED: {}", gate_in.label())
    };
    text(c, 540, y0 + 64, &gstate, 1, INK);
    // Row 2: CV. Only grandmother has an LFO to offer.
    if has_lfo {
        draw_jack(
            c,
            gm_lfo_out_pos().0,
            gm_lfo_out_pos().1,
            "LFO OUT",
            INK,
            ui.cable_drag == Some(DragSrc::Cv(CvSrc::Lfo)),
        );
    }
    let cp = cv_in_pos(cd);
    let cs = if synth == 0 { 260 } else { 120 };
    draw_jack(c, cp.0, cp.1, "CUTOFF IN", INK, cv_in != CvIn::Off);
    let cstate = if cv_in == CvIn::Off { "OPEN".to_string() } else { format!("PATCHED: {}", cv_in.label()) };
    text(c, cs, y0 + 154, &cstate, 1, INK);
    for (i, id) in knobs.iter().enumerate() {
        let x = 20 + i as i32 * 122;
        let sel = ui.drag.map(|(d, _, _)| d) == Some(*id);
        knob(c, x, y0 + 210, *id, r, INK, sel);
    }
}

fn draw_back<T: RenderTarget>(c: &mut Canvas<T>, r: &View, ui: &UiState) {
    // SP back: OUT + GATE OUT jacks, pad select, tune.
    rect(c, 8, BSP_Y, 1264, 300, BACK_SP_BG);
    text(c, 20, BSP_Y + 10, "SP-1200 -- BACK", 2, INK);
    draw_jack(c, sp_out_pos().0, sp_out_pos().1, "OUT", INK, ui.cable_drag == Some(DragSrc::Audio(OutJack::Sp)));
    draw_jack(
        c,
        sp_gate_out_pos().0,
        sp_gate_out_pos().1,
        "GATE OUT",
        INK,
        ui.cable_drag == Some(DragSrc::Gate(GateSrc::Sp)),
    );
    for i in 0..8 {
        let x = 200 + i as i32 * 110;
        rect(c, x, BSP_Y + 50, 100, 40, if r.sp_sel == i { SEL } else { BTN });
        text(c, x + 10, BSP_Y + 63, &format!("PAD {}", i + 1), 1, if r.sp_sel == i { DARK_INK } else { INK });
    }
    let sel = ui.drag.map(|(d, _, _)| d) == Some(KnobId::SpTune);
    knob(c, 1100, BSP_Y + 44, KnobId::SpTune, r, INK, sel);
    text(c, 20, BSP_Y + 220, "DRAG AN OUT TO AN INPUT -- CLICK A PATCHED INPUT TO UNPATCH", 1, DIM);
    text(c, 20, BSP_Y + 250, "GATE OUT PLAYS THE SELECTED PAD'S ROW", 1, DIM);

    draw_synth_back(
        c,
        r,
        ui,
        0,
        BGM_Y,
        BACK_GM_BG,
        "GRANDMOTHER -- BACK",
        &[
            KnobId::GmNoise,
            KnobId::GmEnvAmt,
            KnobId::GmKbTrack,
            KnobId::GmFAttack,
            KnobId::GmFDecay,
            KnobId::GmFSustain,
            KnobId::GmFRelease,
            KnobId::GmReverb,
        ],
        r.gm_filter_in,
        r.gm_gate_in,
        r.gm_cv_in,
        true,
    );
    draw_synth_back(
        c,
        r,
        ui,
        1,
        BARP_Y,
        BACK_ARP_BG,
        "ARP 2600 -- BACK",
        &[
            KnobId::ArpNoise,
            KnobId::ArpRing,
            KnobId::ArpShRate,
            KnobId::ArpLag,
            KnobId::ArpVcaGain,
            KnobId::ArpArAttack,
            KnobId::ArpArRelease,
            KnobId::ArpReverb,
        ],
        r.arp_filter_in,
        r.arp_gate_in,
        r.arp_cv_in,
        false,
    );

    // Sampler back: OUT jack only.
    rect(c, 8, BSAMP_Y, 1264, 108, BACK_SAMP_BG);
    text(c, 20, BSAMP_Y + 10, "SAMPLER -- BACK", 2, INK);
    draw_jack(c, samp_out_pos().0, samp_out_pos().1, "OUT", INK, ui.cable_drag == Some(DragSrc::Audio(OutJack::Samp)));
    text(c, 120, BSAMP_Y + 64, &format!("SAMPLE {}", r.samp.name), 1, INK);

    // Mixer back: fixed inputs + master out.
    rect(c, 8, MIX_Y, 1264, 140, MIX_BG);
    text(c, 20, MIX_Y + 10, "MIXER -- BACK", 2, INK);
    for ch in 0..4 {
        let (jx, jy) = mix_in_pos(ch);
        draw_jack(c, jx, jy, ["SP IN", "GM IN", "ARP IN", "SAMP IN"][ch], INK, false);
    }
    draw_jack(c, master_out_pos().0, master_out_pos().1, "MASTER OUT", INK, false);
    text(c, 20, MIX_Y + 116, "CHANNEL INPUTS ARE FIXED", 1, DIM);

    // Cables over everything: fixed mixer runs dim, patches by kind
    // (audio orange, gate green, CV yellow).
    let (s, g, a, p) = (sp_out_pos(), gm_out_pos(), arp_out_pos(), samp_out_pos());
    bezier(c, s.0, s.1, mix_in_pos(0).0, mix_in_pos(0).1, CABLE_DIM);
    bezier(c, g.0, g.1, mix_in_pos(1).0, mix_in_pos(1).1, CABLE_DIM);
    bezier(c, a.0, a.1, mix_in_pos(2).0, mix_in_pos(2).1, CABLE_DIM);
    bezier(c, p.0, p.1, mix_in_pos(3).0, mix_in_pos(3).1, CABLE_DIM);
    let filt = |src: FilterIn| -> Option<(i32, i32)> {
        match src {
            FilterIn::Sp => Some(s),
            FilterIn::Gm => Some(g),
            FilterIn::Arp => Some(a),
            FilterIn::Samp => Some(p),
            FilterIn::Off => None,
        }
    };
    if let Some(src) = filt(r.gm_filter_in) {
        bezier(c, src.0, src.1, gm_in_pos().0, gm_in_pos().1, CABLE_HOT);
    }
    if let Some(src) = filt(r.arp_filter_in) {
        bezier(c, src.0, src.1, arp_in_pos().0, arp_in_pos().1, CABLE_HOT);
    }
    let gate = |src: GateIn| -> Option<(i32, i32)> {
        match src {
            GateIn::Sp => Some(sp_gate_out_pos()),
            GateIn::Gm => Some(gm_gate_out_pos()),
            GateIn::Arp => Some(arp_gate_out_pos()),
            GateIn::Off => None,
        }
    };
    if let Some(src) = gate(r.gm_gate_in) {
        bezier(c, src.0, src.1, gm_gate_in_pos().0, gm_gate_in_pos().1, CABLE_GATE);
    }
    if let Some(src) = gate(r.arp_gate_in) {
        bezier(c, src.0, src.1, arp_gate_in_pos().0, arp_gate_in_pos().1, CABLE_GATE);
    }
    if r.gm_cv_in == CvIn::Lfo {
        bezier(c, gm_lfo_out_pos().0, gm_lfo_out_pos().1, gm_cv_in_pos().0, gm_cv_in_pos().1, CABLE_CV);
    }
    if r.arp_cv_in == CvIn::Lfo {
        bezier(c, gm_lfo_out_pos().0, gm_lfo_out_pos().1, arp_cv_in_pos().0, arp_cv_in_pos().1, CABLE_CV);
    }
    if let (Some(src), Some((mx, my))) = (ui.cable_drag, ui.cable_to) {
        let (sx, sy) = drag_pos(src);
        let col = match src {
            DragSrc::Audio(_) => CABLE_HOT,
            DragSrc::Gate(_) => CABLE_GATE,
            DragSrc::Cv(_) => CABLE_CV,
        };
        bezier(c, sx, sy, mx, my, col);
    }
}

/// Patch a dragged cable into a synth's filter input. A synth never
/// takes itself; anything else patches. Returns false when rejected.
/// Map a dragged audio cable to the filter-in it patches, or
/// None for an illegal patch (a strip never takes itself).
/// Pure: the caller sends the result to the audio thread.
pub fn cable_filter(src: OutJack, dst: InJack) -> Option<FilterIn> {
    match (dst, src) {
        (InJack::Gm, OutJack::Sp) => Some(FilterIn::Sp),
        (InJack::Gm, OutJack::Arp) => Some(FilterIn::Arp),
        (InJack::Gm, OutJack::Samp) => Some(FilterIn::Samp),
        (InJack::Arp, OutJack::Sp) => Some(FilterIn::Sp),
        (InJack::Arp, OutJack::Gm) => Some(FilterIn::Gm),
        (InJack::Arp, OutJack::Samp) => Some(FilterIn::Samp),
        _ => None,
    }
}

/// Map a dragged gate cable to the gate-in it patches, or None
/// for an illegal patch. The strip keeps its own per-step notes
/// and borrows only the pattern. Pure: see cable_filter.
pub fn gate_filter(src: GateSrc, dst: GateDst) -> Option<GateIn> {
    match (dst, src) {
        (GateDst::Gm, GateSrc::Sp) => Some(GateIn::Sp),
        (GateDst::Gm, GateSrc::Arp) => Some(GateIn::Arp),
        (GateDst::Arp, GateSrc::Sp) => Some(GateIn::Sp),
        (GateDst::Arp, GateSrc::Gm) => Some(GateIn::Gm),
        _ => None,
    }
}

/// Map a dragged CV cable to the cutoff input it patches, or
/// None unless the source is the grandmother's LFO. Pure: see
/// cable_filter.
pub fn cv_filter(src: CvSrc, dst: CvDst) -> Option<CvIn> {
    if src != CvSrc::Lfo {
        return None;
    }
    match dst {
        CvDst::Gm | CvDst::Arp => Some(CvIn::Lfo),
    }
}

fn jack_hit(x: i32, y: i32, cx: i32, cy: i32) -> bool {
    (x - cx).abs() <= 18 && (y - cy).abs() <= 18
}

/// Hit-test for the back view. Mixer jacks are fixed, so they are not
/// actions; synth strip background selects the synth.
pub fn hit_back(x: i32, y: i32) -> Action {
    // Jacks first (all rows, all strips).
    if jack_hit(x, y, sp_out_pos().0, sp_out_pos().1) {
        return Action::JackOut(OutJack::Sp);
    }
    if jack_hit(x, y, sp_gate_out_pos().0, sp_gate_out_pos().1) {
        return Action::GateOutJack(GateSrc::Sp);
    }
    if jack_hit(x, y, gm_out_pos().0, gm_out_pos().1) {
        return Action::JackOut(OutJack::Gm);
    }
    if jack_hit(x, y, gm_in_pos().0, gm_in_pos().1) {
        return Action::JackIn(InJack::Gm);
    }
    if jack_hit(x, y, gm_gate_out_pos().0, gm_gate_out_pos().1) {
        return Action::GateOutJack(GateSrc::Gm);
    }
    if jack_hit(x, y, gm_gate_in_pos().0, gm_gate_in_pos().1) {
        return Action::GateInJack(GateDst::Gm);
    }
    if jack_hit(x, y, gm_lfo_out_pos().0, gm_lfo_out_pos().1) {
        return Action::CvOutJack(CvSrc::Lfo);
    }
    if jack_hit(x, y, gm_cv_in_pos().0, gm_cv_in_pos().1) {
        return Action::CvInJack(CvDst::Gm);
    }
    if jack_hit(x, y, arp_out_pos().0, arp_out_pos().1) {
        return Action::JackOut(OutJack::Arp);
    }
    if jack_hit(x, y, arp_in_pos().0, arp_in_pos().1) {
        return Action::JackIn(InJack::Arp);
    }
    if jack_hit(x, y, arp_gate_out_pos().0, arp_gate_out_pos().1) {
        return Action::GateOutJack(GateSrc::Arp);
    }
    if jack_hit(x, y, arp_gate_in_pos().0, arp_gate_in_pos().1) {
        return Action::GateInJack(GateDst::Arp);
    }
    if jack_hit(x, y, arp_cv_in_pos().0, arp_cv_in_pos().1) {
        return Action::CvInJack(CvDst::Arp);
    }
    if jack_hit(x, y, samp_out_pos().0, samp_out_pos().1) {
        return Action::JackOut(OutJack::Samp);
    }
    // SP back: pad select + tune.
    if (BSP_Y..BSP_Y + 300).contains(&y) {
        for i in 0..8 {
            if in_rect(x, y, 200 + i as i32 * 110, BSP_Y + 50, 100, 40) {
                return Action::SelectPad(i);
            }
        }
        let (rx, ry, w, h) = knob_cell(1100, BSP_Y + 44);
        if in_rect(x, y, rx, ry, w, h) {
            return Action::Knob(KnobId::SpTune);
        }
        return Action::None;
    }
    // Synth backs: knobs, then background selects.
    for (synth, y0, knobs) in [
        (
            0usize,
            BGM_Y,
            [
                KnobId::GmNoise,
                KnobId::GmEnvAmt,
                KnobId::GmKbTrack,
                KnobId::GmFAttack,
                KnobId::GmFDecay,
                KnobId::GmFSustain,
                KnobId::GmFRelease,
                KnobId::GmReverb,
            ],
        ),
        (
            1usize,
            BARP_Y,
            [
                KnobId::ArpNoise,
                KnobId::ArpRing,
                KnobId::ArpShRate,
                KnobId::ArpLag,
                KnobId::ArpVcaGain,
                KnobId::ArpArAttack,
                KnobId::ArpArRelease,
                KnobId::ArpReverb,
            ],
        ),
    ] {
        if (y0..y0 + 300).contains(&y) {
            for (i, id) in knobs.iter().enumerate() {
                let (rx, ry, w, h) = knob_cell(20 + i as i32 * 122, y0 + 210);
                if in_rect(x, y, rx, ry, w, h) {
                    return Action::Knob(*id);
                }
            }
            return Action::SelectSynth(synth);
        }
    }
    // Sampler back: background selects the sampler.
    if (BSAMP_Y..BSAMP_Y + 108).contains(&y) {
        return Action::SelectSynth(2);
    }
    Action::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rack::Rack;

    #[test]
    fn hit_spots_match_drawn_cells() {
        let v = View::from_rack(&Rack::new());
        assert_eq!(hit(&v, 210, 20), Action::Transport);
        assert_eq!(hit(&v, 30, SP_Y + 60), Action::Pad(0));
        assert_eq!(hit(&v, 130, SP_Y + 140), Action::Step(0));
        assert_eq!(hit(&v, 240, SP_Y + 210), Action::Knob(KnobId::SpCutoff));
        assert_eq!(hit(&v, 770, GM_Y + 165), Action::GateCell(0, 0));
        assert_eq!(hit(&v, 1090, GM_Y + 20), Action::GmFilterIn);
        assert_eq!(hit(&v, 210, MIX_Y + 60), Action::Knob(KnobId::MixLevel(0)));
        assert_eq!(hit(&v, 870, MIX_Y + 60), Action::Knob(KnobId::MixLevel(3)));
        assert_eq!(hit(&v, 1130, MIX_Y + 60), Action::Knob(KnobId::MixMaster));
        assert_eq!(hit(&v, 30, SAMP_Y + 160), Action::SampKey(48));
        assert_eq!(hit(&v, 5, 5), Action::None);
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
    fn back_jacks_hit_and_cables_patch() {
        assert_eq!(hit_back(60, BSP_Y + 70), Action::JackOut(OutJack::Sp));
        assert_eq!(hit_back(60, BSP_Y + 170), Action::GateOutJack(GateSrc::Sp));
        assert_eq!(hit_back(200, BGM_Y + 70), Action::JackIn(InJack::Gm));
        assert_eq!(hit_back(480, BGM_Y + 70), Action::GateInJack(GateDst::Gm));
        assert_eq!(hit_back(60, BGM_Y + 160), Action::CvOutJack(CvSrc::Lfo));
        assert_eq!(hit_back(200, BARP_Y + 70), Action::JackIn(InJack::Arp));
        assert_eq!(hit_back(60, BSAMP_Y + 70), Action::JackOut(OutJack::Samp));
        assert_eq!(hit_back(60, BGM_Y + 250), Action::Knob(KnobId::GmNoise));
        assert_eq!(hit_back(360, BSP_Y + 70), Action::SelectPad(1));
        // Cable mappers are pure: they report the patch, the audio
        // thread applies it (covered end to end in rt tests).
        assert_eq!(cable_filter(OutJack::Sp, InJack::Gm), Some(FilterIn::Sp));
        assert_eq!(cable_filter(OutJack::Arp, InJack::Gm), Some(FilterIn::Arp));
        assert_eq!(cable_filter(OutJack::Samp, InJack::Gm), Some(FilterIn::Samp));
        assert_eq!(cable_filter(OutJack::Gm, InJack::Gm), None, "self-patch is illegal");
        assert_eq!(cable_filter(OutJack::Gm, InJack::Arp), Some(FilterIn::Gm));
        assert_eq!(cable_filter(OutJack::Arp, InJack::Arp), None);
        assert_eq!(gate_filter(GateSrc::Sp, GateDst::Gm), Some(GateIn::Sp));
        assert_eq!(gate_filter(GateSrc::Gm, GateDst::Gm), None, "self gate-patch is illegal");
        assert_eq!(gate_filter(GateSrc::Gm, GateDst::Arp), Some(GateIn::Gm));
        assert_eq!(cv_filter(CvSrc::Lfo, CvDst::Arp), Some(CvIn::Lfo));
    }

    #[test]
    fn front_view_draws_to_surface() {
        use sdl2::pixels::PixelFormatEnum;
        use sdl2::surface::Surface;
        let mut r = Rack::new();
        r.gm_gate[0] = true;
        r.gm_gate_notes[0] = 7;
        r.samp.note_on(60);
        let v = View::from_rack(&r);
        let ui = UiState::default();
        let surface = Surface::new(W, H, PixelFormatEnum::RGB24).unwrap();
        let mut canvas = surface.into_canvas().unwrap();
        super::draw(&mut canvas, &v, &ui);
    }

    #[test]
    fn back_view_draws_to_surface() {
        use sdl2::pixels::PixelFormatEnum;
        use sdl2::surface::Surface;
        let mut r = Rack::new();
        r.gm_filter_in = FilterIn::Samp;
        r.arp_filter_in = FilterIn::Gm;
        r.gm_gate_in = GateIn::Sp;
        r.arp_cv_in = CvIn::Lfo;
        let v = View::from_rack(&r);
        let ui = UiState { flipped: true, cable_drag: Some(DragSrc::Audio(OutJack::Arp)), cable_to: Some((500, 500)), ..UiState::default() };
        let surface = Surface::new(W, H, PixelFormatEnum::RGB24).unwrap();
        let mut canvas = surface.into_canvas().unwrap();
        super::draw(&mut canvas, &v, &ui);
    }
}

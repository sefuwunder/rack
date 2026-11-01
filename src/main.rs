//! Rack player v1: one window, three engines, one mixer.
//!
//! Reason-style virtual rack, compact edition: an SP-1200 strip (pads,
//! steps, per-pad filter + LFO), two synth strips (macro knobs, keyboard,
//! gate row, filter-in switch), and a mixer. The SP transport is the
//! rack clock. No save/load yet; the rack always starts from the factory
//! state of the three engines.

mod arp;
mod font;
mod gm;
mod panel;
mod rack;
mod sp;

use panel::{Action, UiState};
use rack::Rack;
use sdl2::audio::{AudioCallback, AudioSpecDesired};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::surface::Surface;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct RackCallback {
    rack: Arc<Mutex<Rack>>,
}

impl AudioCallback for RackCallback {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        match self.rack.lock() {
            Ok(mut r) => r.render_block(out),
            Err(_) => {
                for x in out.iter_mut() {
                    *x = 0.0;
                }
            }
        }
    }
}

/// Computer-key semitone for the selected synth. Letters only: digits
/// 1-8 always play SP pads, so the Q-row black keys that would need
/// number keys are skipped (same layout as grandmother's keyboard
/// minus its number row).
fn key_semi(k: Keycode) -> Option<i32> {
    Some(match k {
        Keycode::Z => 0,
        Keycode::S => 1,
        Keycode::X => 2,
        Keycode::D => 3,
        Keycode::C => 4,
        Keycode::V => 5,
        Keycode::G => 6,
        Keycode::B => 7,
        Keycode::H => 8,
        Keycode::N => 9,
        Keycode::J => 10,
        Keycode::M => 11,
        Keycode::Q => 12,
        Keycode::W => 14,
        Keycode::E => 16,
        Keycode::R => 17,
        Keycode::T => 19,
        Keycode::Y => 21,
        Keycode::U => 23,
        Keycode::I => 24,
        Keycode::O => 26,
        Keycode::P => 28,
        _ => return None,
    })
}

fn apply_action(rack: &mut Rack, ui: &mut UiState, a: Action, my: i32) {
    match a {
        Action::Transport => rack.toggle_transport(),
        Action::BpmUp => {
            rack.sp.bpm = (rack.sp.bpm + 1.0).clamp(sp::engine::BPM_MIN, sp::engine::BPM_MAX);
        }
        Action::BpmDown => {
            rack.sp.bpm = (rack.sp.bpm - 1.0).clamp(sp::engine::BPM_MIN, sp::engine::BPM_MAX);
        }
        Action::SwingUp => rack.sp.swing = (rack.sp.swing + 1.0).clamp(50.0, 75.0),
        Action::SwingDown => rack.sp.swing = (rack.sp.swing - 1.0).clamp(50.0, 75.0),
        Action::Pad(i) => {
            rack.sp_sel = i;
            rack.sp.trigger(i);
        }
        Action::Step(i) => {
            let p = rack.sp_sel;
            rack.sp.patterns[p][i] = !rack.sp.patterns[p][i];
        }
        Action::SpFilter => {
            let p = rack.sp_sel;
            let k = rack.sp.pads[p].filter_kind.cycle();
            let c = rack.sp.pads[p].filter_cutoff;
            rack.sp.set_filter(p, k, c);
        }
        Action::Knob(id) => {
            ui.drag = Some((id, my, id.t(rack)));
        }
        Action::GmGate(i) => rack.gm_gate[i] = !rack.gm_gate[i],
        Action::ArpGate(i) => rack.arp_gate[i] = !rack.arp_gate[i],
        Action::GmFilterIn => rack.cycle_filter_in(0),
        Action::ArpFilterIn => rack.cycle_filter_in(1),
        Action::GmKey(note) => {
            rack.selected = 0;
            rack.gm.voice.note_on(note);
            ui.mouse_note = Some((0, note));
        }
        Action::ArpKey(note) => {
            rack.selected = 1;
            rack.arp.note_on(note as f32);
            ui.mouse_note = Some((1, note));
        }
        Action::Mute(ch) => rack.mute[ch] = !rack.mute[ch],
        Action::SelectSynth(s) => rack.selected = s,
        Action::None => {}
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Headless smoke: render blocks with a pad + two notes sounding.
    if args.iter().any(|a| a == "--smoke") {
        let secs: f32 = args
            .iter()
            .position(|a| a == "--smoke")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(3.0);
        let mut r = Rack::new();
        r.sp.trigger(0);
        r.gm.voice.note_on(48);
        r.arp.note_on(48.0);
        let blocks = (secs * 44100.0 / 1024.0) as usize;
        let mut peak = 0.0f32;
        for _ in 0..blocks {
            let mut blk = vec![0.0f32; 1024];
            r.render_block(&mut blk);
            for v in blk.iter() {
                assert!(v.is_finite());
                peak = peak.max(v.abs());
            }
        }
        println!("RACK SMOKE {secs}s PEAK {peak:.3}");
        if peak <= 0.0 {
            eprintln!("SMOKE FAILED: silent output");
            std::process::exit(1);
        }
        return;
    }

    let sdl = sdl2::init().expect("sdl init");
    let video = sdl.video().expect("video");

    // --shot: draw the rack to a BMP without opening a window.
    if let Some(i) = args.iter().position(|a| a == "--shot") {
        let path = args.get(i + 1).cloned().unwrap_or_else(|| "rack-shot.bmp".to_string());
        let r = Rack::new();
        let ui = UiState::default();
        let surface =
            Surface::new(panel::W, panel::H, PixelFormatEnum::RGB24).expect("surface");
        let mut canvas = surface.into_canvas().expect("surface canvas");
        panel::draw(&mut canvas, &r, &ui);
        let surface = canvas.into_surface();
        surface.save_bmp(&path).expect("save bmp");
        println!("SHOT {path}");
        return;
    }

    let window = video
        .window("RACK", panel::W, panel::H)
        .position_centered()
        .build()
        .expect("window");
    let mut canvas = window.into_canvas().present_vsync().build().expect("canvas");

    let audio = sdl.audio().expect("audio");
    let rack = Arc::new(Mutex::new(Rack::new()));
    let spec = AudioSpecDesired { freq: Some(44100), channels: Some(1), samples: Some(1024) };
    let device = audio
        .open_playback(None, &spec, |_| RackCallback { rack: Arc::clone(&rack) })
        .expect("audio device");
    device.resume();

    let mut ui = UiState::default();
    let mut mouse_pos: (i32, i32) = (0, 0);
    let mut pressed: HashMap<Keycode, (usize, u8)> = HashMap::new();
    let mut pump = sdl.event_pump().expect("event pump");
    'run: loop {
        for ev in pump.poll_iter() {
            match ev {
                Event::Quit { .. } => break 'run,
                Event::KeyDown { keycode: Some(k), repeat, .. } => {
                    if k == Keycode::Escape {
                        break 'run;
                    }
                    if k == Keycode::Space && !repeat {
                        if let Ok(mut r) = rack.lock() {
                            r.toggle_transport();
                        }
                        continue;
                    }
                    if let Some(n) = match k {
                        Keycode::Num1 => Some(0),
                        Keycode::Num2 => Some(1),
                        Keycode::Num3 => Some(2),
                        Keycode::Num4 => Some(3),
                        Keycode::Num5 => Some(4),
                        Keycode::Num6 => Some(5),
                        Keycode::Num7 => Some(6),
                        Keycode::Num8 => Some(7),
                        _ => None,
                    } {
                        if let Ok(mut r) = rack.lock() {
                            r.sp_sel = n;
                            r.sp.trigger(n);
                        }
                        continue;
                    }
                    match k {
                        Keycode::Minus => {
                            if let Ok(mut r) = rack.lock() {
                                r.sp.bpm = (r.sp.bpm - 1.0)
                                    .clamp(sp::engine::BPM_MIN, sp::engine::BPM_MAX);
                            }
                        }
                        Keycode::Equals => {
                            if let Ok(mut r) = rack.lock() {
                                r.sp.bpm = (r.sp.bpm + 1.0)
                                    .clamp(sp::engine::BPM_MIN, sp::engine::BPM_MAX);
                            }
                        }
                        Keycode::Left | Keycode::Right => {
                            if repeat {
                                continue;
                            }
                            if let Ok(mut r) = rack.lock() {
                                let d = if k == Keycode::Left { -1 } else { 1 };
                                if r.selected == 0 {
                                    r.gm_octave = (r.gm_octave + d).clamp(-2, 2);
                                } else {
                                    r.arp_octave = (r.arp_octave + d).clamp(-2, 2);
                                }
                            }
                        }
                        _ => {
                            if repeat {
                                continue;
                            }
                            if let Some(semi) = key_semi(k) {
                                if let Ok(mut r) = rack.lock() {
                                    let s = r.selected;
                                    let midi = (r.base_note(s) as i32 + semi).clamp(0, 127) as u8;
                                    if s == 0 {
                                        r.gm.voice.note_on(midi);
                                    } else {
                                        r.arp.note_on(midi as f32);
                                    }
                                    pressed.insert(k, (s, midi));
                                }
                            }
                        }
                    }
                }
                Event::KeyUp { keycode: Some(k), .. } => {
                    if let Some((s, midi)) = pressed.remove(&k) {
                        if let Ok(mut r) = rack.lock() {
                            if s == 0 {
                                r.gm.voice.note_off(midi);
                            } else {
                                r.arp.note_off(midi as f32);
                            }
                        }
                    }
                }
                Event::MouseButtonDown { x, y, .. } => {
                    mouse_pos = (x, y);
                    if let Ok(mut r) = rack.lock() {
                        let a = panel::hit(&r, x, y);
                        apply_action(&mut r, &mut ui, a, y);
                    }
                }
                Event::MouseMotion { x, y, .. } => {
                    mouse_pos = (x, y);
                    if let Some((id, start_y, start_t)) = ui.drag {
                        if let Ok(mut r) = rack.lock() {
                            id.set_t(&mut r, start_t + (start_y - y) as f32 / 150.0);
                        }
                    }
                }
                Event::MouseButtonUp { .. } => {
                    ui.drag = None;
                    if let Some((s, midi)) = ui.mouse_note.take() {
                        if let Ok(mut r) = rack.lock() {
                            if s == 0 {
                                r.gm.voice.note_off(midi);
                            } else {
                                r.arp.note_off(midi as f32);
                            }
                        }
                    }
                }
                Event::MouseWheel { y, .. } => {
                    // Wheel over a knob nudges it; y > 0 (up) increases.
                    if y != 0 {
                        if let Ok(mut r) = rack.lock() {
                            if let Action::Knob(id) = panel::hit(&r, mouse_pos.0, mouse_pos.1) {
                                let t = id.t(&r) + 0.02 * y as f32;
                                id.set_t(&mut r, t);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if let Ok(r) = rack.lock() {
            panel::draw(&mut canvas, &r, &ui);
        }
        canvas.present();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

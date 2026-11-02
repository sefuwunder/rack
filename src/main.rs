//! Rack player v1: one window, four engines, one mixer.
//!
//! Reason-style virtual rack, compact edition: an SP-1200 strip (pads,
//! steps, per-pad filter + LFO), two synth strips (macro knobs, keyboard,
//! gate row with per-step notes, filter-in switch), a sampler strip
//! (pitched sample voice, drop a .wav/.mp3 on the window), and a mixer.
//! The SP transport is the rack clock. No keys were added or changed:
//! save/load are header buttons, everything else is mouse, TAB flip,
//! or the existing keys.

mod arp;
mod font;
mod gm;
mod panel;
mod project;
mod rack;
mod rt;
mod samp;
mod sp;

use panel::{
    cable_filter, cv_filter, gate_filter, hit_back, Action, CvDst, DragSrc, GateDst, InJack,
    UiState, FLIP_RECT,
};
use rack::{CvIn, FilterIn, GateIn, Rack};
use rt::{AudioEngine, Cmd, View};
use sdl2::audio::{AudioCallback, AudioSpecDesired};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::surface::Surface;
use sp::decode::decode_file;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

/// SDL callback: owns the audio engine outright. No lock is
/// taken here; the UI reaches audio only through the command
/// channel, and the first block's size is logged once so a
/// granted size other than the requested 256 is visible.
struct RackCallback {
    engine: AudioEngine,
    announced: bool,
}

impl AudioCallback for RackCallback {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        if !self.announced {
            self.announced = true;
            eprintln!("AUDIO {}-sample blocks", out.len());
        }
        self.engine.render_block(out);
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

/// Send a command to the audio thread. The receiver lives as
/// long as the app does; a failure means audio went away.
fn send(tx: &Sender<Cmd>, cmd: Cmd) {
    let _ = tx.send(cmd);
}

fn apply_action(tx: &Sender<Cmd>, view: &View, ui: &mut UiState, a: Action, my: i32) {
    match a {
        Action::Transport => send(tx, Cmd::Transport),
        Action::BpmUp => send(tx, Cmd::BpmStep(1.0)),
        Action::BpmDown => send(tx, Cmd::BpmStep(-1.0)),
        Action::SwingUp => send(tx, Cmd::SwingStep(1.0)),
        Action::SwingDown => send(tx, Cmd::SwingStep(-1.0)),
        Action::Pad(i) => {
            send(tx, Cmd::SelectPad(i));
            send(tx, Cmd::TriggerPad(i));
        }
        Action::Step(i) => send(tx, Cmd::ToggleStep(i)),
        Action::SpFilter => send(tx, Cmd::CycleSpFilter),
        Action::Knob(id) => {
            ui.drag = Some((id, my, id.t(view)));
        }
        Action::GmFilterIn => send(tx, Cmd::CycleFilterIn(0)),
        Action::ArpFilterIn => send(tx, Cmd::CycleFilterIn(1)),
        Action::GmKey(note) => {
            send(tx, Cmd::SelectSynth(0));
            send(tx, Cmd::NoteOn { dev: 0, midi: note });
            ui.mouse_note = Some((0, note));
        }
        Action::ArpKey(note) => {
            send(tx, Cmd::SelectSynth(1));
            send(tx, Cmd::NoteOn { dev: 1, midi: note });
            ui.mouse_note = Some((1, note));
        }
        Action::SampKey(note) => {
            send(tx, Cmd::SelectSynth(2));
            send(tx, Cmd::NoteOn { dev: 2, midi: note });
            ui.mouse_note = Some((2, note));
        }
        Action::SampLoop => send(tx, Cmd::ToggleLoop),
        Action::Mute(ch) => send(tx, Cmd::ToggleMute(ch)),
        Action::SelectSynth(s) => send(tx, Cmd::SelectSynth(s)),
        Action::SelectPad(i) => send(tx, Cmd::SelectPad(i)),
        // Handled in the event loop, not here: gate presses (toggle
        // vs note drag), back-view cables, save/load buttons.
        Action::GateCell(..)
        | Action::JackOut(_)
        | Action::JackIn(_)
        | Action::GateOutJack(_)
        | Action::GateInJack(_)
        | Action::CvOutJack(_)
        | Action::CvInJack(_)
        | Action::Save
        | Action::Load => {}
        Action::None => {}
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Headless smoke: render blocks with a pad + three notes sounding.
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
        r.samp.note_on(60);
        let blocks = (secs * 44100.0 / 256.0) as usize;
        let mut peak = 0.0f32;
        for _ in 0..blocks {
            let mut blk = vec![0.0f32; 256];
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

    // --shot / --shot-back: draw the rack to a BMP without a window.
    let shot = args
        .iter()
        .position(|a| a == "--shot")
        .map(|i| (false, i))
        .or_else(|| args.iter().position(|a| a == "--shot-back").map(|i| (true, i)));
    if let Some((flipped, i)) = shot {
        let path = args.get(i + 1).cloned().unwrap_or_else(|| {
            if flipped { "rack-shot-back.bmp".to_string() } else { "rack-shot.bmp".to_string() }
        });
        let r = Rack::new();
        let view = View::from_rack(&r);
        let ui = UiState { flipped, ..UiState::default() };
        let surface =
            Surface::new(panel::W, panel::H, PixelFormatEnum::RGB24).expect("surface");
        let mut canvas = surface.into_canvas().expect("surface canvas");
        panel::draw(&mut canvas, &view, &ui);
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
    // Project + sample flags. --project loads at startup and becomes
    // the SAVE target; --sample replaces the sampler's default sine.
    let mut project_path = "rack-project.json".to_string();
    if let Some(i) = args.iter().position(|a| a == "--project") {
        if let Some(p) = args.get(i + 1) {
            project_path = p.clone();
        }
    }
    // The Rack is built here, before audio starts, so --project
    // and --sample apply without any thread handoff; the audio
    // thread then owns it outright.
    let mut rack = Rack::new();
    if args.iter().any(|a| a == "--project") && std::path::Path::new(&project_path).exists() {
        match project::load(&project_path) {
            Ok(loaded) => rack = loaded,
            Err(e) => eprintln!("--project: {e} (starting factory)"),
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--sample") {
        if let Some(p) = args.get(i + 1) {
            if let Err(e) = rack.samp.load_file(p) {
                eprintln!("--sample: {e}");
            }
        }
    }
    let (cmd_tx, cmd_rx) = channel();
    let (view_tx, view_rx) = channel();
    let engine = AudioEngine::new(rack, cmd_rx, view_tx);
    let spec = AudioSpecDesired { freq: Some(44100), channels: Some(1), samples: Some(256) };
    let device = audio
        .open_playback(None, &spec, |_| RackCallback { engine, announced: false })
        .expect("audio device");
    device.resume();

    let mut ui = UiState::default();
    let mut view = View::default();
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
                    if k == Keycode::Tab && !repeat {
                        // Flip the rack; any drag dies with the view.
                        ui.flipped = !ui.flipped;
                        ui.drag = None;
                        ui.cable_drag = None;
                        ui.cable_to = None;
                        ui.gate_press = None;
                        continue;
                    }
                    if k == Keycode::Space && !repeat {
                        send(&cmd_tx, Cmd::Transport);
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
                        send(&cmd_tx, Cmd::SelectPad(n));
                        send(&cmd_tx, Cmd::TriggerPad(n));
                        continue;
                    }
                    match k {
                        Keycode::Minus => send(&cmd_tx, Cmd::BpmStep(-1.0)),
                        Keycode::Equals => send(&cmd_tx, Cmd::BpmStep(1.0)),
                        Keycode::Left | Keycode::Right => {
                            if repeat {
                                continue;
                            }
                            let d = if k == Keycode::Left { -1 } else { 1 };
                            send(&cmd_tx, Cmd::OctaveStep { dev: view.selected, dir: d });
                        }
                        _ => {
                            if repeat {
                                continue;
                            }
                            if let Some(semi) = key_semi(k) {
                                let s = view.selected;
                                let midi = (view.base_note(s) as i32 + semi).clamp(0, 127) as u8;
                                send(&cmd_tx, Cmd::NoteOn { dev: s, midi });
                                pressed.insert(k, (s, midi));
                            }
                        }
                    }
                }
                Event::KeyUp { keycode: Some(k), .. } => {
                    if let Some((s, midi)) = pressed.remove(&k) {
                        send(&cmd_tx, Cmd::NoteOff { dev: s, midi });
                    }
                }
                Event::MouseButtonDown { x, y, .. } => {
                    mouse_pos = (x, y);
                    let (fx, fy, fw, fh) = FLIP_RECT;
                    if x >= fx && x < fx + fw && y >= fy && y < fy + fh {
                        ui.flipped = !ui.flipped;
                        ui.drag = None;
                        ui.cable_drag = None;
                        ui.cable_to = None;
                        ui.gate_press = None;
                        continue;
                    }
                    if ui.flipped {
                        match panel::hit_back(x, y) {
                            Action::JackOut(src) => {
                                ui.cable_drag = Some(DragSrc::Audio(src));
                                ui.cable_to = Some((x, y));
                            }
                            Action::GateOutJack(src) => {
                                ui.cable_drag = Some(DragSrc::Gate(src));
                                ui.cable_to = Some((x, y));
                            }
                            Action::CvOutJack(src) => {
                                ui.cable_drag = Some(DragSrc::Cv(src));
                                ui.cable_to = Some((x, y));
                            }
                            Action::JackIn(dst) => {
                                // Click a patched input to unpatch it.
                                let synth = match dst {
                                    InJack::Gm => 0,
                                    InJack::Arp => 1,
                                };
                                send(&cmd_tx, Cmd::SetFilterIn { synth, value: FilterIn::Off });
                            }
                            Action::GateInJack(dst) => {
                                let synth = match dst {
                                    GateDst::Gm => 0,
                                    GateDst::Arp => 1,
                                };
                                send(&cmd_tx, Cmd::SetGateIn { synth, value: GateIn::Off });
                            }
                            Action::CvInJack(dst) => {
                                let synth = match dst {
                                    CvDst::Gm => 0,
                                    CvDst::Arp => 1,
                                };
                                send(&cmd_tx, Cmd::SetCvIn { synth, value: CvIn::Off });
                            }
                            Action::Knob(id) => {
                                ui.drag = Some((id, y, id.t(&view)));
                            }
                            Action::SelectPad(i) => send(&cmd_tx, Cmd::SelectPad(i)),
                            Action::SelectSynth(s) => send(&cmd_tx, Cmd::SelectSynth(s)),
                            _ => {}
                        }
                        continue;
                    }
                    let a = panel::hit(&view, x, y);
                    match a {
                        Action::Save => {
                            // Serialize on the audio thread (it owns
                            // the Rack), write the file here. The
                            // reply lands within a block or two; the
                            // 100 ms cap is only a safety net.
                            let (reply_tx, reply_rx) = channel();
                            send(&cmd_tx, Cmd::SaveRequest(reply_tx));
                            ui.status = match reply_rx.recv_timeout(Duration::from_millis(100)) {
                                Ok(text) => match std::fs::write(&project_path, text) {
                                    Ok(()) => format!("SAVED {project_path}"),
                                    Err(e) => format!("can't write {project_path}: {e}"),
                                },
                                Err(_) => "SAVE TIMEOUT".to_string(),
                            };
                        }
                        Action::Load => {
                            ui.status = match project::load(&project_path) {
                                Ok(loaded) => {
                                    send(&cmd_tx, Cmd::LoadProject(loaded));
                                    format!("LOADED {project_path}")
                                }
                                Err(e) => e,
                            };
                        }
                        Action::GateCell(s, st) => {
                            // Press starts a toggle-or-note-drag:
                            // release without moving toggles, a
                            // vertical drag changes the step note.
                            let off = if s == 0 { view.gm_gate_notes[st] } else { view.arp_gate_notes[st] };
                            ui.gate_press = Some((s, st, y, off, false));
                        }
                        _ => apply_action(&cmd_tx, &view, &mut ui, a, y),
                    }
                }
                Event::MouseMotion { x, y, .. } => {
                    mouse_pos = (x, y);
                    if ui.cable_drag.is_some() {
                        ui.cable_to = Some((x, y));
                    }
                    if let Some((s, st, sy, off, _)) = ui.gate_press {
                        if (y - sy).abs() > 4 {
                            ui.gate_press = Some((s, st, sy, off, true));
                            // Drag up = higher note, 12px per semitone.
                            let noff = (off as i32 + (sy - y) / 12).clamp(-24, 24) as i8;
                            send(&cmd_tx, Cmd::GateNote { synth: s, step: st, note: noff });
                        }
                    }
                    if let Some((id, start_y, start_t)) = ui.drag {
                        send(&cmd_tx, Cmd::SetKnobT(id, start_t + (start_y - y) as f32 / 150.0));
                    }
                }
                Event::MouseButtonUp { x, y, .. } => {
                    if let Some(src) = ui.cable_drag.take() {
                        ui.cable_to = None;
                        if ui.flipped {
                            match (src, hit_back(x, y)) {
                                (DragSrc::Audio(a), Action::JackIn(dst)) => {
                                    if let Some(value) = cable_filter(a, dst) {
                                        let synth = match dst {
                                            InJack::Gm => 0,
                                            InJack::Arp => 1,
                                        };
                                        send(&cmd_tx, Cmd::SetFilterIn { synth, value });
                                    }
                                }
                                (DragSrc::Gate(g), Action::GateInJack(dst)) => {
                                    if let Some(value) = gate_filter(g, dst) {
                                        let synth = match dst {
                                            GateDst::Gm => 0,
                                            GateDst::Arp => 1,
                                        };
                                        send(&cmd_tx, Cmd::SetGateIn { synth, value });
                                    }
                                }
                                (DragSrc::Cv(cv), Action::CvInJack(dst)) => {
                                    if let Some(value) = cv_filter(cv, dst) {
                                        let synth = match dst {
                                            CvDst::Gm => 0,
                                            CvDst::Arp => 1,
                                        };
                                        send(&cmd_tx, Cmd::SetCvIn { synth, value });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if let Some((s, st, _, _, moved)) = ui.gate_press.take() {
                        if !moved {
                            send(&cmd_tx, Cmd::ToggleGate { synth: s, step: st });
                        }
                    }
                    ui.drag = None;
                    if let Some((s, midi)) = ui.mouse_note.take() {
                        send(&cmd_tx, Cmd::NoteOff { dev: s, midi });
                    }
                }
                Event::MouseWheel { y, .. } => {
                    // Front view only: the back view has no wheel
                    // targets, and front geometry must not leak through.
                    if ui.flipped {
                        continue;
                    }
                    // Wheel over a knob nudges it; wheel over a gate
                    // cell nudges its note by a semitone.
                    if y != 0 {
                        // Step commands, not read-modify-write:
                        // every tick in a scroll burst lands, even
                        // when the snapshot hasn't refreshed yet.
                        match panel::hit(&view, mouse_pos.0, mouse_pos.1) {
                            Action::Knob(id) => {
                                send(&cmd_tx, Cmd::KnobStep(id, 0.02 * y as f32));
                            }
                            Action::GateCell(s, st) => {
                                send(&cmd_tx, Cmd::GateNoteStep { synth: s, step: st, dir: y.signum() as i8 });
                            }
                            _ => {}
                        }
                    }
                }
                Event::DropFile { filename, .. } => {
                    // Decode here (slow, but the audio thread never
                    // waits on it), then hand the PCM over.
                    ui.status = match decode_file(&filename) {
                        Ok(data) => {
                            let name = filename
                                .rsplit(['/', '\\'])
                                .next()
                                .unwrap_or(&filename)
                                .to_string();
                            send(
                                &cmd_tx,
                                Cmd::LoadSample { data, name: name.clone(), path: Some(filename) },
                            );
                            format!("SAMPLE {name}")
                        }
                        Err(e) => e,
                    };
                }
                _ => {}
            }
        }
        while let Ok(v) = view_rx.try_recv() {
            view = v;
        }
        panel::draw(&mut canvas, &view, &ui);
        canvas.present();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

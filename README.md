# rack

A Reason-style virtual rack, player edition: four instruments in one
window, one mixer, fixed routings plus patch cables. Rust + SDL2, zero
npm-style dependencies (sdl2 + minimp3 only).

## The rack

Top to bottom, one 1280x1196 window:

- **Header** — SAVE / LOAD project buttons, transport (PLAY/STOP), BPM,
  swing. The SP-1200 transport is the rack clock.
- **SP-1200 strip** — 8 pads (click to play + select), 16-step row for the
  selected pad, and per-pad edit knobs: filter kind button (OFF/LP/HP),
  CUTOFF, RES, LFO RATE, LFO CUT, LFO RES, LEVEL. Boots with the SP-1200
  factory preset 0, so PLAY immediately grooves.
- **GRANDMOTHER strip** — macro knobs (OSC1, OSC2, CUTOFF, RESO, amp
  ADSR, GLIDE, LFO RATE), a 2-octave clickable keyboard, a 16-step GATE
  row with a note per step, and a FILTER IN switch.
- **ARP 2600 strip** — same shape: 3 OSC levels, CUTOFF, RESO, ADSR,
  GLIDE, keyboard, GATE row with per-step notes, FILTER IN switch.
- **SAMPLER strip** — one pitched sample voice, root C4: LEVEL, TUNE,
  START knobs, a LOOP toggle, and a 2-octave keyboard. Boots with a 1s
  440 Hz sine so the strip sounds before any file is loaded; drop a
  .wav/.mp3 on the window (or pass `--sample <path>`) to replace it.
- **MIXER strip** — LEVEL + MUTE per channel (now four), peak bars,
  MASTER.

**Flip to back** (`TAB` or the FLIP button in the header — flip is
instant, no animation): the back shows every device's OUT jack and each
synth's FILTER IN, GATE IN, and CUTOFF IN jacks, plus the
grandmother's LFO OUT, with the signal flow drawn as cables. Mixer
inputs are fixed and drawn dim; filter-in patches are orange, gate
patches green, CV patches yellow. Drag from an OUT jack to a synth's
input to patch it, click a patched IN to unpatch. The back cables are
the same routing the front FILTER IN buttons cycle — one source of
truth, two ways to work it. The back also carries the deeper controls
the front macros skip: SP pad TUNE, grandmother filter envelope /
noise / env amount / key track / reverb, ARP noise / ring /
sample-and-hold / lag / VCA gain / AR envelope / reverb. Gate rows
stay on the front: they are step sequencers, not patch points.

## Patch points

- **Filter in** (orange): FILTER IN on a synth strip routes another
  device's audio through that strip's filter — DRUMS, the other synth,
  or the SAMPLER — via a dedicated ladder instance that mirrors the
  strip's cutoff/resonance knobs. It is parallel: the source still
  sounds dry on its own channel, and the filtered copy joins the target
  strip's channel. Sources are the raw voice buses, so two strips
  feeding each other can't loop. A strip never takes itself.
- **Gate in** (green): a synth's GATE IN borrows another row's step
  pattern — the SP selected pad's row, or the other synth's row — while
  the strip keeps its own per-step notes. So the ARP can play its own
  melody on the drums' rhythm, for example. A strip never takes itself.
- **Cutoff in** (yellow): the grandmother's LFO OUT bends a strip's
  cutoff by up to +-1 octave (triangle by default, rate from the front
  LFO RATE knob). CV updates once per render block (~5.8 ms), which is
  fine for musical LFO rates. The base cutoff is restored after every
  render, so knob reads always show the unbent value. Self-patching
  the grandmother's LFO into its own cutoff is allowed: the LFO does
  not depend on the cutoff, so there is no loop.

## Per-step notes

A gate step plays its strip's base note (C3 + octave) plus the step's
own semitone offset (-24..+24, 0 = base). A gated step shows its offset
(`+7`, `-5`) in the cell. Click a step to toggle it; drag it
vertically (12 px per semitone) or wheel over it to change its note.
Gate release targets the note that actually sounded, not the base, so
an octave change mid-note can't leave a hanging voice.

## Projects

SAVE writes the rack surface to a JSON file, LOAD reads it back. The
default file is `rack-project.json` in the working directory;
`--project <path>` loads that file at startup and makes it the SAVE
target. Saved: BPM, swing, SP patterns and per-pad settings, both
synths' knob surfaces, octaves, gate rows + per-step notes, all patch
states, the sampler's knobs/octave, and the mixer. Not saved: sample
audio (the sampler's source path is saved and reloaded when the file
still exists, otherwise the default sine stays), SP pad audio (factory
drums regenerate deterministically), and synth parameters the rack UI
never exposes (those stay at their defaults, which is identical to any
state the rack can produce). No new keys were added for this: SAVE and
LOAD are header buttons, and the project format is plain JSON with a
minimal built-in parser (no new dependency).

## Clock and mixer

- **Mixer**: every device feeds its own channel (level, mute) into the
  master. All four engines render into one 44.1 kHz mono callback
  with 256-sample blocks (5.8 ms). The audio thread owns the Rack
  outright: the UI sends commands down a channel and reads a
  snapshot back each block, so no lock sits on the render path and
  the UI can stall, decode, or write files without glitching audio.
  The arp synth is stereo; its frames are folded to mono.
- **Clock gates**: on every SP step, each synth's GATE row fires: a set
  gate retriggers that synth at its base note plus the step offset, an
  unset gate releases it. Gate rows loop every 16 steps, so rows past
  the pattern length don't fire.

## Play it

```
cargo build --release
./target/release/rack
./target/release/rack --project my-song.json --sample ~/samples/bass.wav
```

System dependency: `libsdl2-dev`. Keys (unchanged — nothing was added
or rebound):

- `SPACE` play/stop, `1-8` SP pads, `ESC` quit
- Letters play the selected device: Z-row low octave, Q-row high octave
  (digits stay pads, so the Q-row black keys that would need number keys
  are skipped)
- `Left`/`Right` shift the selected device's octave, `-`/`=` BPM,
  `TAB` flips the rack
- Mouse: click pads, steps, gates, mutes, FILTER IN, SAVE/LOAD; drag
  knobs vertically; drag a gate step vertically to change its note;
  mouse wheel over a knob or a gate step nudges it. Clicking a strip
  selects that device for the letter keys. On the back: drag OUT → IN
  to patch, click a patched IN to unpatch. Drop a .wav/.mp3 on the
  window to load it into the sampler.

## Check it without a display

```
SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy ./target/release/rack --smoke 3
SDL_VIDEODRIVER=dummy ./target/release/rack --shot rack-shot.bmp
cargo test --release
```

`--smoke` renders blocks with a pad and three notes sounding and fails
on silence. `--shot` draws the front of the rack to a BMP, `--shot-back`
the back. Tests cover the mixer, the clock gates, per-step notes, gate
and CV patching, filter-in routing (including the sampler), cable patch
rules, sampler pitch/loop/file loading, project round-trips, and the
vendored engine suites.

## Vendored engines

The three instruments live in this repo as vendored copies; the original
repos are untouched and remain the source of truth for their instruments
(rack fixes do not flow back automatically):

- `src/sp/` — sp1200-rs engine, dsp, fourtrack, wav, decode
- `src/gm/` — grandmother voice, osc, filter, env, lfo, arp, reverb,
  patch, seq
- `src/arp/` — arp2600 voice, osc, filter, env, patch, reverb, dsp
- `src/font.rs` — grandmother's 5x7 bitmap font
- `src/samp.rs` — the rack sampler (not vendored: new in this repo)
- `src/project.rs` — project JSON (not vendored: new in this repo)

Copies are verbatim except `crate::` paths rewritten to the namespace
(`crate::gm::`, `crate::arp::`, `crate::sp::`), plus one marked rack
adaptation: `Synth::lfo_value()` in `src/gm/voice.rs`, the accessor
the LFO OUT jack reads. MP3 fixtures for the vendored decode tests
live in `tests/data/`.

## v1 limits

- No MIDI input, no full patch bays (macro knobs on the front, deeper
  knobs on the back, but the synths' internal patch bays are not
  exposed). Cables carry filter-in, gate, and one CV path only.
- No fourtrack tape panel (the engine is vendored; the tape UI is not
  wired in yet). Step length stays 16 in the UI.
- Gate timing is sample-accurate (a gate fires on its step
  boundary's device sample, not at a block edge); CV is still
  block-rate (one LFO value per 5.8 ms block).
- Gate notes and keyboard notes share each device's mono voice; the
  last event wins. The sampler is mono as well.
- The sampler is one voice, one sample, root C4. It does not slice,
  and it does not record: samples come from files, not from the rack.

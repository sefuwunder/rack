# rack

A Reason-style virtual rack, player edition: three instruments in one
window, one mixer, fixed routings. Rust + SDL2, zero npm-style
dependencies (sdl2 + minimp3 only).

## The rack

Top to bottom, one 1280x960 window:

- **Header** — transport (PLAY/STOP), BPM, swing. The SP-1200 transport
  is the rack clock.
- **SP-1200 strip** — 8 pads (click to play + select), 16-step row for the
  selected pad, and per-pad edit knobs: filter kind button (OFF/LP/HP),
  CUTOFF, RES, LFO RATE, LFO CUT, LFO RES, LEVEL. Boots with the SP-1200
  factory preset 0, so PLAY immediately grooves.
- **GRANDMOTHER strip** — macro knobs (OSC1, OSC2, CUTOFF, RESO, amp
  ADSR, GLIDE, LFO RATE), a 2-octave clickable keyboard, a 16-step GATE
  row, and a FILTER IN switch.
- **ARP 2600 strip** — same shape: 3 OSC levels, CUTOFF, RESO, ADSR,
  GLIDE, keyboard, GATE row, FILTER IN switch.
- **MIXER strip** — LEVEL + MUTE per channel, peak bars, MASTER.

**Flip to back** (`TAB` or the FLIP button in the header — flip is
instant, no animation): the back shows every device's OUT jack and each
synth's FILTER IN jack, with the signal flow drawn as cables. Mixer
inputs are fixed and drawn dim; the filter-in patches are bright.
Drag from an OUT jack to a synth's FILTER IN to patch it, click a
patched IN to unpatch. The back cables are the same routing the front
FILTER IN buttons cycle — one source of truth, two ways to work it.
The back also carries the deeper controls the front macros skip: SP
pad TUNE, grandmother filter envelope / noise / env amount / key
track / reverb, ARP noise / ring / sample-and-hold / lag / VCA gain /
AR envelope / reverb. Gate rows stay on the front: they are step
sequencers, not patch points.

## Fixed routings (v1 has no free cables)

- **Mixer**: every device feeds its own channel (level, mute) into the
  master. All three engines render into one 44.1 kHz mono callback.
- **Clock gates**: on every SP step, each synth's GATE row fires: a set
  gate retriggers that synth at its base note (C3 + octave), an unset
  gate releases it. Gate rows loop every 16 steps, so rows past the
  pattern length don't fire.
- **Filter in**: FILTER IN on a synth strip routes another device's audio
  through that strip's filter — DRUMS or the other synth — via a
  dedicated ladder instance that mirrors the strip's cutoff/resonance
  knobs. It is parallel: the source still sounds dry on its own channel,
  and the filtered copy joins the target strip's channel. Sources are
  the raw voice buses, so two strips feeding each other can't loop.

## Play it

```
cargo build --release
./target/release/rack
```

System dependency: `libsdl2-dev`. Keys:

- `SPACE` play/stop, `1-8` SP pads, `ESC` quit
- Letters play the selected synth: Z-row low octave, Q-row high octave
  (digits stay pads, so the Q-row black keys that would need number keys
  are skipped)
- `Left`/`Right` shift the selected synth's octave, `-`/`=` BPM,
  `TAB` flips the rack
- Mouse: click pads, steps, gates, mutes, FILTER IN; drag knobs
  vertically; mouse wheel over a knob nudges it. Clicking a strip
  selects that synth for the letter keys. On the back: drag OUT → IN
  to patch, click a patched IN to unpatch.

## Check it without a display

```
SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy ./target/release/rack --smoke 3
SDL_VIDEODRIVER=dummy ./target/release/rack --shot rack-shot.bmp
cargo test --release
```

`--smoke` renders blocks with a pad and two notes sounding and fails on
silence. `--shot` draws the front of the rack to a BMP, `--shot-back`
the back. Tests cover the mixer, the clock gates, filter-in routing,
cable patch rules, and the vendored engine suites.

## Vendored engines

The three instruments live in this repo as vendored copies; the original
repos are untouched and remain the source of truth for their instruments
(rack fixes do not flow back automatically):

- `src/sp/` — sp1200-rs engine, dsp, fourtrack, wav, decode
- `src/gm/` — grandmother voice, osc, filter, env, lfo, arp, reverb,
  patch, seq
- `src/arp/` — arp2600 voice, osc, filter, env, patch, reverb, dsp
- `src/font.rs` — grandmother's 5x7 bitmap font

Copies are verbatim except `crate::` paths rewritten to the namespace
(`crate::gm::`, `crate::arp::`, `crate::sp::`). MP3 fixtures for the
vendored decode tests live in `tests/data/`.

## v1 limits

- No save/load, no MIDI input, no full patch bays (macro knobs on the
  front, deeper knobs on the back, but the synths' internal patch bays
  are not exposed). Cables carry the audio filter-in paths only.
- No fourtrack tape panel (the engine is vendored; the tape UI is not
  wired in yet). Step length stays 16 in the UI.
- Gate timing is block-level (a gate note starts within the same
  23 ms block as its step, not sample-accurately).
- Gate notes and keyboard notes share each synth's mono voice; the last
  event wins.

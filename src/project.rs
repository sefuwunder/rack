//! Project save/load: one JSON file for the rack surface.
//!
//! Saved: transport (bpm, swing), SP patterns (16 steps) and per-pad
//! filter/LFO/level/tune/label, both synths' knob surfaces (front
//! macros + back knobs), octaves, gate rows + per-step notes, all
//! patch states (filter/gate/CV), the sampler's knobs/octave, and the
//! mixer. Deliberately NOT saved: sample audio (the sampler's source
//! path is saved and reloaded when the file still exists, otherwise
//! the default sine stays), SP pad audio (factory drums regenerate
//! deterministically), and synth parameters the rack UI never exposes
//! (oscillator waves, internal patch routings) — those stay default,
//! which is identical to the state the rack can produce.
//!
//! No new dependencies: a minimal JSON value, parser, and serializer
//! live here. Loading is tolerant: unknown or missing keys keep the
//! fresh-rack default, so older projects still load.

use crate::rack::{CvIn, FilterIn, GateIn, Rack};
use crate::sp::dsp::FilterKind;

// ---- Minimal JSON ----

#[derive(Clone, Debug, PartialEq)]
pub enum JVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JVal>),
    Obj(Vec<(String, JVal)>),
}

impl JVal {
    fn get(&self, key: &str) -> Option<&JVal> {
        match self {
            JVal::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn as_f64(&self) -> Option<f64> {
        match self {
            JVal::Num(n) => Some(*n),
            _ => None,
        }
    }
    fn as_bool(&self) -> Option<bool> {
        match self {
            JVal::Bool(b) => Some(*b),
            _ => None,
        }
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            JVal::Str(s) => Some(s),
            _ => None,
        }
    }
    fn as_arr(&self) -> Option<&[JVal]> {
        match self {
            JVal::Arr(a) => Some(a),
            _ => None,
        }
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render(v: &JVal) -> String {
    match v {
        JVal::Null => "null".to_string(),
        JVal::Bool(b) => b.to_string(),
        JVal::Num(n) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{:.0}", n)
            } else {
                format!("{}", n)
            }
        }
        JVal::Str(s) => escape(s),
        JVal::Arr(a) => {
            let parts: Vec<String> = a.iter().map(render).collect();
            format!("[{}]", parts.join(","))
        }
        JVal::Obj(pairs) => {
            let parts: Vec<String> =
                pairs.iter().map(|(k, v)| format!("{}:{}", escape(k), render(v))).collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Result<u8, String> {
        self.ws();
        self.b.get(self.i).copied().ok_or_else(|| "unexpected end of input".to_string())
    }
    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek()? == c {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.i))
        }
    }
    fn lit(&mut self, s: &str) -> Result<(), String> {
        if self.b[self.i..].starts_with(s.as_bytes()) {
            self.i += s.len();
            Ok(())
        } else {
            Err(format!("expected '{s}' at byte {}", self.i))
        }
    }
    fn value(&mut self) -> Result<JVal, String> {
        match self.peek()? {
            b'{' => {
                self.i += 1;
                let mut pairs = Vec::new();
                if self.peek()? == b'}' {
                    self.i += 1;
                    return Ok(JVal::Obj(pairs));
                }
                loop {
                    let k = match self.value()? {
                        JVal::Str(s) => s,
                        _ => return Err(format!("object key must be a string at byte {}", self.i)),
                    };
                    self.expect(b':')?;
                    let v = self.value()?;
                    pairs.push((k, v));
                    match self.peek()? {
                        b',' => {
                            self.i += 1;
                        }
                        b'}' => {
                            self.i += 1;
                            break;
                        }
                        c => return Err(format!("expected ',' or '}}', got '{}'", c as char)),
                    }
                }
                Ok(JVal::Obj(pairs))
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                if self.peek()? == b']' {
                    self.i += 1;
                    return Ok(JVal::Arr(items));
                }
                loop {
                    items.push(self.value()?);
                    match self.peek()? {
                        b',' => {
                            self.i += 1;
                        }
                        b']' => {
                            self.i += 1;
                            break;
                        }
                        c => return Err(format!("expected ',' or ']', got '{}'", c as char)),
                    }
                }
                Ok(JVal::Arr(items))
            }
            b'"' => {
                self.i += 1;
                let mut bytes: Vec<u8> = Vec::new();
                loop {
                    let c = *self.b.get(self.i).ok_or("unterminated string")?;
                    self.i += 1;
                    match c {
                        b'"' => break,
                        b'\\' => {
                            let e = *self.b.get(self.i).ok_or("unterminated escape")?;
                            self.i += 1;
                            match e {
                                b'"' => bytes.push(b'"'),
                                b'\\' => bytes.push(b'\\'),
                                b'/' => bytes.push(b'/'),
                                b'n' => bytes.push(b'\n'),
                                b'r' => bytes.push(b'\r'),
                                b't' => bytes.push(b'\t'),
                                b'u' => {
                                    let hex = self.b.get(self.i..self.i + 4).ok_or("bad \\u escape")?;
                                    let hex = std::str::from_utf8(hex).map_err(|_| "bad \\u escape")?;
                                    let cp = u32::from_str_radix(hex, 16).map_err(|_| "bad \\u escape")?;
                                    let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                                    let mut buf = [0u8; 4];
                                    bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                                    self.i += 4;
                                }
                                _ => return Err(format!("bad escape '\\{}'", e as char)),
                            }
                        }
                        _ => bytes.push(c),
                    }
                }
                let s = String::from_utf8(bytes).map_err(|_| "string is not valid UTF-8")?;
                Ok(JVal::Str(s))
            }
            b't' => {
                self.lit("true")?;
                Ok(JVal::Bool(true))
            }
            b'f' => {
                self.lit("false")?;
                Ok(JVal::Bool(false))
            }
            b'n' => {
                self.lit("null")?;
                Ok(JVal::Null)
            }
            _ => {
                let start = self.i;
                while self.i < self.b.len()
                    && matches!(self.b[self.i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                {
                    self.i += 1;
                }
                let text = std::str::from_utf8(&self.b[start..self.i])
                    .map_err(|_| "bad number".to_string())?;
                text.parse::<f64>().map(JVal::Num).map_err(|_| format!("bad number '{text}'"))
            }
        }
    }
}

pub fn parse(src: &str) -> Result<JVal, String> {
    let mut p = Parser { b: src.as_bytes(), i: 0 };
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("trailing data at byte {}", p.i));
    }
    Ok(v)
}

// ---- Save / load ----

fn num(v: f32) -> JVal {
    JVal::Num(v as f64)
}

fn bools(a: &[bool]) -> JVal {
    JVal::Arr(a.iter().map(|b| JVal::Bool(*b)).collect())
}

fn obj(pairs: Vec<(&str, JVal)>) -> JVal {
    JVal::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn filter_str(f: FilterIn) -> &'static str {
    match f {
        FilterIn::Off => "OFF",
        FilterIn::Sp => "SP",
        FilterIn::Gm => "GM",
        FilterIn::Arp => "ARP",
        FilterIn::Samp => "SAMP",
    }
}

fn parse_filter(s: &str) -> FilterIn {
    match s {
        "SP" => FilterIn::Sp,
        "GM" => FilterIn::Gm,
        "ARP" => FilterIn::Arp,
        "SAMP" => FilterIn::Samp,
        _ => FilterIn::Off,
    }
}

fn gate_str(g: GateIn) -> &'static str {
    match g {
        GateIn::Off => "OFF",
        GateIn::Sp => "SP",
        GateIn::Gm => "GM",
        GateIn::Arp => "ARP",
    }
}

fn parse_gate(s: &str) -> GateIn {
    match s {
        "SP" => GateIn::Sp,
        "GM" => GateIn::Gm,
        "ARP" => GateIn::Arp,
        _ => GateIn::Off,
    }
}

fn cv_str(c: CvIn) -> &'static str {
    match c {
        CvIn::Off => "OFF",
        CvIn::Lfo => "LFO",
    }
}

fn parse_cv(s: &str) -> CvIn {
    match s {
        "LFO" => CvIn::Lfo,
        _ => CvIn::Off,
    }
}

fn synth_obj(
    knobs: Vec<(&str, f32)>,
    octave: i32,
    gates: &[bool; 16],
    notes: &[i8; 16],
    filter_in: FilterIn,
    gate_in: GateIn,
    cv_in: CvIn,
) -> JVal {
    obj(vec![
        ("knobs", JVal::Obj(knobs.into_iter().map(|(k, v)| (k.to_string(), num(v))).collect())),
        ("octave", JVal::Num(octave as f64)),
        ("gates", bools(gates)),
        (
            "notes",
            JVal::Arr(notes.iter().map(|n| JVal::Num(*n as f64)).collect()),
        ),
        ("filter_in", JVal::Str(filter_str(filter_in).to_string())),
        ("gate_in", JVal::Str(gate_str(gate_in).to_string())),
        ("cv_in", JVal::Str(cv_str(cv_in).to_string())),
    ])
}

pub fn serialize(r: &Rack) -> String {
    let patterns = JVal::Arr(
        r.sp.patterns
            .iter()
            .map(|row| bools(&row[..16]))
            .collect(),
    );
    let pads = JVal::Arr(
        r.sp.pads
            .iter()
            .map(|p| {
                obj(vec![
                    ("kind", JVal::Str(p.filter_kind.as_str().to_string())),
                    ("cutoff", num(p.filter_cutoff)),
                    ("res", num(p.filter_res)),
                    ("lfo_rate", num(p.lfo_rate)),
                    ("lfo_cut", num(p.lfo_cut)),
                    ("lfo_res", num(p.lfo_res)),
                    ("level", num(p.level)),
                    ("tune", num(p.tune_st)),
                    (
                        "label",
                        match &p.label {
                            Some(l) => JVal::Str(l.clone()),
                            None => JVal::Null,
                        },
                    ),
                ])
            })
            .collect(),
    );
    let g = &r.gm.params;
    let gm = synth_obj(
        vec![
            ("osc1", g.osc1_level),
            ("osc2", g.osc2_level),
            ("cutoff", g.cutoff),
            ("reso", g.reso),
            ("attack", g.a_attack),
            ("decay", g.a_decay),
            ("sustain", g.a_sustain),
            ("release", g.a_release),
            ("glide", g.glide),
            ("lfo_rate", g.lfo_rate),
            ("noise", g.noise_level),
            ("env_amt", g.env_amt),
            ("kb_track", g.kb_track),
            ("f_attack", g.f_attack),
            ("f_decay", g.f_decay),
            ("f_sustain", g.f_sustain),
            ("f_release", g.f_release),
            ("reverb", g.reverb_mix),
        ],
        r.gm_octave,
        &r.gm_gate,
        &r.gm_gate_notes,
        r.gm_filter_in,
        r.gm_gate_in,
        r.gm_cv_in,
    );
    let a = &r.arp.params;
    let arp = synth_obj(
        vec![
            ("osc1", a.vco1_level),
            ("osc2", a.vco2_level),
            ("osc3", a.vco3_level),
            ("cutoff", a.vcf_cutoff),
            ("reso", a.vcf_reso),
            ("attack", a.adsr_a),
            ("decay", a.adsr_d),
            ("sustain", a.adsr_s),
            ("release", a.adsr_r),
            ("glide", a.glide),
            ("noise", a.noise_level),
            ("ring", a.ring_level),
            ("sh_rate", a.sh_rate),
            ("lag", a.lag_time),
            ("vca_gain", a.vca_gain),
            ("ar_attack", a.ar_a),
            ("ar_release", a.ar_r),
            ("reverb", a.rev_mix),
        ],
        r.arp_octave,
        &r.arp_gate,
        &r.arp_gate_notes,
        r.arp_filter_in,
        r.arp_gate_in,
        r.arp_cv_in,
    );
    let samp = obj(vec![
        ("level", num(r.samp.level)),
        ("tune", num(r.samp.tune_st)),
        ("start", num(r.samp.start)),
        ("loop", JVal::Bool(r.samp.loop_)),
        ("octave", JVal::Num(r.samp.octave as f64)),
        ("name", JVal::Str(r.samp.name.clone())),
        (
            "path",
            match &r.samp.path {
                Some(p) => JVal::Str(p.clone()),
                None => JVal::Null,
            },
        ),
    ]);
    let mixer = obj(vec![
        (
            "levels",
            JVal::Arr(r.level.iter().map(|v| num(*v)).collect()),
        ),
        ("mutes", bools(&r.mute)),
        ("master", num(r.master)),
    ]);
    render(&obj(vec![
        ("version", JVal::Num(1.0)),
        ("bpm", JVal::Num(r.sp.bpm)),
        ("swing", JVal::Num(r.sp.swing)),
        ("sp_sel", JVal::Num(r.sp_sel as f64)),
        ("selected", JVal::Num(r.selected as f64)),
        ("sp_patterns", patterns),
        ("sp_pads", pads),
        ("gm", gm),
        ("arp", arp),
        ("samp", samp),
        ("mixer", mixer),
    ]))
}

fn get_f<'a>(v: &'a JVal, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}

fn get_b(v: &JVal, key: &str) -> Option<bool> {
    v.get(key).and_then(|x| x.as_bool())
}

fn get_s<'a>(v: &'a JVal, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

fn apply_bools(dst: &mut [bool; 16], v: &JVal, key: &str) {
    if let Some(arr) = v.get(key).and_then(|x| x.as_arr()) {
        for (i, slot) in dst.iter_mut().enumerate() {
            if let Some(JVal::Bool(b)) = arr.get(i) {
                *slot = *b;
            }
        }
    }
}

fn apply_notes(dst: &mut [i8; 16], v: &JVal, key: &str) {
    if let Some(arr) = v.get(key).and_then(|x| x.as_arr()) {
        for (i, slot) in dst.iter_mut().enumerate() {
            if let Some(JVal::Num(n)) = arr.get(i) {
                *slot = (*n as i32).clamp(-24, 24) as i8;
            }
        }
    }
}

/// Load a project file into a fresh rack. Missing keys keep defaults.
/// The sampler's saved path is reloaded when the file still exists;
/// otherwise the default sine stays (the saved name is not shown for
/// audio that isn't there).
pub fn load(path: &str) -> Result<Rack, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))?;
    let v = parse(&text)?;
    let mut r = Rack::new();
    if let Some(n) = get_f(&v, "bpm") {
        r.sp.bpm = n;
    }
    if let Some(n) = get_f(&v, "swing") {
        r.sp.swing = n;
    }
    if let Some(n) = get_f(&v, "sp_sel") {
        r.sp_sel = (n as usize).min(7);
    }
    if let Some(n) = get_f(&v, "selected") {
        r.selected = (n as usize).min(2);
    }
    if let Some(arr) = v.get("sp_patterns").and_then(|x| x.as_arr()) {
        r.sp.patterns = [[false; crate::sp::engine::MAX_STEPS]; crate::sp::engine::N_PADS];
        for (p, row) in arr.iter().enumerate().take(8) {
            if let Some(steps) = row.as_arr() {
                for (i, s) in steps.iter().enumerate().take(16) {
                    if let JVal::Bool(b) = s {
                        r.sp.patterns[p][i] = *b;
                    }
                }
            }
        }
    }
    if let Some(arr) = v.get("sp_pads").and_then(|x| x.as_arr()) {
        for (p, pad) in arr.iter().enumerate().take(8) {
            let kind = get_s(pad, "kind").map(FilterKind::from_str).unwrap_or(FilterKind::Off);
            let cut = get_f(pad, "cutoff").unwrap_or(r.sp.pads[p].filter_cutoff as f64) as f32;
            r.sp.set_filter(p, kind, cut);
            if let Some(n) = get_f(pad, "res") {
                r.sp.set_res(p, n as f32);
            }
            if let Some(n) = get_f(pad, "lfo_rate") {
                r.sp.set_lfo_rate(p, n as f32);
            }
            if let Some(n) = get_f(pad, "lfo_cut") {
                r.sp.set_lfo_cut(p, n as f32);
            }
            if let Some(n) = get_f(pad, "lfo_res") {
                r.sp.set_lfo_res(p, n as f32);
            }
            if let Some(n) = get_f(pad, "level") {
                r.sp.pads[p].level = (n as f32).clamp(0.0, 1.0);
            }
            if let Some(n) = get_f(pad, "tune") {
                r.sp.pads[p].tune_st = (n as f32).clamp(-24.0, 24.0);
            }
            if let Some(l) = get_s(pad, "label") {
                r.sp.pads[p].label = Some(l.to_string());
            }
        }
    }
    if let Some(g) = v.get("gm") {
        let p = &mut r.gm.params;
        if let Some(JVal::Obj(pairs)) = g.get("knobs") {
            for (k, val) in pairs {
                if let JVal::Num(n) = val {
                    let n = *n as f32;
                    match k.as_str() {
                        "osc1" => p.osc1_level = n,
                        "osc2" => p.osc2_level = n,
                        "cutoff" => p.cutoff = n,
                        "reso" => p.reso = n,
                        "attack" => p.a_attack = n,
                        "decay" => p.a_decay = n,
                        "sustain" => p.a_sustain = n,
                        "release" => p.a_release = n,
                        "glide" => p.glide = n,
                        "lfo_rate" => p.lfo_rate = n,
                        "noise" => p.noise_level = n,
                        "env_amt" => p.env_amt = n,
                        "kb_track" => p.kb_track = n,
                        "f_attack" => p.f_attack = n,
                        "f_decay" => p.f_decay = n,
                        "f_sustain" => p.f_sustain = n,
                        "f_release" => p.f_release = n,
                        "reverb" => p.reverb_mix = n,
                        _ => {}
                    }
                }
            }
        }
        if let Some(n) = get_f(g, "octave") {
            r.gm_octave = (n as i32).clamp(-2, 2);
        }
        apply_bools(&mut r.gm_gate, g, "gates");
        apply_notes(&mut r.gm_gate_notes, g, "notes");
        if let Some(s) = get_s(g, "filter_in") {
            r.gm_filter_in = parse_filter(s);
        }
        if let Some(s) = get_s(g, "gate_in") {
            r.gm_gate_in = parse_gate(s);
        }
        if let Some(s) = get_s(g, "cv_in") {
            r.gm_cv_in = parse_cv(s);
        }
    }
    if let Some(a) = v.get("arp") {
        let p = &mut r.arp.params;
        if let Some(JVal::Obj(pairs)) = a.get("knobs") {
            for (k, val) in pairs {
                if let JVal::Num(n) = val {
                    let n = *n as f32;
                    match k.as_str() {
                        "osc1" => p.vco1_level = n,
                        "osc2" => p.vco2_level = n,
                        "osc3" => p.vco3_level = n,
                        "cutoff" => p.vcf_cutoff = n,
                        "reso" => p.vcf_reso = n,
                        "attack" => p.adsr_a = n,
                        "decay" => p.adsr_d = n,
                        "sustain" => p.adsr_s = n,
                        "release" => p.adsr_r = n,
                        "glide" => p.glide = n,
                        "noise" => p.noise_level = n,
                        "ring" => p.ring_level = n,
                        "sh_rate" => p.sh_rate = n,
                        "lag" => p.lag_time = n,
                        "vca_gain" => p.vca_gain = n,
                        "ar_attack" => p.ar_a = n,
                        "ar_release" => p.ar_r = n,
                        "reverb" => p.rev_mix = n,
                        _ => {}
                    }
                }
            }
        }
        if let Some(n) = get_f(a, "octave") {
            r.arp_octave = (n as i32).clamp(-2, 2);
        }
        apply_bools(&mut r.arp_gate, a, "gates");
        apply_notes(&mut r.arp_gate_notes, a, "notes");
        if let Some(s) = get_s(a, "filter_in") {
            r.arp_filter_in = parse_filter(s);
        }
        if let Some(s) = get_s(a, "gate_in") {
            r.arp_gate_in = parse_gate(s);
        }
        if let Some(s) = get_s(a, "cv_in") {
            r.arp_cv_in = parse_cv(s);
        }
    }
    if let Some(s) = v.get("samp") {
        if let Some(n) = get_f(s, "level") {
            r.samp.level = (n as f32).clamp(0.0, 1.0);
        }
        if let Some(n) = get_f(s, "tune") {
            r.samp.tune_st = (n as f32).clamp(-24.0, 24.0);
        }
        if let Some(n) = get_f(s, "start") {
            r.samp.start = (n as f32).clamp(0.0, 1.0);
        }
        if let Some(b) = get_b(s, "loop") {
            r.samp.loop_ = b;
        }
        if let Some(n) = get_f(s, "octave") {
            r.samp.octave = (n as i32).clamp(-2, 2);
        }
        if let Some(p) = get_s(s, "path") {
            let _ = r.samp.load_file(p);
        }
    }
    if let Some(m) = v.get("mixer") {
        if let Some(arr) = m.get("levels").and_then(|x| x.as_arr()) {
            for (i, slot) in r.level.iter_mut().enumerate() {
                if let Some(JVal::Num(n)) = arr.get(i) {
                    *slot = (*n as f32).clamp(0.0, 1.0);
                }
            }
        }
        if let Some(arr) = m.get("mutes").and_then(|x| x.as_arr()) {
            for (i, slot) in r.mute.iter_mut().enumerate() {
                if let Some(JVal::Bool(b)) = arr.get(i) {
                    *slot = *b;
                }
            }
        }
        if let Some(n) = get_f(m, "master") {
            r.master = (n as f32).clamp(0.0, 1.0);
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips() {
        let v = parse(r#"{"a":[1,2.5,"x",true,null],"b":{"c":"d\ne"}}"#).unwrap();
        assert_eq!(render(&v), r#"{"a":[1,2.5,"x",true,null],"b":{"c":"d\ne"}}"#);
        assert!(parse("{bad").is_err());
        assert!(parse("[1,]").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn project_round_trips() {
        let mut r = Rack::new();
        r.sp.bpm = 132.0;
        r.sp.swing = 55.0;
        r.sp_sel = 3;
        r.selected = 2;
        r.sp.patterns[2][5] = true;
        r.sp.set_filter(1, FilterKind::Lowpass, 800.0);
        r.sp.set_res(1, 2.5);
        r.sp.pads[1].level = 0.6;
        r.sp.pads[1].tune_st = -5.0;
        r.sp.pads[1].label = Some("KICK2".to_string());
        r.gm.params.cutoff = 1234.0;
        r.gm.params.noise_level = 0.3;
        r.gm.params.reverb_mix = 0.4;
        r.gm_octave = 1;
        r.gm_gate[7] = true;
        r.gm_gate_notes[7] = 12;
        r.gm_filter_in = FilterIn::Samp;
        r.gm_gate_in = GateIn::Sp;
        r.gm_cv_in = CvIn::Lfo;
        r.arp.params.vcf_cutoff = 2222.0;
        r.arp.params.ring_level = 0.5;
        r.arp.params.sh_rate = 9.0;
        r.arp_gate_notes[2] = -7;
        r.samp.level = 0.7;
        r.samp.tune_st = 3.0;
        r.samp.start = 0.25;
        r.samp.loop_ = true;
        r.samp.octave = -1;
        r.level = [0.9, 0.7, 0.6, 0.5];
        r.mute = [false, true, false, false];
        r.master = 0.75;
        let path = "/tmp/rack-project-roundtrip.json";
        // Production path, split across threads: the audio thread
        // serializes, the UI thread writes the file.
        std::fs::write(path, serialize(&r)).unwrap();
        let r2 = load(path).unwrap();
        assert_eq!(r2.sp.bpm, 132.0);
        assert_eq!(r2.sp.swing, 55.0);
        assert_eq!(r2.sp_sel, 3);
        assert_eq!(r2.selected, 2);
        assert!(r2.sp.patterns[2][5]);
        assert!(!r2.sp.patterns[2][6]);
        assert_eq!(r2.sp.pads[1].filter_kind, FilterKind::Lowpass);
        assert_eq!(r2.sp.pads[1].filter_cutoff, 800.0);
        assert_eq!(r2.sp.pads[1].filter_res, 2.5);
        assert_eq!(r2.sp.pads[1].level, 0.6);
        assert_eq!(r2.sp.pads[1].tune_st, -5.0);
        assert_eq!(r2.sp.pads[1].label.as_deref(), Some("KICK2"));
        assert_eq!(r2.gm.params.cutoff, 1234.0);
        assert_eq!(r2.gm.params.noise_level, 0.3);
        assert_eq!(r2.gm.params.reverb_mix, 0.4);
        assert_eq!(r2.gm_octave, 1);
        assert!(r2.gm_gate[7]);
        assert_eq!(r2.gm_gate_notes[7], 12);
        assert_eq!(r2.gm_filter_in, FilterIn::Samp);
        assert_eq!(r2.gm_gate_in, GateIn::Sp);
        assert_eq!(r2.gm_cv_in, CvIn::Lfo);
        assert_eq!(r2.arp.params.vcf_cutoff, 2222.0);
        assert_eq!(r2.arp.params.ring_level, 0.5);
        assert_eq!(r2.arp.params.sh_rate, 9.0);
        assert_eq!(r2.arp_gate_notes[2], -7);
        assert_eq!(r2.samp.level, 0.7);
        assert_eq!(r2.samp.tune_st, 3.0);
        assert_eq!(r2.samp.start, 0.25);
        assert!(r2.samp.loop_);
        assert_eq!(r2.samp.octave, -1);
        assert_eq!(r2.level, [0.9, 0.7, 0.6, 0.5]);
        assert_eq!(r2.mute, [false, true, false, false]);
        assert_eq!(r2.master, 0.75);
        assert!(load("/tmp/rack-project-missing.json").is_err());
    }
}

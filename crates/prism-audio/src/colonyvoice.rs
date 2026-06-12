//! `ColonyVoice` — sonifies a whole **colony** (docs/organism.md Slice 0). It
//! reads the `world`, finds EVERY membrane in it, and sums one
//! [`MembraneVoice`](crate::membranevoice)-style blend per cell — each at a
//! pitch derived from the cell's key (a pentatonic scale, so the colony is
//! always a chord). So **division is audible for free**: a new cell → a new
//! note in the chorus; a cell that fully decays (no atoms) → its note fades;
//! the F/Φ/B cycle inside each cell morphs that note's timbre.
//!
//! Phase is a single free-running sample counter `t` (each cell's phase is
//! `(t+i)·pitch/sr`), so no per-cell phase state is needed across a *dynamic*
//! population. Block-rate. This is the dynamic, colony-scale `Structure→Signal`
//! render (Slice 1 generalises it to the functor).

use std::any::Any;
use std::f64::consts::TAU;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::membranevoice::count_roles;
use crate::signal::{signal_from_slice, signal_type};

/// A pentatonic-major scale over two octaves (semitone offsets) — any subset of
/// cells sounds consonant, so a growing/dividing colony stays musical.
const PENTATONIC: [i32; 10] = [0, 2, 4, 7, 9, 12, 14, 16, 19, 21];

/// `(key, f, phi, b)` for every membrane in the tree, with each cell's key (for
/// a stable per-cell pitch). Recurses; nested cells each get their own voice.
fn collect_cells(v: &Value, key: &str, out: &mut Vec<(String, f64, f64, f64)>) {
    if v.get_field("_type").and_then(|t| t.as_str()) == Some("Membrane") {
        let (f, phi, b) = count_roles(v);
        out.push((key.to_string(), f, phi, b));
    }
    if let Some(m) = v.as_map() {
        for (k, child) in m.iter() {
            if !k.starts_with('_') {
                collect_cells(child, k, out);
            }
        }
    }
}

/// A stable pitch for a cell from its key: hash → a pentatonic degree → Hz.
fn cell_pitch(key: &str, base_hz: f64) -> f64 {
    let hash: u32 = key.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let semis = PENTATONIC[(hash as usize) % PENTATONIC.len()];
    base_hz * 2f64.powf(semis as f64 / 12.0)
}

#[derive(Clone, Debug)]
pub struct ColonyVoice {
    pub base_hz: f64,
    pub sample_rate: f64,
    pub block_size: usize,
    pub gain: f64,
    pub full: f64,
}

impl ColonyVoice {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            base_hz: cfg("base").unwrap_or(110.0),
            sample_rate: cfg("sample_rate").unwrap_or(48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            gain: cfg("gain").unwrap_or(0.3),
            full: cfg("full").unwrap_or(6.0).max(1.0),
        }
    }
}

impl Process for ColonyVoice {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("t".to_string(), Schema::float()),
            ("world".to_string(), Schema::Any),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("t".to_string(), Schema::overwrite(Schema::float())),
            ("out".to_string(), signal_type()),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let t = state.get_field("t").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let world = state.get_field("world").cloned().unwrap_or_else(Value::map);

        let mut cells = Vec::new();
        collect_cells(&world, "_root", &mut cells);

        let n = self.block_size;
        let mut out = vec![0.0f32; n];
        // Equal-power-ish sum so a thickening chorus does not clip.
        let norm = 1.0 / (cells.len().max(1) as f64).sqrt();

        for (key, f, phi, b) in &cells {
            let total = f + phi + b;
            if total <= 0.0 {
                continue;
            }
            let pitch = cell_pitch(key, self.base_hz);
            let inc = pitch / self.sample_rate;
            let amp = self.gain * (total / self.full).min(1.0) * norm;
            let (wf, wphi, wb) = (f / total, phi / total, b / total);

            for (i, sample) in out.iter_mut().enumerate() {
                let phase = (t + i as f64) * inc;
                let frac = phase - phase.floor();
                let saw = 2.0 * frac - 1.0; // F — bright
                let tri = if frac < 0.5 { 4.0 * frac - 1.0 } else { 3.0 - 4.0 * frac }; // Φ
                let sub_frac = (phase * 0.5) - (phase * 0.5).floor();
                let sub = (TAU * sub_frac).sin(); // B — sub
                *sample += (amp * (wf * saw + wphi * tri + wb * sub)) as f32;
            }
        }

        Update::value(Value::tree([
            ("t", Value::float(t + n as f64)),
            ("out", signal_from_slice(&out)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

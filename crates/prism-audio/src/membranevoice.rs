//! `MembraneVoice` — the **sonification bridge** for the M/R organism
//! (docs/organism.md Slice 0). It reads a *membrane* (a map of role-atoms
//! `{_type: F|Phi|B}`, rewritten by the metabolism BRS) and makes the cell
//! **sound its current F/Φ/B blend** — the metabolic state, heard:
//!
//! - **F** (metabolism, the active doer) → a **saw** (bright, full harmonics);
//! - **Φ** (repair, the restorer)        → a **triangle** (mellow, soft);
//! - **B** (substrate, the raw)          → a **sub** sine an octave down (depth).
//!
//! The block is `(f·saw + φ·tri + b·sub) / (f+φ+b)` at the cell's `pitch`, so as
//! reactions retype atoms the *waveform morphs* — entropy (→B) darkens and
//! deepens it, repair/replication (→F/Φ) brighten it. Amplitude tracks
//! **liveness** (the atom count): a full cell is loud, an empty one (full decay)
//! is silent. This is a hand-built `Structure → Signal` render; Slice 1
//! generalises it to the render functor.
//!
//! Block-rate (`interval = block / sample_rate`); `phase` lives on a state slot.

use std::any::Any;
use std::f64::consts::TAU;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in, modulated_freq};
use crate::signal::{signal_from_slice, signal_type};

/// Count the role-atoms `{_type: F|Phi|B}` in a membrane subtree. Recurses, so
/// it works wired to a single membrane *or* a whole `world`, and a cell's sound
/// includes any nested sub-cells (metabolism.py nests cells). `_`-prefixed keys
/// — `_type` tags and bookkeeping — are skipped except to read an atom's role.
pub fn count_roles(cell: &Value) -> (f64, f64, f64) {
    let (mut f, mut phi, mut b) = (0.0, 0.0, 0.0);
    fn walk(v: &Value, f: &mut f64, phi: &mut f64, b: &mut f64) {
        match v.get_field("_type").and_then(|t| t.as_str()) {
            Some("F") => *f += 1.0,
            Some("Phi") => *phi += 1.0,
            Some("B") => *b += 1.0,
            _ => {}
        }
        if let Some(m) = v.as_map() {
            for (k, child) in m.iter() {
                if k.starts_with('_') {
                    continue;
                }
                walk(child, f, phi, b);
            }
        }
    }
    walk(cell, &mut f, &mut phi, &mut b);
    (f, phi, b)
}

/// A voice that sonifies one membrane's F/Φ/B blend at a fixed `pitch` (its
/// lineage). `gain` is the base level; `full` is the atom count mapped to full
/// loudness (so liveness fades a decaying cell). `pitch_cv` (V/oct) lets the
/// blend be transposed — patchable like every module.
#[derive(Clone, Debug)]
pub struct MembraneVoice {
    pub pitch_hz: f64,
    pub sample_rate: f64,
    pub block_size: usize,
    pub gain: f64,
    pub full: f64,
    pub fm_exp_depth: f64,
}

impl MembraneVoice {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            pitch_hz: cfg("pitch").unwrap_or(220.0),
            sample_rate: cfg("sample_rate").unwrap_or(48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            gain: cfg("gain").unwrap_or(0.3),
            full: cfg("full").unwrap_or(6.0).max(1.0),
            fm_exp_depth: cfg("fm_depth").unwrap_or(1.0),
        }
    }
}

impl Process for MembraneVoice {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("phase".to_string(), Schema::float()),
            // The membrane to sonify — a heterogeneous map of role-atoms.
            ("cell".to_string(), Schema::Any),
            // V/oct transpose — CV ≡ audio, patchable.
            ("fm_exp".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("phase".to_string(), Schema::overwrite(Schema::float())),
            ("out".to_string(), signal_type()),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let mut phase = state.get_field("phase").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let cell = state.get_field("cell").cloned().unwrap_or_else(Value::map);
        let fm_exp = cv_in(state, "fm_exp");

        let (f, phi, b) = count_roles(&cell);
        let total = f + phi + b;
        let n = self.block_size;
        let mut out = Vec::with_capacity(n);

        if total <= 0.0 {
            // No atoms — the cell is dead. Silence (and hold phase).
            return Update::value(Value::tree([
                ("phase", Value::float(phase)),
                ("out", signal_from_slice(&vec![0.0f32; n])),
            ]));
        }

        // Liveness → loudness: a full cell at `gain`, a near-empty one quiet.
        let amp = self.gain * (total / self.full).min(1.0);
        let (wf, wphi, wb) = (f / total, phi / total, b / total);

        for i in 0..n {
            let freq =
                modulated_freq(self.pitch_hz, at(&fm_exp, i) * self.fm_exp_depth, 0.0);
            let inc = freq / self.sample_rate;
            let frac = phase - phase.floor();

            let saw = 2.0 * frac - 1.0; // F — bright
            let tri = if frac < 0.5 { 4.0 * frac - 1.0 } else { 3.0 - 4.0 * frac }; // Φ — mellow
            let sub_frac = (phase * 0.5) - (phase * 0.5).floor();
            let sub = (TAU * sub_frac).sin(); // B — an octave-down sine (depth)

            let w = wf * saw + wphi * tri + wb * sub;
            out.push((amp * w) as f32);
            phase += inc;
        }

        let mut next_phase = phase.fract();
        if next_phase < 0.0 {
            next_phase += 1.0;
        }

        Update::value(Value::tree([
            ("phase", Value::float(next_phase)),
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

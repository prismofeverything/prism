//! `Svf` — a **multimode state-variable filter** (joranalogue Filter 8 lineage): one
//! kernel, four simultaneous outputs (`lp` / `hp` / `bp` / `notch`), and a resonance
//! that climbs to self-oscillation (so it doubles as a **resonator**). The topology is
//! the Cytomic / Andy Simper TPT (trapezoidal-integrated) SVF — stable under
//! audio-rate modulation, which matters because **cutoff and resonance are CV inputs**
//! (`cutoff_cv`, `res_cv` — CV ≡ audio), not frozen config.
//!
//! Two integrator states (`ic1`, `ic2`) carry across blocks on state slots. When the
//! cutoff/res CVs are unpatched the per-sample coefficients are constant, so the
//! common (knob-only) case stays cheap; a patched CV recomputes them per sample.

use std::any::Any;
use std::f64::consts::PI;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_to_vec, signal_type};

#[derive(Clone, Debug)]
pub struct Svf {
    /// Base cutoff in Hz (the knob); `cutoff_cv` modulates it exponentially.
    pub cutoff_hz: f64,
    /// Base resonance in `[0, 1)` — 0 is gentle (Q≈0.5), near 1 self-oscillates.
    pub resonance: f64,
    pub sample_rate: f64,
    pub block_size: usize,
    /// Exponential cutoff-CV depth — octaves per unit of `cutoff_cv` (default 1.0).
    pub cutoff_depth: f64,
    /// Resonance-CV depth — added to the base resonance per unit of `res_cv`.
    pub res_depth: f64,
}

impl Svf {
    pub fn new(cutoff_hz: f64, resonance: f64, sample_rate: f64, block_size: usize) -> Self {
        Self {
            cutoff_hz,
            resonance,
            sample_rate,
            block_size,
            cutoff_depth: 1.0,
            res_depth: 1.0,
        }
    }

    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            cutoff_hz: cfg("cutoff").unwrap_or(1_000.0),
            resonance: cfg("resonance").unwrap_or(0.0),
            sample_rate: cfg("sample_rate").unwrap_or(48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            cutoff_depth: cfg("cutoff_depth").unwrap_or(1.0),
            res_depth: cfg("res_depth").unwrap_or(1.0),
        }
    }

    /// The TPT-SVF coefficients `(g, k, a1, a2, a3)` for a cutoff + resonance. `k = 1/Q`
    /// maps resonance `[0,1)` to `[2, ~0.02]` (clamped off 0 so it rings but never blows
    /// up). `g = tan(π·fc/fs)` is the pre-warped integrator gain.
    fn coeffs(&self, cutoff_hz: f64, resonance: f64) -> (f64, f64, f64, f64) {
        let fc = cutoff_hz.clamp(10.0, self.sample_rate * 0.49);
        let g = (PI * fc / self.sample_rate).tan();
        let k = (2.0 - 1.98 * resonance.clamp(0.0, 1.0)).max(0.02);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        (k, a1, a2, a3)
    }
}

impl Process for Svf {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            // `input`, not `in` — `in` is a reserved word in the `.ys` surface, so the
            // audio-signal port a `.ys` patch wires must be a plain identifier.
            ("input".to_string(), signal_type()),
            ("ic1".to_string(), Schema::float()),
            ("ic2".to_string(), Schema::float()),
            ("cutoff_cv".to_string(), signal_type()),
            ("res_cv".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("lp".to_string(), signal_type()),
            ("hp".to_string(), signal_type()),
            ("bp".to_string(), signal_type()),
            ("notch".to_string(), signal_type()),
            // `out` aliases `lp` so a drop-in patch reading `out` gets a low-pass.
            ("out".to_string(), signal_type()),
            ("ic1".to_string(), Schema::overwrite(Schema::float())),
            ("ic2".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = state.get_field("input").map(signal_to_vec).unwrap_or_default();
        let mut ic1 = state.get_field("ic1").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut ic2 = state.get_field("ic2").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let cutoff_cv = cv_in(state, "cutoff_cv");
        let res_cv = cv_in(state, "res_cv");

        // Constant coefficients unless a CV modulates them (the cheap common path).
        let modulated = !cutoff_cv.is_empty() || !res_cv.is_empty();
        let (mut k, mut a1, mut a2, mut a3) = self.coeffs(self.cutoff_hz, self.resonance);

        let n = input.len().max(self.block_size);
        let (mut lp, mut hp, mut bp, mut notch, mut out) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );

        for i in 0..n {
            if modulated {
                let fc = self.cutoff_hz * 2.0_f64.powf(at(&cutoff_cv, i) * self.cutoff_depth);
                let res = self.resonance + at(&res_cv, i) * self.res_depth;
                let c = self.coeffs(fc, res);
                k = c.0;
                a1 = c.1;
                a2 = c.2;
                a3 = c.3;
            }
            let v0 = input.get(i).copied().unwrap_or(0.0) as f64;
            let v3 = v0 - ic2;
            let v1 = a1 * ic1 + a2 * v3;
            let v2 = ic2 + a2 * ic1 + a3 * v3;
            ic1 = 2.0 * v1 - ic1;
            ic2 = 2.0 * v2 - ic2;

            let s_lp = v2;
            let s_bp = v1;
            let s_hp = v0 - k * v1 - v2;
            let s_notch = s_lp + s_hp;

            lp.push(s_lp as f32);
            bp.push(s_bp as f32);
            hp.push(s_hp as f32);
            notch.push(s_notch as f32);
            out.push(s_lp as f32);
        }

        Update::value(Value::tree([
            ("lp", signal_from_slice(&lp)),
            ("hp", signal_from_slice(&hp)),
            ("bp", signal_from_slice(&bp)),
            ("notch", signal_from_slice(&notch)),
            ("out", signal_from_slice(&out)),
            ("ic1", Value::float(ic1)),
            ("ic2", Value::float(ic2)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

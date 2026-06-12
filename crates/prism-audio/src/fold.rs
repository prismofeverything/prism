//! `Fold` — a **wavefolder** (joranalogue Fold 6 / Serge wave multipliers / Buchla
//! timbre). West-Coast synthesis: instead of *subtracting* harmonics with a filter, a
//! folder *adds* them — drive a signal past ±1 and reflect it back on itself, and each
//! fold multiplies the spectrum. The `fold` drive and the `bias` (pre-fold offset, which
//! makes the folding asymmetric → even harmonics) are both CV inputs, so a slow envelope
//! on `fold_cv` is the classic evolving-timbre patch. Stateless — a pure shaper.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_to_vec, signal_type};

#[derive(Clone, Debug)]
pub struct Fold {
    /// Base drive into the folder (the knob); ≥1 starts folding. `fold_cv` adds.
    pub fold: f64,
    /// Pre-fold DC offset (asymmetry → even harmonics). `bias_cv` adds.
    pub bias: f64,
    pub fold_depth: f64,
    pub bias_depth: f64,
    pub block_size: usize,
}

impl Fold {
    pub fn new(fold: f64, block_size: usize) -> Self {
        Self {
            fold,
            bias: 0.0,
            fold_depth: 1.0,
            bias_depth: 1.0,
            block_size,
        }
    }

    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            fold: cfg("fold").unwrap_or(1.0),
            bias: cfg("bias").unwrap_or(0.0),
            fold_depth: cfg("fold_depth").unwrap_or(1.0),
            bias_depth: cfg("bias_depth").unwrap_or(1.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
        }
    }
}

/// Reflect `x` into `[-1, 1]` — an analytic triangle fold (a triangle wave of `x`, no
/// iteration). For `x ∈ [-1, 1]` it is the identity; beyond, it folds back repeatedly.
fn triangle_fold(x: f64) -> f64 {
    let p = (x + 1.0) * 0.25; // period-4 → triangle
    let t = p - (p + 0.5).floor(); // sawtooth in [-0.5, 0.5]
    4.0 * t.abs() - 1.0
}

impl Process for Fold {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("input".to_string(), signal_type()),
            ("fold_cv".to_string(), signal_type()),
            ("bias_cv".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("out".to_string(), signal_type())])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = state.get_field("input").map(signal_to_vec).unwrap_or_default();
        let fold_cv = cv_in(state, "fold_cv");
        let bias_cv = cv_in(state, "bias_cv");

        let out: Vec<f32> = (0..self.block_size)
            .map(|i| {
                let x = input.get(i).copied().unwrap_or(0.0) as f64;
                let drive = self.fold + at(&fold_cv, i) * self.fold_depth;
                let bias = self.bias + at(&bias_cv, i) * self.bias_depth;
                triangle_fold((x + bias) * drive) as f32
            })
            .collect();

        Update::value(Value::tree([("out", signal_from_slice(&out))]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

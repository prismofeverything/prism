//! `Compare` — a comparator + **analog logic** utility (joranalogue Compare 2 /
//! Schlappi Boundary). The comparator turns a continuous `Signal` into a gate
//! (`input ≥ threshold`), and the min/max/rectify outputs are the Serge-style
//! "analog Boolean" over two signals — the glue that makes a patch *decide*. Every
//! parameter is, of course, a CV: the `threshold` is `base + threshold_cv·depth`.
//!
//! Outputs (simultaneous): `gate` (1/0), `inv` (¬gate), `min`/`max` of `input` & `b`,
//! `rect` (full-wave |input|). Stateless — a pure per-sample function.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Compare {
    /// Base comparison threshold (the knob); `threshold_cv` moves it.
    pub threshold: f64,
    pub threshold_depth: f64,
    pub block_size: usize,
}

impl Compare {
    pub fn new(threshold: f64, block_size: usize) -> Self {
        Self {
            threshold,
            threshold_depth: 1.0,
            block_size,
        }
    }

    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            threshold: cfg("threshold").unwrap_or(0.0),
            threshold_depth: cfg("threshold_depth").unwrap_or(1.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
        }
    }
}

impl Process for Compare {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("input".to_string(), signal_type()),
            ("b".to_string(), signal_type()),
            ("threshold_cv".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("gate".to_string(), signal_type()),
            ("inv".to_string(), signal_type()),
            ("min".to_string(), signal_type()),
            ("max".to_string(), signal_type()),
            ("rect".to_string(), signal_type()),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = cv_in(state, "input");
        let b = cv_in(state, "b");
        let threshold_cv = cv_in(state, "threshold_cv");

        let n = self.block_size;
        let (mut gate, mut inv, mut min, mut max, mut rect) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );

        for i in 0..n {
            let a = at(&input, i);
            let bi = at(&b, i);
            let thr = self.threshold + at(&threshold_cv, i) * self.threshold_depth;
            let g = if a >= thr { 1.0 } else { 0.0 };
            gate.push(g as f32);
            inv.push((1.0 - g) as f32);
            min.push(a.min(bi) as f32);
            max.push(a.max(bi) as f32);
            rect.push(a.abs() as f32);
        }

        Update::value(Value::tree([
            ("gate", signal_from_slice(&gate)),
            ("inv", signal_from_slice(&inv)),
            ("min", signal_from_slice(&min)),
            ("max", signal_from_slice(&max)),
            ("rect", signal_from_slice(&rect)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

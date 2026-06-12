//! `Mix` — a 4-channel mixer with a **CV level per channel** (a VCA on each input, then
//! a sum). `out = master · Σ inₖ · (levelₖ + lvlₖ_cv)`. Each channel's level is a knob
//! (`l1..l4`, default unity) plus its CV input (`lvl1..lvl4`), so you can automate
//! levels, crossfade, or duck (CV ≡ audio — a level is just a Signal). Unpatched inputs
//! are silent and unpatched level CVs leave the knob, so you wire only the channels you
//! use. Stateless. (The bare additive `Signal` bus — many writers to one slot — is the
//! zero-module mixer; `Mix` adds the per-channel VCAs.)

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Mix {
    /// Per-channel base levels (the knobs).
    pub levels: [f64; 4],
    pub master: f64,
    pub block_size: usize,
}

impl Mix {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str, d: f64| config.get_field(k).and_then(|v| v.as_f64()).unwrap_or(d);
        Self {
            levels: [cfg("l1", 1.0), cfg("l2", 1.0), cfg("l3", 1.0), cfg("l4", 1.0)],
            master: cfg("master", 1.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
        }
    }
}

impl Process for Mix {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("in1".to_string(), signal_type()),
            ("in2".to_string(), signal_type()),
            ("in3".to_string(), signal_type()),
            ("in4".to_string(), signal_type()),
            ("lvl1".to_string(), signal_type()),
            ("lvl2".to_string(), signal_type()),
            ("lvl3".to_string(), signal_type()),
            ("lvl4".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("out".to_string(), signal_type())])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let ins = [
            cv_in(state, "in1"),
            cv_in(state, "in2"),
            cv_in(state, "in3"),
            cv_in(state, "in4"),
        ];
        let lvls = [
            cv_in(state, "lvl1"),
            cv_in(state, "lvl2"),
            cv_in(state, "lvl3"),
            cv_in(state, "lvl4"),
        ];

        let out: Vec<f32> = (0..self.block_size)
            .map(|i| {
                let mut acc = 0.0;
                for c in 0..4 {
                    acc += at(&ins[c], i) * (self.levels[c] + at(&lvls[c], i));
                }
                (acc * self.master) as f32
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

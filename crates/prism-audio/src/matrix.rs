//! `Matrix` — a 4-way morphing crossfader (joranalogue Morph 4): `morph` (0…3, plus
//! `morph_cv`) blends between `in1`..`in4` — at an integer `k` the output IS `inₖ`, in
//! between it crossfades. Sweep `morph_cv` to scan/blend sources (a modulation hub, a
//! wavetable-of-signals, a quad panner's front-end). Stateless.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Matrix {
    pub morph: f64,
    pub morph_depth: f64,
    pub block_size: usize,
}

impl Matrix {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str, d: f64| config.get_field(k).and_then(|v| v.as_f64()).unwrap_or(d);
        Self {
            morph: cfg("morph", 0.0),
            morph_depth: cfg("morph_depth", 1.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
        }
    }
}

impl Process for Matrix {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("in1".to_string(), signal_type()),
            ("in2".to_string(), signal_type()),
            ("in3".to_string(), signal_type()),
            ("in4".to_string(), signal_type()),
            ("morph_cv".to_string(), signal_type()),
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
        let morph_cv = cv_in(state, "morph_cv");

        let out: Vec<f32> = (0..self.block_size)
            .map(|i| {
                let m = (self.morph + at(&morph_cv, i) * self.morph_depth).clamp(0.0, 3.0);
                let i0 = m.floor() as usize;
                let i1 = (i0 + 1).min(3);
                let t = m - i0 as f64;
                (at(&ins[i0], i) * (1.0 - t) + at(&ins[i1], i) * t) as f32
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

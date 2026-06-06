//! `Vca` — a voltage-controlled amplifier: `out = in × gain`. Stateless.
//! `gain` is a control-rate input (one value per block, constant within the
//! block — k-rate). Kept a `Process` so the whole standard library ticks
//! uniformly at the block rate; a stateless node could equally be a `Step`.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::signal::{signal_from_slice, signal_to_vec, signal_type};

#[derive(Clone, Debug)]
pub struct Vca {
    pub sample_rate: f64,
    pub block_size: usize,
}

impl Vca {
    pub fn new(sample_rate: f64, block_size: usize) -> Self {
        Self {
            sample_rate,
            block_size,
        }
    }
}

impl Process for Vca {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("in".to_string(), signal_type()),
            ("gain".to_string(), Schema::float()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("out".to_string(), signal_type())])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = state.get_field("in").map(signal_to_vec).unwrap_or_default();
        let gain = state.get_field("gain").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let out: Vec<f32> = input.iter().map(|&x| x * gain).collect();
        Update::value(Value::tree([("out", signal_from_slice(&out))]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

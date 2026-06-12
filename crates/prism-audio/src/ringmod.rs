//! `RingMod` — a four-quadrant multiplier, `out = a · b`. Ring modulation: two inputs
//! multiply, producing the sum & difference frequencies (sidebands, not the originals)
//! — clangorous, metallic, bell-like. A **VCA is the two-quadrant case** (one input
//! unipolar); ring mod is the full bipolar product. Pure, stateless — the simplest
//! cross-modulator, and exactly the Schlappi/feedback-patch primitive. `a`/`b` are just
//! Signals (CV ≡ audio), so it rings audio×audio, audio×CV, or CV×CV alike.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct RingMod {
    pub block_size: usize,
}

impl RingMod {
    pub fn new(block_size: usize) -> Self {
        Self { block_size }
    }

    pub fn from_config(config: &Value) -> Self {
        Self {
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
        }
    }
}

impl Process for RingMod {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("a".to_string(), signal_type()),
            ("b".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("out".to_string(), signal_type())])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let a = cv_in(state, "a");
        let b = cv_in(state, "b");
        let out: Vec<f32> = (0..self.block_size)
            .map(|i| (at(&a, i) * at(&b, i)) as f32)
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

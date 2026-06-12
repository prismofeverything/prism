//! `ClockDiv` — divide a `clock` by `div`: a `trig` (and a half-period `gate`) every Nth
//! input pulse, with `reset`. (`Counter`'s bits give the power-of-2 divisions for free;
//! `ClockDiv` is an arbitrary ÷N — triplets, 5s, polymeter.) State: the pulse count.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in, rising_edge};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct ClockDiv {
    pub div: usize,
    pub block_size: usize,
    pub threshold: f64,
}

impl ClockDiv {
    pub fn from_config(config: &Value) -> Self {
        Self {
            div: config.get_field("div").and_then(|v| v.as_i64()).unwrap_or(2).max(1) as usize,
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            threshold: config.get_field("threshold").and_then(|v| v.as_f64()).unwrap_or(0.5),
        }
    }
}

impl Process for ClockDiv {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("clock".to_string(), signal_type()),
            ("reset".to_string(), signal_type()),
            ("count".to_string(), Schema::float()),
            ("clk_z".to_string(), Schema::float()),
            ("rst_z".to_string(), Schema::float()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("trig".to_string(), signal_type()),
            ("gate".to_string(), signal_type()),
            ("count".to_string(), Schema::overwrite(Schema::float())),
            ("clk_z".to_string(), Schema::overwrite(Schema::float())),
            ("rst_z".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let mut count = state.get_field("count").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
        let mut clk_z = state.get_field("clk_z").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut rst_z = state.get_field("rst_z").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let clock = cv_in(state, "clock");
        let reset = cv_in(state, "reset");
        let div = self.div as i64;

        let n = self.block_size;
        let (mut trig, mut gate) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let r = at(&reset, i);
            if rising_edge(rst_z, r, self.threshold) {
                count = 0;
            }
            rst_z = r;
            let c = at(&clock, i);
            let mut t = 0.0;
            if rising_edge(clk_z, c, self.threshold) {
                count += 1;
                if count >= div {
                    count = 0;
                    t = 1.0; // a divided pulse every Nth input pulse
                }
            }
            clk_z = c;
            trig.push(t as f32);
            gate.push(if count * 2 < div { 1.0 } else { 0.0 });
        }

        Update::value(Value::tree([
            ("trig", signal_from_slice(&trig)),
            ("gate", signal_from_slice(&gate)),
            ("count", Value::float(count as f64)),
            ("clk_z", Value::float(clk_z)),
            ("rst_z", Value::float(rst_z)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

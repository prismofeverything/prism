//! `Counter` — a 4-bit binary accumulator, the Schlappi **Nibbler**. A `clock` rising
//! edge increments a count 0→15 (wrapping); `reset` zeroes it. The idea (Schlappi's):
//! **counting in binary is inherently musical.** Each bit output is a clock division —
//! `b0` is clock/2, `b1` clock/4, `b2` clock/8, `b3` clock/16 — so the bits are an
//! instant rhythm section; and `cv` is the count as a stepped voltage (a 4-bit D/A
//! staircase), a melody/modulation when scanned. State: the count + the two edge levels.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in, rising_edge};
use crate::signal::{signal_from_slice, signal_type};

const BITS: u32 = 4;
const MODULO: i64 = 1 << BITS; // 16
const MAX: f64 = (MODULO - 1) as f64; // 15

#[derive(Clone, Debug)]
pub struct Counter {
    pub block_size: usize,
    pub threshold: f64,
}

impl Counter {
    pub fn new(block_size: usize) -> Self {
        Self {
            block_size,
            threshold: 0.5,
        }
    }

    pub fn from_config(config: &Value) -> Self {
        Self {
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            threshold: config.get_field("threshold").and_then(|v| v.as_f64()).unwrap_or(0.5),
        }
    }
}

impl Process for Counter {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("count".to_string(), Schema::float()),
            ("clk_z".to_string(), Schema::float()),
            ("rst_z".to_string(), Schema::float()),
            ("clock".to_string(), signal_type()),
            ("reset".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("cv".to_string(), signal_type()),
            ("b0".to_string(), signal_type()),
            ("b1".to_string(), signal_type()),
            ("b2".to_string(), signal_type()),
            ("b3".to_string(), signal_type()),
            ("count".to_string(), Schema::overwrite(Schema::float())),
            ("clk_z".to_string(), Schema::overwrite(Schema::float())),
            ("rst_z".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        // Integer state is stored as f64 (exact for these small ranges) so it matches the
        // `overwrite(float)` output schema and survives the engine apply across ticks.
        let mut count = state.get_field("count").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
        let mut clk_z = state.get_field("clk_z").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut rst_z = state.get_field("rst_z").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let clock = cv_in(state, "clock");
        let reset = cv_in(state, "reset");

        let n = self.block_size;
        let (mut cv, mut b0, mut b1, mut b2, mut b3) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );

        for i in 0..n {
            let r = at(&reset, i);
            if rising_edge(rst_z, r, self.threshold) {
                count = 0;
            }
            rst_z = r;
            let c = at(&clock, i);
            if rising_edge(clk_z, c, self.threshold) {
                count = (count + 1) % MODULO;
            }
            clk_z = c;

            cv.push((count as f64 / MAX) as f32);
            b0.push((count & 1) as f32);
            b1.push(((count >> 1) & 1) as f32);
            b2.push(((count >> 2) & 1) as f32);
            b3.push(((count >> 3) & 1) as f32);
        }

        Update::value(Value::tree([
            ("cv", signal_from_slice(&cv)),
            ("b0", signal_from_slice(&b0)),
            ("b1", signal_from_slice(&b1)),
            ("b2", signal_from_slice(&b2)),
            ("b3", signal_from_slice(&b3)),
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

//! `LowPass` — a one-pole low-pass filter: `y[n] = y[n-1] + a·(x[n] − y[n-1])`,
//! with `a = 1 − exp(−2π·cutoff/rate)`. The filter memory `z1` (the last
//! output sample) is carried across blocks on a state slot — the same
//! self-wired state pattern as the oscillator's phase. A stateful module is a
//! `Process` (it advances its own memory once per block); making it a `Step`
//! would risk self-retriggering on its `z1` output.

use std::any::Any;
use std::f64::consts::TAU;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::signal::{signal_from_slice, signal_to_vec, signal_type};

#[derive(Clone, Debug)]
pub struct LowPass {
    pub cutoff_hz: f64,
    pub sample_rate: f64,
    pub block_size: usize,
}

impl LowPass {
    pub fn new(cutoff_hz: f64, sample_rate: f64, block_size: usize) -> Self {
        Self {
            cutoff_hz,
            sample_rate,
            block_size,
        }
    }

    /// The one-pole smoothing coefficient in (0, 1).
    fn coeff(&self) -> f64 {
        1.0 - (-TAU * self.cutoff_hz / self.sample_rate).exp()
    }
}

impl Process for LowPass {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("in".to_string(), signal_type()),
            ("z1".to_string(), Schema::float()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("out".to_string(), signal_type()),
            ("z1".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = state.get_field("in").map(signal_to_vec).unwrap_or_default();
        let mut z = state.get_field("z1").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let a = self.coeff();

        let out: Vec<f32> = input
            .iter()
            .map(|&x| {
                z += a * (x as f64 - z);
                z as f32
            })
            .collect();

        Update::value(Value::tree([
            ("out", signal_from_slice(&out)),
            ("z1", Value::float(z)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

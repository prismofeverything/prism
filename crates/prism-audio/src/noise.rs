//! `Noise` — a white-noise source. Deterministic (a seeded `xorshift` whose state
//! lives on a slot, so a render is reproducible), full-band `[-1, 1]`. Patch it into a
//! [`SampleHold`](crate::samplehold)'s `input` with a clock on `trigger` and you have
//! the classic **random-voltage** generator (the Serge SSG / Turing-machine staple);
//! patch it straight to a VCA for percussion/breath. Amplitude is a CV (`am`) like
//! every other parameter.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_type};

const DEFAULT_SEED: u32 = 0x2545_F491;

#[derive(Clone, Debug)]
pub struct Noise {
    pub block_size: usize,
    pub amplitude: f64,
    pub am_depth: f64,
}

impl Noise {
    pub fn new(block_size: usize) -> Self {
        Self {
            block_size,
            amplitude: 1.0,
            am_depth: 1.0,
        }
    }

    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            amplitude: cfg("amplitude").unwrap_or(1.0),
            am_depth: cfg("am_depth").unwrap_or(1.0),
        }
    }
}

impl Process for Noise {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("seed".to_string(), Schema::float()),
            ("am".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("out".to_string(), signal_type()),
            ("seed".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        // The RNG state rides a slot (an i64); 0 (an unseeded slot) takes the default.
        let mut seed = state.get_field("seed").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
        if seed == 0 {
            seed = DEFAULT_SEED;
        }
        let am = cv_in(state, "am");

        let mut out = Vec::with_capacity(self.block_size);
        for i in 0..self.block_size {
            // xorshift32 — cheap, full-period, deterministic.
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let r = (seed as f64 / u32::MAX as f64) * 2.0 - 1.0; // [-1, 1]
            let amp = self.amplitude + at(&am, i) * self.am_depth;
            out.push((amp * r) as f32);
        }

        Update::value(Value::tree([
            ("out", signal_from_slice(&out)),
            ("seed", Value::Int(seed as i64)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

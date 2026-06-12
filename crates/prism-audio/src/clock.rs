//! `Clock` — a master clock: a free-running pulse train at `rate` Hz (tuned in octaves
//! by `rate_cv` — V/oct tempo), with a per-cycle `trig` (a one-sample pulse) and a `gate`
//! (high for `width` of each cycle). `reset` restarts the phase. Patch `trig` to advance
//! a `Sequencer`/`Counter`, `gate` to a `Lpg`/envelope. (A cycling `Slope` clocks too;
//! `Clock` is the dedicated, square, V/oct-tunable one + clean dividers via `Counter`.)
//! State: the phase.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in, rising_edge};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Clock {
    pub rate_hz: f64,
    pub width: f64,
    pub sample_rate: f64,
    pub block_size: usize,
    pub rate_depth: f64,
    pub threshold: f64,
}

impl Clock {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            rate_hz: cfg("rate").unwrap_or(2.0),
            width: cfg("width").unwrap_or(0.5),
            sample_rate: cfg("sample_rate").unwrap_or(48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            rate_depth: cfg("rate_depth").unwrap_or(1.0),
            threshold: cfg("threshold").unwrap_or(0.5),
        }
    }
}

impl Process for Clock {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("phase".to_string(), Schema::float()),
            ("rst_z".to_string(), Schema::float()),
            ("rate_cv".to_string(), signal_type()),
            ("reset".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("gate".to_string(), signal_type()),
            ("trig".to_string(), signal_type()),
            ("phase".to_string(), Schema::overwrite(Schema::float())),
            ("rst_z".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let mut phase = state.get_field("phase").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut rst_z = state.get_field("rst_z").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let rate_cv = cv_in(state, "rate_cv");
        let reset = cv_in(state, "reset");

        let n = self.block_size;
        let (mut gate, mut trig) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let r = at(&reset, i);
            if rising_edge(rst_z, r, self.threshold) {
                phase = 0.0;
            }
            rst_z = r;

            let rate = self.rate_hz * 2.0_f64.powf(at(&rate_cv, i) * self.rate_depth);
            phase += rate / self.sample_rate;
            let mut t = 0.0;
            if phase >= 1.0 {
                phase -= 1.0;
                t = 1.0; // one-sample pulse per cycle
            }
            gate.push(if phase < self.width { 1.0 } else { 0.0 });
            trig.push(t as f32);
        }

        Update::value(Value::tree([
            ("gate", signal_from_slice(&gate)),
            ("trig", signal_from_slice(&trig)),
            ("phase", Value::float(phase)),
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

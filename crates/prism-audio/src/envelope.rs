//! `Envelope` — a gated attack/release (AR) control envelope. Control-rate:
//! one `level` per block, a linear ramp toward 1 while `gate > 0.5` (attack)
//! and toward 0 otherwise (release). The `level` is carried on a self-wired
//! state slot. This is the canonical k-rate modulator — its output drives a
//! `Vca`'s gain to shape a voice. (A full ADSR extends this with decay +
//! sustain stages; AR is the honest minimal form for A2.)

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

#[derive(Clone, Debug)]
pub struct Envelope {
    pub attack_s: f64,
    pub release_s: f64,
    pub sample_rate: f64,
    pub block_size: usize,
}

impl Envelope {
    pub fn new(attack_s: f64, release_s: f64, sample_rate: f64, block_size: usize) -> Self {
        Self {
            attack_s,
            release_s,
            sample_rate,
            block_size,
        }
    }

    /// Build from a patch node's `config` map.
    pub fn from_config(config: &Value) -> Self {
        Self {
            attack_s: config.get_field("attack").and_then(|v| v.as_f64()).unwrap_or(0.01),
            release_s: config.get_field("release").and_then(|v| v.as_f64()).unwrap_or(0.1),
            sample_rate: config.get_field("sample_rate").and_then(|v| v.as_f64()).unwrap_or(48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
        }
    }
}

impl Process for Envelope {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("gate".to_string(), Schema::float()),
            ("level".to_string(), Schema::float()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("level".to_string(), Schema::overwrite(Schema::float()))])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let gate = state.get_field("gate").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut level = state.get_field("level").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let dt = self.block_size as f64 / self.sample_rate; // seconds per block

        if gate > 0.5 {
            level += dt / self.attack_s.max(1e-6);
            level = level.min(1.0);
        } else {
            level -= dt / self.release_s.max(1e-6);
            level = level.max(0.0);
        }

        Update::value(Value::tree([("level", Value::float(level))]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

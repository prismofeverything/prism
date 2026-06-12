//! `Ladder` — a 4-pole **Moog transistor-ladder** low-pass: the warm, resonant classic,
//! the other great filter color beside the clean [`Svf`](crate::svf). Four one-pole
//! stages in series with global feedback from the 4th pole (resonance → self-oscillation
//! near the top) and a `tanh` drive on the feedback for the Moog saturation. `cutoff_cv`
//! (V/oct) and `res_cv` are modulation inputs like everything else (CV ≡ audio). State:
//! the four pole memories `y1..y4`.

use std::any::Any;
use std::f64::consts::TAU;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_to_vec, signal_type};

#[derive(Clone, Debug)]
pub struct Ladder {
    pub cutoff_hz: f64,
    /// `[0,1]` — 0 is no resonance, ~1 self-oscillates.
    pub resonance: f64,
    pub sample_rate: f64,
    pub block_size: usize,
    pub cutoff_depth: f64,
    pub res_depth: f64,
}

impl Ladder {
    pub fn new(cutoff_hz: f64, resonance: f64, sample_rate: f64, block_size: usize) -> Self {
        Self {
            cutoff_hz,
            resonance,
            sample_rate,
            block_size,
            cutoff_depth: 1.0,
            res_depth: 1.0,
        }
    }

    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            cutoff_hz: cfg("cutoff").unwrap_or(1_000.0),
            resonance: cfg("resonance").unwrap_or(0.0),
            sample_rate: cfg("sample_rate").unwrap_or(48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            cutoff_depth: cfg("cutoff_depth").unwrap_or(1.0),
            res_depth: cfg("res_depth").unwrap_or(1.0),
        }
    }
}

impl Process for Ladder {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("input".to_string(), signal_type()),
            ("y1".to_string(), Schema::float()),
            ("y2".to_string(), Schema::float()),
            ("y3".to_string(), Schema::float()),
            ("y4".to_string(), Schema::float()),
            ("cutoff_cv".to_string(), signal_type()),
            ("res_cv".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("out".to_string(), signal_type()),  // the 4-pole low-pass
            ("lp2".to_string(), signal_type()),  // a 2-pole tap (gentler slope)
            ("y1".to_string(), Schema::overwrite(Schema::float())),
            ("y2".to_string(), Schema::overwrite(Schema::float())),
            ("y3".to_string(), Schema::overwrite(Schema::float())),
            ("y4".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = state.get_field("input").map(signal_to_vec).unwrap_or_default();
        let f = |k: &str| state.get_field(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let (mut y1, mut y2, mut y3, mut y4) = (f("y1"), f("y2"), f("y3"), f("y4"));
        let cutoff_cv = cv_in(state, "cutoff_cv");
        let res_cv = cv_in(state, "res_cv");

        let n = self.block_size;
        let (mut out, mut lp2) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let fc = (self.cutoff_hz * 2.0_f64.powf(at(&cutoff_cv, i) * self.cutoff_depth))
                .clamp(10.0, self.sample_rate * 0.45);
            let g = 1.0 - (-TAU * fc / self.sample_rate).exp();
            // resonance → feedback amount; <4 stays stable, ~3.9 self-oscillates.
            let k = (self.resonance + at(&res_cv, i) * self.res_depth).clamp(0.0, 1.0) * 3.95;

            let x = input.get(i).copied().unwrap_or(0.0) as f64;
            let fb = x - k * y4;
            // tanh on the feedback path = the Moog drive/character.
            y1 += g * (fb.tanh() - y1);
            y2 += g * (y1 - y2);
            y3 += g * (y2 - y3);
            y4 += g * (y3 - y4);
            out.push(y4 as f32);
            lp2.push(y2 as f32);
        }

        Update::value(Value::tree([
            ("out", signal_from_slice(&out)),
            ("lp2", signal_from_slice(&lp2)),
            ("y1", Value::float(y1)),
            ("y2", Value::float(y2)),
            ("y3", Value::float(y3)),
            ("y4", Value::float(y4)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

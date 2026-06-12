//! `Sequencer` — a clocked step sequencer (joranalogue Step 8). A `clock` rising edge
//! advances through a list of `steps` (looping); `reset` returns to step 0. Outputs the
//! current step as a held `cv` (a staircase — patch it to `fm_exp` for a melody, or to
//! any CV input for a rhythmic modulation) and a `trig` pulse on each advance (patch to
//! an envelope's `trigger`). The `steps` are a `.ys` list — the sequence is data, so a
//! reaction could rewrite it live (the homoiconic angle). State: the step index + the
//! clock/reset edge levels.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in, rising_edge};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Sequencer {
    /// The step values (a `.ys` list); `cv` outputs `steps[index]`.
    pub steps: Vec<f64>,
    pub block_size: usize,
    pub threshold: f64,
}

impl Sequencer {
    pub fn new(steps: Vec<f64>, block_size: usize) -> Self {
        Self {
            steps,
            block_size,
            threshold: 0.5,
        }
    }

    pub fn from_config(config: &Value) -> Self {
        let steps = match config.get_field("steps") {
            Some(Value::List(items)) => items.iter().filter_map(|v| v.as_f64()).collect(),
            _ => Vec::new(),
        };
        let steps = if steps.is_empty() {
            // A short rising staircase by default (quarter-octave steps).
            vec![0.0, 0.25, 0.5, 0.75]
        } else {
            steps
        };
        Self {
            steps,
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            threshold: config.get_field("threshold").and_then(|v| v.as_f64()).unwrap_or(0.5),
        }
    }
}

impl Process for Sequencer {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("index".to_string(), Schema::float()),
            ("clk_z".to_string(), Schema::float()),
            ("rst_z".to_string(), Schema::float()),
            ("clock".to_string(), signal_type()),
            ("reset".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("cv".to_string(), signal_type()),
            ("trig".to_string(), signal_type()),
            ("index".to_string(), Schema::overwrite(Schema::float())),
            ("clk_z".to_string(), Schema::overwrite(Schema::float())),
            ("rst_z".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let len = self.steps.len().max(1) as i64;
        let mut index = state.get_field("index").and_then(|v| v.as_i64()).unwrap_or(0);
        let mut clk_z = state.get_field("clk_z").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut rst_z = state.get_field("rst_z").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let clock = cv_in(state, "clock");
        let reset = cv_in(state, "reset");

        let n = self.block_size;
        let (mut cv, mut trig) = (Vec::with_capacity(n), Vec::with_capacity(n));

        for i in 0..n {
            let r = at(&reset, i);
            if rising_edge(rst_z, r, self.threshold) {
                index = 0;
            }
            rst_z = r;
            let c = at(&clock, i);
            let mut advanced = 0.0;
            if rising_edge(clk_z, c, self.threshold) {
                index = (index + 1) % len;
                advanced = 1.0; // a one-sample trigger on each step change
            }
            clk_z = c;

            cv.push(self.steps[index as usize % self.steps.len().max(1)] as f32);
            trig.push(advanced as f32);
        }

        Update::value(Value::tree([
            ("cv", signal_from_slice(&cv)),
            ("trig", signal_from_slice(&trig)),
            ("index", Value::Int(index)),
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

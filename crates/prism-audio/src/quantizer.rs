//! `Quantizer` — snap a continuous pitch CV to a musical **scale**, so wandering /
//! random / sequenced CVs land in-key. `input` is pitch in octaves (V/oct, 1.0 = +1
//! octave); `scale` is a `.ys` list of semitone degrees within the octave (default
//! major). It finds the nearest scale degree (across the octave boundary too) and emits
//! the quantized pitch on `out`, plus a one-sample `gate` pulse whenever the note
//! CHANGES — patch that to an envelope `trigger` for "play a note on each new pitch."
//! State: the last quantized semitone (for change detection).

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Quantizer {
    /// Scale degrees in semitones within `[0, 12)` (e.g. major = `[0,2,4,5,7,9,11]`).
    pub scale: Vec<f64>,
    pub block_size: usize,
}

impl Quantizer {
    pub fn new(scale: Vec<f64>, block_size: usize) -> Self {
        Self { scale, block_size }
    }

    pub fn from_config(config: &Value) -> Self {
        let scale = match config.get_field("scale") {
            Some(Value::List(items)) => items.iter().filter_map(|v| v.as_f64()).collect(),
            _ => Vec::new(),
        };
        let scale = if scale.is_empty() {
            vec![0.0, 2.0, 4.0, 5.0, 7.0, 9.0, 11.0] // major
        } else {
            scale
        };
        Self {
            scale,
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
        }
    }

    /// Snap a semitone-within-octave to the nearest scale degree (considering the next
    /// octave's root so values just below 12 wrap up correctly).
    fn nearest(&self, within: f64) -> f64 {
        let mut best = self.scale[0];
        let mut best_d = f64::INFINITY;
        for &deg in self.scale.iter().chain(std::iter::once(&(self.scale[0] + 12.0))) {
            let d = (within - deg).abs();
            if d < best_d {
                best_d = d;
                best = deg;
            }
        }
        best
    }
}

impl Process for Quantizer {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("input".to_string(), signal_type()),
            ("last".to_string(), Schema::float()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("out".to_string(), signal_type()),
            ("gate".to_string(), signal_type()),
            ("last".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = cv_in(state, "input");
        let mut last = state.get_field("last").and_then(|v| v.as_f64()).unwrap_or(f64::NAN);

        let n = self.block_size;
        let (mut out, mut gate) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let semis = at(&input, i) * 12.0;
            let oct = semis.div_euclid(12.0);
            let within = semis - oct * 12.0;
            let q_semis = oct * 12.0 + self.nearest(within);
            out.push((q_semis / 12.0) as f32);
            // a one-sample pulse when the quantized note changes
            gate.push(if q_semis != last { 1.0 } else { 0.0 });
            last = q_semis;
        }

        Update::value(Value::tree([
            ("out", signal_from_slice(&out)),
            ("gate", signal_from_slice(&gate)),
            ("last", Value::float(last)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

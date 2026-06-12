//! `Pan` — equal-power stereo placement: `pan` (−1 hard-left … +1 hard-right, plus
//! `pan_cv`) splits `input` into `left` and `right` using a cosine/sine law (constant
//! perceived loudness across the field). Patch an LFO or `Chaos` into `pan_cv` for
//! autopan / spatial movement. Stateless. (Full stereo *playback* is a later `AudioOut`
//! channel extension; today `left`/`right` are two Signals to use or sum.)

use std::any::Any;
use std::f64::consts::FRAC_PI_2;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Pan {
    pub pan: f64,
    pub pan_depth: f64,
    pub block_size: usize,
}

impl Pan {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str, d: f64| config.get_field(k).and_then(|v| v.as_f64()).unwrap_or(d);
        Self {
            pan: cfg("pan", 0.0),
            pan_depth: cfg("pan_depth", 1.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
        }
    }
}

impl Process for Pan {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("input".to_string(), signal_type()),
            ("pan_cv".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("left".to_string(), signal_type()),
            ("right".to_string(), signal_type()),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = cv_in(state, "input");
        let pan_cv = cv_in(state, "pan_cv");
        let n = self.block_size;
        let (mut left, mut right) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let p = (self.pan + at(&pan_cv, i) * self.pan_depth).clamp(-1.0, 1.0);
            let angle = (p + 1.0) * 0.5 * FRAC_PI_2; // 0..π/2
            let x = at(&input, i);
            left.push((x * angle.cos()) as f32);
            right.push((x * angle.sin()) as f32);
        }
        Update::value(Value::tree([
            ("left", signal_from_slice(&left)),
            ("right", signal_from_slice(&right)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

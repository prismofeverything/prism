//! `Shaper` — the distortion/waveshaper family: `drive_cv` sets the gain into the shaper,
//! and three shapes come out at once — `rect` (full-wave |x|, an octave-up timbre), `clip`
//! (hard ±1), `drive` (a `tanh` soft-clip, warm overdrive). (`Fold` is the *wrapping*
//! shaper; this is the *saturating* one.) Stateless — a pure per-sample function.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Shaper {
    pub drive: f64,
    pub drive_depth: f64,
    pub block_size: usize,
}

impl Shaper {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str, d: f64| config.get_field(k).and_then(|v| v.as_f64()).unwrap_or(d);
        Self {
            drive: cfg("drive", 1.0),
            drive_depth: cfg("drive_depth", 1.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
        }
    }
}

impl Process for Shaper {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("input".to_string(), signal_type()),
            ("drive_cv".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("out".to_string(), signal_type()), // = drive (the most common)
            ("rect".to_string(), signal_type()),
            ("clip".to_string(), signal_type()),
            ("drive".to_string(), signal_type()),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = cv_in(state, "input");
        let drive_cv = cv_in(state, "drive_cv");
        let n = self.block_size;
        let (mut rect, mut clip, mut drive) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        for i in 0..n {
            let g = self.drive + at(&drive_cv, i) * self.drive_depth;
            let x = at(&input, i) * g;
            rect.push((x.abs() * 2.0 - 1.0) as f32); // |x| recentred to bipolar
            clip.push(x.clamp(-1.0, 1.0) as f32);
            drive.push(x.tanh() as f32);
        }
        Update::value(Value::tree([
            ("out", signal_from_slice(&drive)),
            ("rect", signal_from_slice(&rect)),
            ("clip", signal_from_slice(&clip)),
            ("drive", signal_from_slice(&drive)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

//! `Delay` — a delay line with feedback and a wet/dry mix: echo, and (with `time_cv`
//! modulating the read position) the basis of chorus / flanger. The basis of reverb too
//! (an FDN is a few `Delay`s). `time` (s), `feedback`, and `mix` are all CV inputs.
//!
//! Implementation note: the delay BUFFER does not live in the engine state (cloning a
//! ~1 s buffer every tick is wasteful) — it is held on the struct behind a `Mutex` and
//! mutated through `&self` (the same move `AudioOut` makes with its ring; `Process` needs
//! no `Clone`). Each patch node gets its own buffer via `from_config`. The read position
//! is fractional (linearly interpolated), so a modulated `time_cv` glides smoothly.

use std::any::Any;
use std::sync::Mutex;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_to_vec, signal_type};

pub struct Delay {
    time: f64,
    feedback: f64,
    mix: f64,
    sample_rate: f64,
    block_size: usize,
    time_depth: f64,
    fb_depth: f64,
    mix_depth: f64,
    /// `(ring buffer, write index)` — the delay line, owned per node.
    buf: Mutex<(Vec<f32>, usize)>,
}

impl Delay {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str, d: f64| config.get_field(k).and_then(|v| v.as_f64()).unwrap_or(d);
        let sample_rate = cfg("sample_rate", 48_000.0);
        let max = cfg("max", 1.0).max(0.001); // max delay (s) → buffer size
        let len = ((max * sample_rate) as usize).max(1);
        Self {
            time: cfg("time", 0.25),
            feedback: cfg("feedback", 0.4),
            mix: cfg("mix", 0.4),
            sample_rate,
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            time_depth: cfg("time_depth", 1.0),
            fb_depth: cfg("fb_depth", 1.0),
            mix_depth: cfg("mix_depth", 1.0),
            buf: Mutex::new((vec![0.0; len], 0)),
        }
    }
}

impl Process for Delay {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("input".to_string(), signal_type()),
            ("time_cv".to_string(), signal_type()),
            ("feedback_cv".to_string(), signal_type()),
            ("mix_cv".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("out".to_string(), signal_type())])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = state.get_field("input").map(signal_to_vec).unwrap_or_default();
        let time_cv = cv_in(state, "time_cv");
        let feedback_cv = cv_in(state, "feedback_cv");
        let mix_cv = cv_in(state, "mix_cv");

        let mut guard = self.buf.lock().expect("Delay buffer poisoned");
        let (buf, w) = &mut *guard;
        let len = buf.len();
        let max_t = (len as f64 - 2.0) / self.sample_rate;

        let mut out = Vec::with_capacity(self.block_size);
        for i in 0..self.block_size {
            let x = input.get(i).copied().unwrap_or(0.0) as f64;
            let delay_s = (self.time + at(&time_cv, i) * self.time_depth).clamp(0.0, max_t);
            let delay_n = delay_s * self.sample_rate;
            // Fractional read at (write - delay), linearly interpolated.
            let read = (*w as f64 - delay_n).rem_euclid(len as f64);
            let i0 = read.floor() as usize % len;
            let i1 = (i0 + 1) % len;
            let frac = read - read.floor();
            let delayed = buf[i0] as f64 * (1.0 - frac) + buf[i1] as f64 * frac;

            let fb = (self.feedback + at(&feedback_cv, i) * self.fb_depth).clamp(0.0, 0.99);
            buf[*w] = (x + delayed * fb) as f32;
            *w = (*w + 1) % len;

            let mix = (self.mix + at(&mix_cv, i) * self.mix_depth).clamp(0.0, 1.0);
            out.push((x * (1.0 - mix) + delayed * mix) as f32);
        }

        Update::value(Value::tree([("out", signal_from_slice(&out))]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl std::fmt::Debug for Delay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Delay")
            .field("time", &self.time)
            .field("feedback", &self.feedback)
            .field("mix", &self.mix)
            .finish()
    }
}

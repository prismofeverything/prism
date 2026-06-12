//! `Comb` — a tuned feedback comb filter = a **Karplus-Strong string**. A delay line
//! exactly one pitch-period long, fed back through a damping low-pass: excite it with a
//! burst (a click of `Noise` gated by a fast envelope) on `input` and it rings like a
//! plucked string. `pitch_cv` (V/oct) tunes the period, `feedback_cv` the sustain,
//! `damping` the brightness/decay. Physical modeling from a delay + a one-pole. The
//! buffer lives on the struct (a `Mutex`, like [`Delay`](crate::delay)) — no
//! clone-per-tick; the read is fractional (interpolated) so pitch CV glides.

use std::any::Any;
use std::sync::Mutex;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_to_vec, signal_type};

pub struct Comb {
    pitch_hz: f64,
    feedback: f64,
    damping: f64,
    sample_rate: f64,
    block_size: usize,
    pitch_depth: f64,
    fb_depth: f64,
    /// `(buffer, write index, damping one-pole state)`.
    buf: Mutex<(Vec<f32>, usize, f64)>,
}

impl Comb {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str, d: f64| config.get_field(k).and_then(|v| v.as_f64()).unwrap_or(d);
        let sample_rate = cfg("sample_rate", 48_000.0);
        // Buffer must hold the LONGEST period (the lowest pitch we allow ≈ 20 Hz).
        let len = ((sample_rate / 20.0) as usize + 4).max(4);
        Self {
            pitch_hz: cfg("pitch", 220.0),
            feedback: cfg("feedback", 0.99),
            damping: cfg("damping", 0.5),
            sample_rate,
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            pitch_depth: cfg("pitch_depth", 1.0),
            fb_depth: cfg("fb_depth", 1.0),
            buf: Mutex::new((vec![0.0; len], 0, 0.0)),
        }
    }
}

impl Process for Comb {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("input".to_string(), signal_type()),
            ("pitch_cv".to_string(), signal_type()),
            ("feedback_cv".to_string(), signal_type()),
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
        let pitch_cv = cv_in(state, "pitch_cv");
        let feedback_cv = cv_in(state, "feedback_cv");

        let mut guard = self.buf.lock().expect("Comb buffer poisoned");
        let (buf, w, z) = &mut *guard;
        let len = buf.len();

        let mut out = Vec::with_capacity(self.block_size);
        for i in 0..self.block_size {
            let pitch = (self.pitch_hz * 2.0_f64.powf(at(&pitch_cv, i) * self.pitch_depth))
                .clamp(20.0, self.sample_rate * 0.45);
            let delay_n = (self.sample_rate / pitch).min(len as f64 - 2.0);
            let read = (*w as f64 - delay_n).rem_euclid(len as f64);
            let i0 = read.floor() as usize % len;
            let i1 = (i0 + 1) % len;
            let frac = read - read.floor();
            let delayed = buf[i0] as f64 * (1.0 - frac) + buf[i1] as f64 * frac;

            // One-pole damping low-pass in the feedback loop (string brightness/decay).
            *z += (1.0 - self.damping) * (delayed - *z);
            let fb = (self.feedback + at(&feedback_cv, i) * self.fb_depth).clamp(0.0, 0.999);
            let x = input.get(i).copied().unwrap_or(0.0) as f64;
            buf[*w] = (x + *z * fb) as f32;
            *w = (*w + 1) % len;
            out.push(delayed as f32);
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

impl std::fmt::Debug for Comb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Comb").field("pitch", &self.pitch_hz).finish()
    }
}

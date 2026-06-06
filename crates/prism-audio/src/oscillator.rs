//! `Oscillator` — a waveform generator. One audio block per tick (block-rate
//! scheduling: `interval = block / sample_rate`, so logical time advances at
//! exactly audio time — `engine.run(1.0)` renders one second).
//!
//! Phase is carried across blocks on a **state slot**, replaced each tick
//! (the module owns its own phase). A prism process is `&self` during
//! `update`, so all evolving state must live in the engine's state tree, never
//! on the struct — exactly as `grow.ys` carries `mass` on its self-wire.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::signal::{signal_from_slice, signal_schema};
use crate::wave::Wave;

/// A waveform oscillator. `freq_hz`, `sample_rate`, and `block_size` are
/// construction-time config (constant for the life of the module); `phase` is
/// runtime state read from / written to the state tree. In A4 `freq` becomes
/// an input port so it can be modulated (FM, pitch CV); for A1 it is config.
#[derive(Clone, Debug)]
pub struct Oscillator {
    pub wave: Wave,
    pub freq_hz: f64,
    pub sample_rate: f64,
    pub block_size: usize,
    pub amplitude: f64,
}

impl Oscillator {
    pub fn new(wave: Wave, freq_hz: f64, sample_rate: f64, block_size: usize) -> Self {
        Self {
            wave,
            freq_hz,
            sample_rate,
            block_size,
            amplitude: 1.0,
        }
    }

    pub fn with_amplitude(mut self, amplitude: f64) -> Self {
        self.amplitude = amplitude;
        self
    }
}

impl Process for Oscillator {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("phase".to_string(), Schema::float())])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("phase".to_string(), Schema::overwrite(Schema::float())),
            (
                "out".to_string(),
                Schema::overwrite(signal_schema(self.block_size)),
            ),
        ])
    }

    /// One block of audio per tick: `block / sample_rate` seconds of logical
    /// time. Reading the wired `interval` is the engine's job; this is the
    /// module's natural rate.
    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let phase = state
            .get_field("phase")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let inc = self.freq_hz / self.sample_rate; // cycles per sample

        let mut block = Vec::with_capacity(self.block_size);
        for i in 0..self.block_size {
            let p = phase + (i as f64) * inc;
            block.push((self.amplitude * self.wave.sample(p)) as f32);
        }

        // The absolute phase at the next block, wrapped to [0, 1) so it never
        // loses float precision over a long render.
        let mut next_phase = (phase + (self.block_size as f64) * inc).fract();
        if next_phase < 0.0 {
            next_phase += 1.0;
        }

        Update::value(Value::tree([
            ("phase", Value::float(next_phase)),
            ("out", signal_from_slice(&block)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

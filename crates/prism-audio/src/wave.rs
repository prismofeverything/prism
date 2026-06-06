//! Waveform kernels — pure functions from phase (in cycles) to a sample in
//! `[-1, 1]`. These are the native DSP that the `.ys` surface will eventually
//! call as value methods: chrysalis stays a thin layer, the math lives here
//! (synthesis-bigraphs.md §V; `feedback_chrysalis_thin_layer`).

use std::f64::consts::TAU;

/// A band-unlimited waveform shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Sine,
    Saw,
    Square,
    Triangle,
}

impl Wave {
    /// Parse a waveform name (anything unrecognized defaults to `Sine`).
    pub fn parse(name: &str) -> Wave {
        match name {
            "Saw" | "saw" => Wave::Saw,
            "Square" | "square" => Wave::Square,
            "Triangle" | "triangle" => Wave::Triangle,
            _ => Wave::Sine,
        }
    }

    /// Sample the waveform at `phase` cycles (any real; the integer part is
    /// ignored, so phase may run unwrapped within a block). Returns `[-1, 1]`.
    pub fn sample(self, phase: f64) -> f64 {
        let frac = phase - phase.floor(); // [0, 1)
        match self {
            Wave::Sine => (TAU * frac).sin(),
            Wave::Saw => 2.0 * frac - 1.0,
            Wave::Square => {
                if frac < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Triangle => {
                if frac < 0.5 {
                    4.0 * frac - 1.0
                } else {
                    3.0 - 4.0 * frac
                }
            }
        }
    }
}

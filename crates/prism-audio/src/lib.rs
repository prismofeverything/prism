//! prism-audio — the audio / modular-synthesis layer for prism.
//!
//! Design: `docs/synthesis-bigraphs.md`. A synthesizer patch IS a bigraph —
//! a module is a process with signal ports, a patch cable is a link, a voice
//! is a composite — so generative synthesis is a BRS rewriting that bigraph.
//!
//! **A1 (this slice)** is the substrate, proven *offline* in logical time:
//! the [`Signal`](signal) block value, an [`Oscillator`](oscillator) process,
//! and offline [`render`](render)ing to a buffer / WAV — no audio device, no
//! wall-clock, fully deterministic under `cargo test` (synthesis-bigraphs.md
//! §II/§X). Later slices add the realtime device boundary (gorgon's lock-free
//! ring, A3) and the distributed `net:` protocol (A8); the *patch* is
//! unchanged — only the sink differs.

pub mod oscillator;
pub mod render;
pub mod signal;
pub mod wav;
pub mod wave;

pub use oscillator::Oscillator;
pub use render::{render, render_kernel, render_oscillator};
pub use signal::{signal_from_slice, signal_schema, signal_to_vec, silence};
pub use wav::write_wav_i16;
pub use wave::Wave;

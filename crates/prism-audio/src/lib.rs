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

pub mod envelope;
pub mod lowpass;
pub mod oscillator;
pub mod patch;
pub mod reactions;
pub mod render;
pub mod signal;
pub mod vca;
pub mod voice;
pub mod wav;
pub mod wave;

pub use envelope::Envelope;
pub use lowpass::LowPass;
pub use oscillator::Oscillator;
pub use patch::{module_node, register_audio, render_patch, wire_map};
pub use reactions::{
    detune_voice, patch_brs_node, prune_oscillator, render_patch_brs, spawn_voice, PATCH_BRS,
};
pub use render::{render, render_kernel, render_mix, render_oscillator, render_path, render_voice};
pub use voice::voice_composite_node;
pub use signal::{
    register_signal, signal_from_slice, signal_registry, signal_schema, signal_to_vec, signal_type,
    silence, SIGNAL,
};
pub use vca::Vca;
pub use wav::write_wav_i16;
pub use wave::Wave;

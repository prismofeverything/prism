//! Render an organism `.ys` patch to a WAV you can play — the M/R organism, heard.
//!
//! ```sh
//! cargo run -p prism-audio --features ys --example render_organism
//! cargo run -p prism-audio --features ys --example render_organism -- <patch.ys> <seconds> <out.wav>
//! ```
//!
//! Offline (no realtime device): compiles the patch, ticks the engine one audio
//! block at a time collecting the `out` Signal bus, peak-normalizes, and writes a
//! 48 kHz mono WAV. Default patch: `packages/synth/examples/mr-colony.ys`.

use std::collections::BTreeMap;

use prism_audio::prelude::{audio_core, audio_modules};
use prism_audio::{render, write_wav_i16};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const BLOCK: usize = 256;
    const SR: f64 = 48_000.0;

    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages/synth/examples/mr-colony.ys").to_string()
    });
    let seconds: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(20.0);
    let wav = args.next().unwrap_or_else(|| "organism.wav".to_string());

    let src = std::fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))?;
    let prog = chrysalis::parse::parse_program(&src).map_err(|e| format!("parse: {e}"))?;
    let mut engine = chrysalis::runner::build_engine(
        &prog,
        audio_core(BLOCK),
        audio_modules(),
        &BTreeMap::new(),
    )
    .map_err(|e| format!("build engine: {e}"))?;

    let n_blocks = (seconds * SR / BLOCK as f64).ceil() as usize;
    let mut samples = render(&mut engine, "out", n_blocks);

    // Peak-normalize to ~-1 dBFS so it is comfortably audible without clipping.
    let peak = samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    if peak > 1e-6 {
        let g = 0.89 / peak;
        for s in &mut samples {
            *s *= g;
        }
    }

    write_wav_i16(&wav, &samples, SR as u32)?;
    println!(
        "rendered {path}\n  {seconds}s · {} samples · source peak {peak:.3} -> {wav}",
        samples.len()
    );
    Ok(())
}

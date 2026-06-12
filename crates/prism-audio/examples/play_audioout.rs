//! Hear the **`AudioOut` sink** — the device wired into the graph, no `--play`.
//!
//! ```sh
//! cargo run -p prism-audio --features realtime,ys --example play_audioout
//! cargo run -p prism-audio --features realtime,ys --example play_audioout -- <patch.ys> <seconds>
//! ```
//!
//! Builds the engine from a `.ys` patch that contains an `AudioOut` process, then just
//! `engine.run(seconds)` — there is NO external device driver here (unlike `play_ys` /
//! the old `run_realtime`). The `AudioOut` *inside the graph* drives the device, and its
//! back-pressure paces the engine to real-time. This is exactly what `chrysalis run
//! patch.ys` will do once the codegen runner builds prism-audio with `realtime` (lang).
//! Default patch: `packages/synth/examples/play-tone.ys`.

use prism_audio::prelude::{audio_core, audio_modules};

fn main() -> anyhow::Result<()> {
    const BLOCK: usize = 256;
    let default_path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages/synth/examples/play-tone.ys");
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| default_path.to_string());
    let seconds: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4.0);

    let src = std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("read {path}: {e}"))?;
    let prog = chrysalis::parse::parse_program(&src).map_err(|e| anyhow::anyhow!("parse: {e}"))?;
    let mut engine = chrysalis::runner::build_engine(
        &prog,
        audio_core(BLOCK),
        audio_modules(),
        &std::collections::BTreeMap::new(),
    )
    .map_err(|e| anyhow::anyhow!("build engine: {e}"))?;

    println!(
        "playing {path} for {seconds}s — the AudioOut in the graph drives the device and\n\
         paces the engine. No --play flag: the sink is wired in. (no sound? check your output device)"
    );
    engine.run(seconds);
    Ok(())
}

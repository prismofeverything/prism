//! Hear the synth WRITE a synth. Run with the realtime feature + your audio device:
//!
//! ```sh
//! cargo run -p prism-audio --features realtime --example play_writes_synths
//! ```
//!
//! The audible twin of `packages/synth/ys/writes-synths.ys` (and the offline
//! `tests/factory.rs`): a 220 Hz root tone plays; then, ~2.5 s in, the
//! [`stack_factory`] reaction reads a `StackSeed` recipe off the live rack and
//! AUTHORS a brand-new compound module type — a detuned oscillator stack a fifth up
//! (five partials clustered around 330 Hz) — which `discover_processes` brings to
//! life on the shared mix bus. You hear the patch *write itself a new instrument* in
//! real time and the texture bloom from a bare root into a fat stacked fifth. The
//! realtime path is the SAME bigraph as the offline render — only the sink differs
//! (gorgon's lock-free ring instead of a buffer).

use prism_audio::device::{run_realtime, RealtimeOpts};
use prism_audio::{
    patch_brs_node, signal_type, silence, stack_factory, stack_voice_node, StackRecipe,
};
use prism_bigraph::{Schema, Value};

fn main() -> anyhow::Result<()> {
    const RATE: f64 = 48_000.0;
    const BLOCK: usize = 256;

    // The root: a single 220 Hz oscillator (a 1-voice stack), summing into `mix`, so
    // there is sound from the first sample.
    let root = stack_voice_node(
        &StackRecipe::super_saw(220.0, 0.0, 1, 0.3),
        BLOCK,
        RATE,
        "mix",
    );

    // The recipe the synth will author from: a 5-voice super-saw a fifth up (≈330 Hz),
    // detuned ±14 cents — the classic "fat" stack. It sits inert on the rack until the
    // factory reads it. EXACTLY the recipe fields, so each redex site captures its bare
    // value (`stack_factory`'s rest-capture note).
    let seed = Value::tree([
        ("_type", Value::String("StackSeed".into())),
        ("base", Value::float(330.0)),
        ("spread", Value::float(14.0)),
        ("voices", Value::float(5.0)),
    ]);

    // The rack: the mix bus, the root voice, the inert recipe — and a PatchBrs ticking
    // every ~2.5 s, so the factory fires a couple of seconds in (you hear the change
    // arrive, not start there).
    let state = Value::tree([
        (
            "rack",
            Value::tree([("mix", silence(BLOCK)), ("root", root), ("seed", seed)]),
        ),
        // The factory BRS over `rack`, ticking ~every 2.5 s — so the synth authors
        // the stack a couple of seconds in (you hear it arrive, not from sample 0).
        ("brs", patch_brs_node("rack", 2.5)),
    ]);
    let schema = Schema::tree([("rack", Schema::tree([("mix", signal_type())]))]);

    // The factory rule — authors `super_saw(base, spread, voices)` from the seed and
    // wires its summed `out` into `mix`, total amplitude 0.4 (split across the voices,
    // so root 0.3 + stack 0.4 stays clear of clipping). Fire-once (it `_remove`s the
    // seed), so the stack is written exactly once — bounded, no runaway.
    let rules = vec![stack_factory("mix", BLOCK, RATE, 0.4)];
    let engine = prism_audio::audio_engine(state, schema, rules, BLOCK);

    println!(
        "playing ~6 s — a 220 Hz root; ~2.5 s in, the synth AUTHORS a detuned 5-oscillator\n\
         stack a fifth up (the writes-synths factory, live) and it blooms onto the mix bus.\n\
         (no sound? pass a device name and check your default output)"
    );
    run_realtime(
        engine,
        &["rack", "mix"],
        RealtimeOpts {
            device: None,
            seconds: 6.0,
            block: BLOCK,
            sample_rate: RATE,
        },
    )
}

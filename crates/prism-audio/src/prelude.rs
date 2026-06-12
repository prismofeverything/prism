//! The audio domain's runnable assembly — a **chrysalis-free** [`Core`] + a
//! plain-data export manifest — and, behind the `ys` feature, the thin language
//! facet (the `.ys` import surface).
//!
//! The audio DSP / processes / types are independent of the language, so building
//! the audio `Core` needs nothing from chrysalis: [`audio_core`] is assembled from
//! the SUBSTRATE alone (prism-std's std natives + prism-bigraph's `Composite` and
//! `Rest`/`Mesh`/`local` protocols). A domain library never deps the language.
//!
//! Only two things are genuinely language concerns and live behind `ys`: the `.ys`
//! **import surface** ([`audio_modules`], a chrysalis `ModuleRegistry`) and running
//! a `.ys` program (`chrysalis::runner::run`). The clean end-state moves
//! those to a separate runner crate / the #10 codegen; they are gated here for now.
//! `StreamProtocol` (running chrysalis subprograms over a stream) is likewise a
//! language protocol a runner adds when it needs it — not part of the domain Core.

use std::sync::{Arc, OnceLock};

use prism_bigraph::composite::Composite;
use prism_bigraph::protocols::{MeshProtocol, RestProtocol};
use prism_bigraph::{Core, ProcessNode, ProcessRegistry, ProtocolRegistry};
use prism_schema::MethodRegistry;

use crate::patch::register_audio;
use crate::signal::signal_registry;

/// The audio domain's unified [`Core`] — **chrysalis-free**. std natives
/// (`RunProcess`, the generic `Composite`) + the audio kernels (`Oscillator` /
/// `LowPass` / `Vca` / `Envelope`) + the `Signal` type (block-sized) + std
/// value-methods + the substrate protocols (`Rest` / `Mesh`; `local` is the engine
/// default). This is the ONE thing an audio program runs against — a chrysalis
/// runner clones + extends it with a `.ys` program's own defs; a pure-Rust runner
/// uses it directly.
///
/// Built like `chrysalis::prelude::std_core` (the `Composite` factory closes over a
/// `OnceLock` set to THIS core, so a spawned voice inherits the *audio* modules),
/// but it depends only on the substrate — proof that the audio features are
/// independent of the language.
pub fn audio_core(block: usize) -> Core {
    let handle: Arc<OnceLock<Core>> = Arc::new(OnceLock::new());
    let mut registry = ProcessRegistry::new();
    prism_std::register_processes(&mut registry); // RunProcess, Simulate, …
    register_audio(&mut registry); // Oscillator / LowPass / Vca / Envelope
    {
        let handle = Arc::clone(&handle);
        registry.register("Composite", move |config| {
            let core = handle.get().expect("audio core handle initialized");
            ProcessNode::Process(Box::new(
                Composite::from_config(&config, core).expect("Composite::from_config"),
            ))
        });
    }

    let mut methods = MethodRegistry::new();
    prism_std::register_methods(&mut methods);

    // The SUBSTRATE protocol set — no chrysalis `StreamProtocol`. `local` is the
    // engine default; `Rest` + `Mesh` ride for distribution (A8's `net:` extends
    // here). A language runner adds `StreamProtocol` itself if a `.ys` uses `stream:`.
    let mut protocols = ProtocolRegistry::new();
    protocols.register(Arc::new(RestProtocol));
    protocols.register(Arc::new(MeshProtocol));

    let core = Core::new()
        .with_processes(Arc::new(registry))
        .with_methods(Arc::new(methods))
        .with_types(signal_registry(block)) // builtins + the Signal type
        .with_protocols(Arc::new(protocols));
    let _ = handle.set(core.clone());
    core
}

/// The block size [`core`] (the codegen convention) builds the audio `Core` at — the
/// `Signal` length the `Signal` type is sized to. A package `.ys` patch wiring an
/// `Oscillator[…, block: 256]` must agree with the `Signal` type's block, so the
/// convention pins one default; a patch needing another size builds its own
/// [`audio_core`] (the realtime device chooses the device buffer size).
pub const DEFAULT_BLOCK: usize = 256;

// ── Codegen convention (#67 Phase 5) ────────────────────────────────────────
//
// A package whose `project.ys` names this crate as a native dependency
// (`dependencies: { audio: { native: '../crates/prism-audio' } }`) is run by a
// generated codegen runner ([`chrysalis::codegen`]) that links this crate and calls
// `prism_audio::prelude::core()` — the standard zero-arg name a package's run-Core is
// reached by ([`docs/packages-decomposition.md`] §3; the resolver colimits it in via
// `resolve_with_natives`). We expose ONLY `core()` — chrysalis-free — and NOT a
// `modules()` (the own-native convention's import surface): the audio processes reach
// a `.ys` through `resolve_native`, which surfaces them as `from audio import …` from
// the colimited Core, so the import surface needs no language facet here. Keeping
// `core()` chrysalis-free is what lets `synth` be a real package without prism-audio
// depending on the language ([[synthesizer_project]]).

/// Codegen convention alias — the one [`Core`] this package exposes, at the
/// [`DEFAULT_BLOCK`] size. Delegates to [`audio_core`]; **chrysalis-free** (it only
/// touches the substrate), so a native-dependency runner can link this crate without
/// pulling the language. The resolver `own_over`s it off the std floor (leaving exactly
/// the audio processes + the `Signal` type) and colimits it into the program's Core.
pub fn core() -> Core {
    audio_core(DEFAULT_BLOCK)
}

/// The PLAY **sink** convention (`docs/domain-libraries.md` §5) — drive a *built*
/// [`Engine`](prism_bigraph::Engine) to the audio device for `seconds`, reading the
/// conventional `out` Signal bus, one engine tick per block. Behind `realtime` ONLY: it
/// takes a built engine, so the codegen runner — which builds it via
/// `chrysalis::runner::build_engine` — drives PLAY *without* prism-audio depending on the
/// language. Play stays chrysalis-free. `chrysalis run <patch>.ys --play` dispatches here
/// when `project.ys` declares `sink: 'audio'`.
#[cfg(feature = "realtime")]
pub fn run_realtime(engine: prism_bigraph::Engine, seconds: f64) -> anyhow::Result<()> {
    crate::device::run_realtime(
        engine,
        &["out"],
        crate::device::RealtimeOpts {
            device: None,
            seconds,
            block: DEFAULT_BLOCK,
            sample_rate: 48_000.0,
        },
    )
}

/// The audio domain's `.ys` export manifest — plain `(import-group, native-name)`
/// data, **chrysalis-free**. The domain only DECLARES what it exports; a runner
/// turns this into the language's import surface ([`audio_modules`]).
pub fn audio_exports() -> &'static [(&'static str, &'static str)] {
    &[
        ("audio", "Oscillator"),
        ("audio", "LowPass"),
        ("audio", "Svf"),
        ("audio", "Vca"),
        ("audio", "Envelope"),
        ("audio", "Slope"),
        ("audio", "Compare"),
        ("audio", "SampleHold"),
        ("audio", "Noise"),
        ("audio", "Fold"),
        ("audio", "RingMod"),
        ("audio", "Counter"),
        ("audio", "Sequencer"),
        ("audio", "AudioOut"),
    ]
}

/// The `.ys` import surface, built from [`audio_exports`] — the LANGUAGE facet (a
/// chrysalis `ModuleRegistry`), behind the `ys` feature. The `Signal` type rides
/// [`audio_core`]'s type registry (seeded by `compile_with_core`), so it resolves
/// as a type name without a separate import.
#[cfg(feature = "ys")]
pub fn audio_modules() -> chrysalis::compile::ModuleRegistry {
    let mut modules = chrysalis::prelude::std_modules();
    for (group, name) in audio_exports() {
        modules = modules.process(group, name);
    }
    modules
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::SIGNAL;

    /// **chrysalis-free proof** — `audio_core` assembles the full audio runtime
    /// from the substrate alone, so this builds + passes in the DEFAULT build (no
    /// `ys` feature, no chrysalis in the dependency tree).
    #[test]
    fn audio_core_serves_std_and_audio_capabilities() {
        let core = audio_core(256);
        for p in [
            "RunProcess",
            "Composite",
            "Oscillator",
            "LowPass",
            "Vca",
            "Envelope",
        ] {
            assert!(core.processes.contains(p), "audio_core should serve `{p}`");
        }
        assert!(
            core.types.type_names().iter().any(|n| *n == SIGNAL),
            "audio_core's type registry carries the Signal type"
        );
    }

    /// The **codegen convention** the package runner calls — `prelude::core()`
    /// (zero-arg) — serves the same audio capabilities + the `Signal` type, in the
    /// DEFAULT build (no `ys` feature, no chrysalis). This is the entry a
    /// `packages/synth` native-dependency runner links: its `own_over(std)` is exactly
    /// the audio processes + `Signal`, which the resolver colimits in conflict-free.
    #[test]
    fn core_convention_serves_audio_chrysalis_free() {
        let core = core();
        for p in ["Oscillator", "LowPass", "Vca", "Envelope"] {
            assert!(core.processes.contains(p), "core() should serve `{p}`");
        }
        assert!(
            core.types.type_names().iter().any(|n| *n == SIGNAL),
            "core() carries the Signal type (rides the resolver colimit)"
        );
    }
}

/// The `.ys` SURFACE tests — the language facet, behind `ys`. They exercise the
/// chrysalis-free [`audio_core`] *through* the chrysalis runner.
#[cfg(all(test, feature = "ys"))]
mod ys_surface {
    use super::*;
    use crate::signal::signal_to_vec;

    fn goertzel(samples: &[f32], freq: f64, rate: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq / rate;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
        for &x in samples {
            let s0 = x as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
        power.max(0.0).sqrt() / (samples.len().max(1) as f64)
    }

    // A one-oscillator audio patch authored on the `.ys` surface — proof the audio
    // domain is reachable from `.ys` through the canonical run-Core door.
    const PATCH_YS: &str = "\
from audio import Oscillator
composite Patch ~{} ->{ out :: Signal } (
  out: instantiate({
    mix: [0.0], phase: 0.0,
    osc: Oscillator[wave: 'Sine', freq: 220.0, amplitude: 0.5, sample_rate: 48000.0, block: 256]
      ~{phase: phase} ->{phase: phase, out: mix}
  }, 0.05).mix
)
";

    #[test]
    fn a_ys_audio_patch_renders_through_run() {
        let prog = chrysalis::parse::parse_program(PATCH_YS).expect("parse");
        let state = chrysalis::runner::run(&prog, audio_core(256), audio_modules(), 0.0)
            .expect("run");
        let sig = state.get_field("out").expect("Patch.out Signal in final state");
        let samples = signal_to_vec(sig);
        assert!(samples.len() >= 256, "a full Signal block rendered (got {})", samples.len());
        let e220 = goertzel(&samples, 220.0, 48_000.0);
        assert!(
            e220 > 0.05,
            "the .ys-authored Oscillator ran through audio_core: energy at 220 Hz ({e220})"
        );
    }

    // THE HEADLINE — a `Composite[…]` expression authors a NEW 2-oscillator module
    // TYPE inline (the synth writes synths, on the surface), instantiate runs it, its
    // summed Signal bridges out. The authored composite carries `interval = block/rate`
    // (256/48000) so its sub-engine ticks at BLOCK-RATE — without it the inner engine
    // ticks zero times inside `instantiate(…, 0.05)` and `bus` stays the initial `[0.0]`
    // (the synth block-rate gotcha; the Rust `stack_voice_node` sets the same interval).
    // Shipped as the package demo `packages/synth/ys/writes-synths.ys`.
    const WRITES_SYNTHS_YS: &str = "\
from audio import Oscillator
composite WritesSynths ~{} ->{ out :: Signal } (
  out: instantiate({
    stack: Composite[
      state: {
        mix: [0.0], pa: 0.0, pb: 0.0,
        oa: Oscillator[wave: 'Sine', freq: 220.0, amplitude: 0.4, sample_rate: 48000.0, block: 256]
          ~{phase: pa} ->{phase: pa, out: mix},
        ob: Oscillator[wave: 'Sine', freq: 330.0, amplitude: 0.4, sample_rate: 48000.0, block: 256]
          ~{phase: pb} ->{phase: pb, out: mix}
      },
      bridge: { inputs: {}, outputs: { out: ['mix'] } },
      interval: 0.00533333
    ] ~{} ->{ out: bus },
    bus: [0.0]
  }, 0.05).bus
)
";

    #[test]
    fn a_ys_expression_authors_a_two_oscillator_module_type() {
        let prog = chrysalis::parse::parse_program(WRITES_SYNTHS_YS).expect("parse");
        let state = chrysalis::runner::run(&prog, audio_core(256), audio_modules(), 0.0)
            .expect("run");
        let sig = state.get_field("out").expect("WritesSynths.out Signal");
        let samples = signal_to_vec(sig);
        let e220 = goertzel(&samples, 220.0, 48_000.0);
        let e330 = goertzel(&samples, 330.0, 48_000.0);
        assert!(
            e220 > 0.02 && e330 > 0.02,
            "the .ys-authored 2-oscillator module ran on the surface: 220 ({e220}) + 330 ({e330})"
        );
    }

    // A CONTINUOUS `.ys` engine — one tick = one audio block — built via the same
    // `build_engine` seam `run_ys_realtime` (the device sink) uses. The oscillator is
    // a live composite child writing the `out` Signal each tick (vs PATCH_YS's one-shot
    // `instantiate`). Device-free proof of the `.ys` → audible path: tick it a few
    // blocks, each a fresh 220 Hz block with the phase carried across ticks.
    const LIVE_YS: &str = "\
from audio import Oscillator
composite Live ~{} ->{ out :: Signal @ out } (
  out: [0.0] |
  phase: 0.0 |
  osc: Oscillator[wave: 'Sine', freq: 220.0, amplitude: 0.4, sample_rate: 48000.0, block: 256]
    ~{phase: phase} ->{phase: phase, out: out}
)
";

    #[test]
    fn a_continuous_ys_patch_ticks_a_block_per_step() {
        let prog = chrysalis::parse::parse_program(LIVE_YS).expect("parse");
        let mut engine = chrysalis::runner::build_engine(
            &prog,
            audio_core(256),
            audio_modules(),
            &std::collections::BTreeMap::new(),
        )
        .expect("build engine");

        // Drive the engine the way the device sink does: one `tick_block` per audio
        // block, reading the `out` bus.
        let blocks: Vec<Vec<f32>> = (0..4)
            .map(|_| crate::render::tick_block(&mut engine, &["out"]))
            .collect();

        for (i, b) in blocks.iter().enumerate() {
            assert!(b.len() >= 256, "block {i} is a full Signal ({} samples)", b.len());
            assert!(
                goertzel(b, 220.0, 48_000.0) > 0.05,
                "block {i} sounds at 220 Hz ({})",
                goertzel(b, 220.0, 48_000.0)
            );
        }
        // The phase carried across ticks — block 1 continues block 0's wave, it is not
        // the same block replayed. This is what makes it CONTINUOUS audio for the device.
        assert!(
            blocks[0] != blocks[1],
            "the wave advances across ticks (continuous, not a repeated block)"
        );
    }
}

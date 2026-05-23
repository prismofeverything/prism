//! Runnable process-contract demo (`docs/process-contracts.md`), end to end from
//! the `.ys` source: parse → enforce contracts → compile against the native
//! integrator library → run the one-shot Step DAG.
//!
//! Run from the repo root:
//!
//! ```text
//! cargo run -p spatio-flux --example integrator_comparison
//! ```
//!
//! The **workflow writes its own artifacts** — the `.ys` `Output` step calls the
//! native `.csv`/`.svg` writer methods, so there is no Rust harness doing IO.
//! It produces, under `outputs/integrator-comparison/`:
//!   - `Rk4.csv`, `ForwardEuler.csv` — the two trajectories
//!   - `mse.csv`                     — per-species MSE between them
//!   - `overlay.svg`                 — the comparison plot (the workflow's `Figure`)
//!
//! The MSE is a *warranted* comparison: `Rk4` and `ForwardEuler` both `fulfill`
//! the `DeterministicMassAction` contract, so wiring them into one `Compare`
//! compiles; an FBA/Gillespie process targeting a different math object would
//! not. The two integrators realize the *same* deterministic mass-action target
//! by *different* methods, and the MSE measures that method-space divergence.
//!
//! Lives in spatio-flux (not chrysalis) for the same reason the test does: it
//! wires the ys *language* to spatio-flux's *native* processes, and chrysalis must
//! not depend on spatio-flux (see memory `feedback_ys_layering`).

use std::sync::Arc;

use prism_bigraph::Engine;
use prism_schema::MethodRegistry;

use chrysalis::compile::{compile_with_modules, ModuleRegistry};
use chrysalis::parse::parse_program;
use spatio_flux::from_config::build_registry;
use spatio_flux::processes::mass_action;

const SRC: &str = include_str!("../../chrysalis/ys/integrator-comparison.ys");

/// The native value-methods the ys-native bodies dispatch to: `TimeSeries`
/// (`species_mse`, `overlay`, `csv`), `Figure` (`svg`), `Map` (`csv`), and the
/// `Integrator` `integrate` the function-bodied processes call.
fn ts_methods() -> MethodRegistry {
    let mut m = MethodRegistry::new();
    mass_action::register_methods(&mut m);
    m
}

/// The native modules the `.ys` imports: `RunProcess` wholesale from `core`, the
/// `rk4`/`euler` integrator objects from `integrators`, the `CRN` model type from
/// `chem`, and the `Path` type from `io`.
fn modules() -> ModuleRegistry {
    ModuleRegistry::new()
        .process("core", "RunProcess")
        .object("integrators", "rk4", mass_action::integrator("rk4"))
        .object("integrators", "euler", mass_action::integrator("euler"))
        .type_(
            "chem",
            "CRN",
            "{species: list[string], reactions: list[{reactants: map[float], products: map[float], k: float}]}",
        )
        .type_("io", "Path", "string")
}

fn main() {
    // 1. Parse → enforce contracts → compile against the native registry.
    let prog = parse_program(SRC).expect("integrator-comparison.ys should parse");
    let result = compile_with_modules(&prog, build_registry(), ts_methods(), modules())
        .expect("compile: contracts enforced, imported rk4/euler/RunProcess/CRN resolve to native");
    // Reaching here means `check::check_contract` passed: both producers wired
    // into `Compare` refine `DeterministicMassAction`. An FBA/Gillespie process
    // (a different target) would have made this a compile error.
    println!(
        "\u{2713} contract enforced: Rk4 and ForwardEuler both fulfill \
         DeterministicMassAction \u{2014} the comparison is warranted.\n"
    );

    // 2. Run the one-shot Step DAG. The `Output` step writes the artifacts.
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(2.0);
    let state = engine.state();

    // 3. Echo the MSE the workflow computed (also written to mse.csv).
    if let Some(mse) = state.get_path(&["mse".into()]).and_then(|v| v.as_map()) {
        println!("MSE \u{2014} Rk4 vs ForwardEuler (same target, different method):");
        for (sp, v) in mse {
            println!("  {sp}: {:.3e}", v.as_f64().unwrap_or(f64::NAN));
        }
    }
    println!(
        "\nThe workflow wrote its artifacts (trajectory CSVs, mse.csv, overlay.svg) \
         to outputs/integrator-comparison/"
    );
}

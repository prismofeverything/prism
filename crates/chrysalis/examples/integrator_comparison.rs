//! Runnable process-contract demo over chrysalis's std prelude (prism-std
//! natives): parse → compile → run; the workflow's `Output` step writes the
//! artifacts itself. Run from the repo root:
//!
//! ```text
//! cargo run -p chrysalis --example integrator_comparison
//! ```

use chrysalis::parse::parse_program;
use chrysalis::prelude::{std_methods, std_modules, std_registry};

const SRC: &str = include_str!("../ys/integrator-comparison.ys");

fn main() {
    let prog = parse_program(SRC).expect("integrator-comparison.ys should parse");
    // Reaching past run() means compile (incl. contract checking) + run worked.
    let state = chrysalis::runner::run(&prog, std_registry(), std_methods(), std_modules(), 2.0)
        .expect("run: contracts enforced, std natives resolve");
    println!(
        "\u{2713} contract enforced: Rk4 and ForwardEuler both fulfill \
         DeterministicMassAction \u{2014} the comparison is warranted.\n"
    );

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

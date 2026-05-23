//! prism-std — prism's native standard library: reusable processes, types, and
//! value-methods that chrysalis exposes to `.ys` as importable modules
//! (`core`/`integrators`/`chem`/`io`). Downstream packages (e.g. spatio-flux)
//! add their own natives on top; this is the part that is *prism* work, not a
//! particular demo.

pub mod mass_action;
pub mod process_runner;

pub use mass_action::{integrator, register_methods};
pub use process_runner::RunProcess;

use prism_bigraph::{ProcessNode, ProcessRegistry};

/// Register prism-std's native process factories into a [`ProcessRegistry`]
/// (currently `RunProcess` — the generic "run a process over time" Step).
pub fn register_processes(reg: &mut ProcessRegistry) {
    reg.register("RunProcess", |config| {
        ProcessNode::Step(Box::new(process_runner::RunProcess::from_config(&config)))
    });
}

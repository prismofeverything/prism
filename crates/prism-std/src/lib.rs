//! prism-std — prism's native standard library: reusable processes, types, and
//! value-methods that chrysalis exposes to `.ys` as importable modules
//! (`core`/`integrators`/`chem`/`io`). Downstream packages (e.g. spatio-flux)
//! add their own natives on top; this is the part that is *prism* work, not a
//! particular demo.

pub mod mass_action;
pub mod math;
pub mod process_runner;
pub mod simulate;

pub use mass_action::{integrator, register_methods, stochastic};
pub use process_runner::RunProcess;
pub use simulate::Simulate;

use prism_bigraph::{ProcessNode, ProcessRegistry};

/// Register prism-std's native process factories into a [`ProcessRegistry`]:
/// `RunProcess` (run a process over time, capturing full frames) and `Simulate`
/// (the engine-faithful event-source runner — folds each update via the schema
/// and captures a delta-log `Trace`; see [`prism_trace`]).
pub fn register_processes(reg: &mut ProcessRegistry) {
    reg.register("RunProcess", |config| {
        ProcessNode::Step(Box::new(process_runner::RunProcess::from_config(&config)))
    });
    reg.register("Simulate", |config| {
        ProcessNode::Step(Box::new(simulate::Simulate::from_config(&config)))
    });
}

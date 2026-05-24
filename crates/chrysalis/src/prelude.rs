//! The std prelude — prism-std's native capabilities assembled into the
//! registries the runner/CLI need. chrysalis bundles prism-std as its standard
//! library (prism-std → chrysalis), so a `.ys` using std imports
//! (`core`/`integrators`/`chem`/`io`) runs with no extra packages. Downstream
//! packages (e.g. spatio-flux) extend these with their own.

use std::sync::{Arc, OnceLock};

use prism_bigraph::composite::Composite;
use prism_bigraph::{Core, ProcessNode, ProcessRegistry};
use prism_schema::MethodRegistry;

use crate::compile::ModuleRegistry;

/// Process factories from prism-std (currently `RunProcess`).
pub fn std_registry() -> ProcessRegistry {
    let mut r = ProcessRegistry::new();
    prism_std::register_processes(&mut r);
    r
}

/// Value-methods from prism-std (`TimeSeries`/`Integrator`/`Figure`/`Map`).
pub fn std_methods() -> MethodRegistry {
    let mut m = MethodRegistry::new();
    prism_std::register_methods(&mut m);
    m
}

/// The std library assembled as a runnable [`Core`]: the std process factories +
/// the generic `Composite` factory (so a server can build composites from a doc)
/// + std value-methods. One object carrying the full std capability set — used by
/// `chrysalis server` and any in-process host that wants it whole.
pub fn std_core() -> Core {
    // The Composite factory needs the whole Core (to build subengines); the Core
    // contains the registry that contains this factory — a cycle resolved by a
    // OnceLock set once the Core is built.
    let handle: Arc<OnceLock<Core>> = Arc::new(OnceLock::new());
    let mut registry = ProcessRegistry::new();
    prism_std::register_processes(&mut registry);
    {
        let handle = Arc::clone(&handle);
        registry.register("Composite", move |config| {
            let core = handle.get().expect("core handle not initialized");
            ProcessNode::Process(Box::new(
                Composite::from_config(&config, core).expect("Composite::from_config"),
            ))
        });
    }
    let mut methods = MethodRegistry::new();
    prism_std::register_methods(&mut methods);
    let core = Core::new()
        .with_processes(Arc::new(registry))
        .with_methods(Arc::new(methods))
        .with_protocols(Arc::new(crate::stream::stream_protocols()));
    let _ = handle.set(core.clone());
    core
}

/// The std importable modules: `core` (RunProcess), `integrators` (rk4/euler),
/// `chem` (CRN), `io` (Path).
pub fn std_modules() -> ModuleRegistry {
    ModuleRegistry::new()
        .process("core", "RunProcess")
        .object("integrators", "rk4", prism_std::integrator("rk4"))
        .object("integrators", "euler", prism_std::integrator("euler"))
        .type_(
            "chem",
            "CRN",
            "{species: list[string], reactions: list[{reactants: map[float], products: map[float], k: float}]}",
        )
        .type_("io", "Path", "string")
}

//! spatio-flux's runtime prelude — its native processes assembled into a runnable
//! [`Core`], layered on chrysalis's std core. This is the package half of
//! prism-std → chrysalis → spatio-flux: chrysalis's tool/runner consumes a `Core`,
//! and [`sf_core`] is the `Core` a spatio-flux `.ys` runs against (std
//! capabilities + spatio-flux's FBA / diffusion / particle processes). It is what
//! a spatio-flux runner bin passes to `chrysalis::runner::run`, and the model for
//! what the `chrysalis compile` codegen will assemble for a non-std package.

use std::sync::{Arc, OnceLock};

use prism_bigraph::composite::Composite;
use prism_bigraph::{Core, ProcessNode, ProcessRegistry};
use prism_schema::MethodRegistry;

/// The full spatio-flux [`Core`]: std natives (`RunProcess`, the generic
/// `Composite`) + std value-methods + spatio-flux's native processes
/// (`MonodKinetics`, `DiffusionAdvection`, `DynamicFBA`, `SpatialDFBA`, the
/// particle processes, …).
pub fn sf_core() -> Core {
    // The Composite factory needs the whole Core to build subengines; the Core
    // contains the registry that contains this factory — a cycle resolved by a
    // OnceLock set once everything is built (same pattern as chrysalis std_core).
    let handle: Arc<OnceLock<Core>> = Arc::new(OnceLock::new());

    // spatio-flux's own native processes, plus the std `RunProcess`.
    let mut registry = crate::from_config::build_registry();
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
        .with_methods(Arc::new(methods));
    let _ = handle.set(core.clone());
    core
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sf_core_serves_std_and_spatioflux_processes() {
        let core = sf_core();
        for p in [
            "RunProcess",
            "Composite",
            "MonodKinetics",
            "DiffusionAdvection",
            "DynamicFBA",
            "SpatialDFBA",
        ] {
            assert!(core.processes.contains(p), "sf_core should serve `{p}`");
        }
    }
}

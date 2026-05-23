//! The std prelude — prism-std's native capabilities assembled into the
//! registries the runner/CLI need. chrysalis bundles prism-std as its standard
//! library (prism-std → chrysalis), so a `.ys` using std imports
//! (`core`/`integrators`/`chem`/`io`) runs with no extra packages. Downstream
//! packages (e.g. spatio-flux) extend these with their own.

use prism_bigraph::ProcessRegistry;
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

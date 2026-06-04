//! coda's runtime prelude — the codegen handshake.
//!
//! A `.ys` project with `package coda` in its `project.ys` triggers chrysalis's
//! codegen path, which generates + builds + caches a runner crate that links
//! `coda::prelude::{registry, methods, modules}`. Modeled on
//! `spatio_flux::prelude` (the worked example in
//! `../docs/chrysalis-design.md`).

use chrysalis::compile::ModuleRegistry;
use prism_bigraph::ProcessRegistry;
use prism_schema::MethodRegistry;

/// Codegen convention: the [`ProcessRegistry`] a coda `.ys` runs against.
/// Inherits prism-std's natives + coda's own processes (CSV loaders + stats).
pub fn registry() -> ProcessRegistry {
    let mut registry = ProcessRegistry::new();
    prism_std::register_processes(&mut registry);
    crate::io::register_processes(&mut registry);
    crate::stats::register_processes(&mut registry);
    registry
}

/// Codegen convention: value-methods. Inherits prism-std; coda's value-methods
/// will land here as needed.
pub fn methods() -> MethodRegistry {
    let mut methods = MethodRegistry::new();
    prism_std::register_methods(&mut methods);
    methods
}

/// Codegen convention: importable modules (`from … import …`). Inherits the
/// std modules (`core` / `integrators` / `chem` / `io` / `meta`) and adds
/// coda's own:
///   - `loaders` → `LoadDominicaCSV`
///   - `stats`   → `LagAutocorrelation`
pub fn modules() -> ModuleRegistry {
    chrysalis::prelude::std_modules()
        .process("loaders", "LoadDominicaCSV")
        .process("stats", "LagAutocorrelation")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prelude_handshake_builds() {
        let _ = registry();
        let _ = methods();
        let _ = modules();
    }
}

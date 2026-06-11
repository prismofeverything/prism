//! `resolver.rs` — resolve a package's dependencies into a linked Core + an import
//! surface (#67, Phase 1).
//!
//! "Depend on package P" = "run against P's Core" (`docs/canonical-run-core.md`):
//! a package is a `Core`, so resolution builds the **colimit** over the dependency
//! edges — the shared std base merged with each dependency's OWN theory, through
//! [`prism_bigraph::Core::merge`] (the idempotent join-semilattice). Two halves a
//! `.ys` program needs to actually USE a dependency:
//!
//! 1. **the linked Core** — every dependency's runtime factories, merged. A
//!    dependency's compiled Core bundles the std floor + per-compile infrastructure,
//!    so we link its [`own_over`](prism_bigraph::Core::own_over) the base — its
//!    declared theory — and `Core::merge` joins theories without that infrastructure
//!    false-conflicting.
//! 2. **the import surface** — a [`ModuleRegistry`] declaring each dependency's
//!    exported *processes*, so `from <dep> import <Name>` makes the compiler treat
//!    `<Name>` as a process to instantiate (not a bare typed value). The factory
//!    rides the Core; the module surface is the compile-time NAME. (Type exports
//!    resolve ambiently through the merged `TypeRegistry`, so they need no module
//!    declaration.)
//!
//! Phase 1 resolves **direct** path dependencies; transitive resolution + version
//! solving + a lockfile are Phase 2 (they hook in at [`compile_lib`], where a
//! dependency's OWN manifest is already loaded). THIN: the linker is `Core::merge`
//! (prism), the compiler is `compile_with_core` (lang) — resolver only WIRES them,
//! cloning no runtime and no linker ([[feedback_chrysalis_thin_layer]]).

use std::path::Path;

use prism_bigraph::{Core, CoreMergeConflict};

use crate::compile::{compile_with_core, ModuleRegistry};
use crate::manifest::Manifest;
use crate::parse::parse_program_in;
use crate::prelude::{std_core, std_modules};

/// The conventional library entry of a `.ys` package (Cargo's `src/lib.rs`).
pub const LIB_ENTRY: &str = "lib.ys";

/// Resolve `manifest`'s DIRECT dependencies into the linked Core + the import-surface
/// ModuleRegistry — the two halves a program compiles + runs against. The linked Core
/// is built on `std_core()`; the module surface EXTENDS `base_modules` (so the caller
/// controls `load()` path resolution — `std_modules()` for CWD-relative, or
/// `std_modules_at(dir)` for program-dir-relative, matching the std run path) with
/// each dependency's exported processes.
pub fn resolve(
    manifest: &Manifest,
    base_modules: ModuleRegistry,
) -> Result<(Core, ModuleRegistry), String> {
    // ONE shared std base — so every dependency's std entries are the SAME `Arc`s
    // (compile clones the registries Arc-value-preserving) and `own_over` strips them
    // by name; the floor is established once.
    let base = std_core();
    let mut linked = base.clone();
    let mut modules = base_modules;

    for dep in &manifest.dependencies {
        let dep_dir = dep.resolved_dir(&manifest.dir);
        let context = || format!("dependency `{}` ({})", dep.name, dep_dir.display());

        let dep_manifest =
            Manifest::load(&dep_dir).map_err(|e| format!("{}: {e}", context()))?;
        let dep_core = compile_lib(&dep_dir, &base).map_err(|e| format!("{}: {e}", context()))?;

        // Link the dependency's OWN theory — `own_over(base)` strips the shared std
        // floor AND the per-compile generic `Composite`/`Brs` factories every compile
        // re-creates, so `merge` joins package theories without that infrastructure
        // false-conflicting. A genuine clash (two packages exporting the same name
        // differently) still surfaces.
        let own = dep_core.own_over(&base);
        modules = declare_process_exports(modules, &dep.name, &dep_manifest.exports, &own);
        linked = linked.merge(&own).map_err(|conflicts| {
            format!(
                "linking dependency `{}` conflicts with what is already linked: {}",
                dep.name,
                render_conflicts(&conflicts)
            )
        })?;
    }
    Ok((linked, modules))
}

/// Convenience: resolve only the linked Core (callers that build their own import
/// surface, or whose program imports nothing from its dependencies).
pub fn resolve_core(manifest: &Manifest) -> Result<Core, String> {
    resolve(manifest, std_modules()).map(|(core, _modules)| core)
}

/// Compile the `.ys` package's library entry (`<dir>/lib.ys`) into its Core: the std
/// `base` extended with the package's own defs.
fn compile_lib(dir: &Path, base: &Core) -> Result<Core, String> {
    let entry = dir.join(LIB_ENTRY);
    let src = std::fs::read_to_string(&entry)
        .map_err(|e| format!("read library entry {}: {e}", entry.display()))?;
    let program =
        parse_program_in(&src, dir).map_err(|e| format!("parse {}: {e}", entry.display()))?;
    let result = compile_with_core(&program, base.clone(), std_modules())
        .map_err(|e| format!("compile {}: {e}", entry.display()))?;
    Ok(result.core)
}

/// Declare a dependency's exported PROCESSES as importable from a module named after
/// the dependency, so `from <dep> import <Name>` resolves `<Name>` as a process at
/// compile time (the factory rides the linked Core). An export that is not a process
/// in the dependency's own theory (a type — resolved ambiently via the merged
/// `TypeRegistry` — or a function) is skipped here.
fn declare_process_exports(
    mut modules: ModuleRegistry,
    dep_name: &str,
    exports: &[String],
    own: &Core,
) -> ModuleRegistry {
    for name in exports {
        if own.processes.contains(name) {
            modules = modules.process(dep_name, name);
        }
    }
    modules
}

fn render_conflicts(conflicts: &[CoreMergeConflict]) -> String {
    conflicts
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

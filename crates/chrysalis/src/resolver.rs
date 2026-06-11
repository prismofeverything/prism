//! `resolver.rs` — resolve a package's dependency DAG into a linked Core + import
//! surface + the resolved graph (#67, Phase 2: transitive + semver).
//!
//! "Depend on package P" = "run against P's Core" (`docs/canonical-run-core.md`).
//! Resolution is the **colimit over the dependency DAG** — *iterated
//! `own_over`-then-`merge` to a shared apex* (the framing `unify` settled): each
//! package is compiled ONCE against `base ⊔ its-resolved-deps`, its OWN theory taken
//! ([`Core::own_over`]), and the theories linked via [`Core::colimit`] — prism's
//! n-ary join (core's primitive; confluent over any order, accumulates every
//! conflict). Because every package's own theory is **memoized by name**, a
//! **diamond** dependency (`prog→A,B`, `A→D`, `B→D`) compiles + merges `D` exactly
//! ONCE at the shared apex — *not* a naive double-union that false-conflicts `D` with
//! itself. **Dependency resolution is confluence.**
//!
//! **Semver.** Each edge carries a [`VersionReq`] (the compatibility functor); the
//! resolver checks the resolved package's [`Version`] satisfies it, and a same-name
//! package reached at two incompatible versions is a conflict (the resolver picks one
//! version per name so the apex is well-defined — the registry as a poset over
//! `name × version`). Cycles are detected.
//!
//! THIN: the linker is `Core::merge` (prism), the compiler is `compile_with_core`
//! (lang) — this module only WIRES them into the DAG walk ([[feedback_chrysalis_thin_layer]]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use prism_bigraph::{Core, CoreMergeConflict};

use crate::compile::{compile_with_core, ModuleRegistry};
use crate::manifest::Manifest;
use crate::parse::parse_program_in;
use crate::prelude::{std_core, std_modules};
use crate::version::Version;

/// The conventional library entry of a `.ys` package (Cargo's `src/lib.rs`).
pub const LIB_ENTRY: &str = "lib.ys";

/// The full result of resolving a package's dependencies: what a program compiles +
/// runs against, plus the resolved graph the lockfile records.
pub struct Resolution {
    /// The linked Core — the shared std floor ⊔ every resolved package's own theory.
    pub core: Core,
    /// The import surface — the root's direct dependencies' exported processes
    /// (`from <dep> import <Name>`). A transitive dependency's exports are surfaced
    /// only into the package that depends on it directly, at that package's compile.
    pub modules: ModuleRegistry,
    /// Every resolved package (sorted by name — deterministic): its identity + the
    /// direct-dependency edges. The lockfile's pinned graph (Phase 2 P2d).
    pub graph: Vec<ResolvedPackage>,
}

/// One node of the resolved dependency graph (the lockfile section).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: Version,
    /// The package's directory (where its `project.ys` + `lib.ys` live).
    pub source: PathBuf,
    /// The names of this package's DIRECT dependencies (the graph edges).
    pub dependencies: Vec<String>,
}

/// Resolve `manifest`'s dependency DAG. `base_modules` (`std_modules()` or
/// `std_modules_at(dir)`) seeds the import surface so the caller controls `load()`
/// path resolution.
pub fn resolve(manifest: &Manifest, base_modules: ModuleRegistry) -> Result<Resolution, String> {
    let mut resolver = Resolver {
        base: std_core(),
        base_modules,
        memo: BTreeMap::new(),
        in_progress: BTreeSet::new(),
    };

    // Resolve each DIRECT dependency (recursively), then surface its exports to the
    // root program. `clone()` because `resolve_dep` borrows the resolver mutably.
    let mut root_modules = resolver.base_modules.clone();
    for dep in &manifest.dependencies {
        let name = resolver.resolve_dep(&dep.resolved_dir(&manifest.dir), &dep.req)?;
        let exports = resolver.memo[&name].export_processes.clone();
        root_modules = declare_process_exports(root_modules, &dep.name, &exports);
    }

    // The colimit: the shared floor ⊔ every resolved package's own theory, via the
    // n-ary join `Core::colimit` (core's prism primitive — confluent over any order,
    // accumulates ALL conflicts). Each name appears ONCE in the memo, so the diamond's
    // apex is a single part — merged once. (Thin-layer: the linker is prism's, never
    // cloned here.)
    let parts: Vec<Core> = resolver.memo.values().map(|p| p.own_core.clone()).collect();
    let core = resolver.base.colimit(&parts).map_err(|conflicts| {
        format!("linking the dependency graph: {}", render_conflicts(&conflicts))
    })?;

    let graph = resolver
        .memo
        .values()
        .map(|p| ResolvedPackage {
            name: p.name.clone(),
            version: p.version,
            source: p.source.clone(),
            dependencies: p.direct_deps.clone(),
        })
        .collect();

    Ok(Resolution {
        core,
        modules: root_modules,
        graph,
    })
}

/// Convenience: resolve only the linked Core (callers with no dependency imports, or
/// that build their own import surface).
pub fn resolve_core(manifest: &Manifest) -> Result<Core, String> {
    resolve(manifest, std_modules()).map(|r| r.core)
}

/// The recursive resolution state: the shared std floor, the import-surface seed, the
/// memo (one resolved package per NAME — the colimit dedup), and a cycle guard.
struct Resolver {
    base: Core,
    base_modules: ModuleRegistry,
    memo: BTreeMap<String, ResolvedPkg>,
    in_progress: BTreeSet<String>,
}

/// A fully resolved package: its identity, its OWN theory (compiled once, the part
/// strictly above `base ⊔ its-deps`), its exported processes, and its direct deps.
struct ResolvedPkg {
    name: String,
    version: Version,
    source: PathBuf,
    own_core: Core,
    export_processes: Vec<String>,
    direct_deps: Vec<String>,
}

impl Resolver {
    /// Resolve the package at `dir`, requiring its version satisfy `req`. Returns the
    /// package's NAME (its key in the memo). Idempotent per name: a second edge to an
    /// already-resolved package returns immediately (the diamond), after checking the
    /// versions agree.
    fn resolve_dep(&mut self, dir: &Path, req: &crate::version::VersionReq) -> Result<String, String> {
        let manifest = Manifest::load(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = manifest.name.clone();
        let version = manifest.version.unwrap_or_else(|| Version::new(0, 0, 0));

        if !req.matches(&version) {
            return Err(format!(
                "`{name}` at {} is {version}, which does not satisfy `{req}`",
                dir.display()
            ));
        }

        // Already resolved (a shared / diamond dependency)? It must be the SAME
        // version — the resolver picks ONE version per name so the apex is well-defined.
        if let Some(existing) = self.memo.get(&name) {
            if existing.version != version {
                return Err(format!(
                    "`{name}` is required at two incompatible versions: {} (already resolved) \
                     and {version} (at {})",
                    existing.version,
                    dir.display()
                ));
            }
            return Ok(name);
        }

        // Cycle guard — an edge that reaches a package currently being resolved.
        if !self.in_progress.insert(name.clone()) {
            return Err(format!("dependency cycle through `{name}`"));
        }

        // Resolve this package's OWN dependencies first (depth-first), collecting
        // their theories + the import surface its `lib.ys` sees + the graph edges.
        let mut dep_cores = Vec::new();
        let mut dep_modules = self.base_modules.clone();
        let mut direct_deps = Vec::new();
        for dep in &manifest.dependencies {
            let dep_name = self.resolve_dep(&dep.resolved_dir(&manifest.dir), &dep.req)?;
            let resolved = &self.memo[&dep_name];
            dep_cores.push(resolved.own_core.clone());
            dep_modules =
                declare_process_exports(dep_modules, &dep.name, &resolved.export_processes);
            direct_deps.push(dep_name);
        }
        // The core this package compiles against = the floor ⊔ its resolved deps'
        // theories (the colimit). A shared dep is the SAME memoized Arc → merged once.
        let against = self.base.colimit(&dep_cores).map_err(|conflicts| {
            format!("linking `{name}`'s dependencies: {}", render_conflicts(&conflicts))
        })?;

        // Compile THIS package against (base ⊔ its resolved deps) with that surface,
        // then take its OWN theory (the part strictly above what it compiled against).
        let compiled = compile_lib(dir, &against, dep_modules)
            .map_err(|e| format!("compiling `{name}` ({}): {e}", dir.display()))?;
        let own_core = compiled.own_over(&against);
        let export_processes = manifest
            .exports
            .iter()
            .filter(|name| own_core.processes.contains(name.as_str()))
            .cloned()
            .collect();

        self.in_progress.remove(&name);
        self.memo.insert(
            name.clone(),
            ResolvedPkg {
                name: name.clone(),
                version,
                source: dir.to_path_buf(),
                own_core,
                export_processes,
                direct_deps,
            },
        );
        Ok(name)
    }
}

/// Compile a `.ys` package's library entry (`<dir>/lib.ys`) against `against` (the
/// base ⊔ its resolved deps), with `modules` declaring its deps' exports.
fn compile_lib(dir: &Path, against: &Core, modules: ModuleRegistry) -> Result<Core, String> {
    let entry = dir.join(LIB_ENTRY);
    let src = std::fs::read_to_string(&entry)
        .map_err(|e| format!("read library entry {}: {e}", entry.display()))?;
    let program =
        parse_program_in(&src, dir).map_err(|e| format!("parse {}: {e}", entry.display()))?;
    let result = compile_with_core(&program, against.clone(), modules)
        .map_err(|e| format!("compile {}: {e}", entry.display()))?;
    Ok(result.core)
}

/// Declare a package's exported PROCESSES as importable from a module named after the
/// dependency, so `from <dep> import <Name>` resolves `<Name>` as a process at compile
/// time (the factory rides the linked Core). Type exports resolve ambiently via the
/// merged `TypeRegistry`, so they need no declaration.
fn declare_process_exports(
    mut modules: ModuleRegistry,
    dep_name: &str,
    export_processes: &[String],
) -> ModuleRegistry {
    for name in export_processes {
        modules = modules.process(dep_name, name);
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

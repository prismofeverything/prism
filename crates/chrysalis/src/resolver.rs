//! `resolver.rs` — resolve a package's dependency DAG into a linked Core + import
//! surface + the resolved graph (#67, Phases 2 + 5).
//!
//! "Depend on package P" = "colimit P's Core" (`docs/packages-decomposition.md`). A
//! package's Core comes from `.ys` (compiled here) **or** from a native Rust crate's
//! `domain_core()` — *native vs `.ys` is just where the part's Core is sourced*, and
//! [`Core::colimit`] links them uniformly. Resolution is the **colimit over the
//! dependency DAG** — *iterated `own_over`-then-`merge` to a shared apex* (the framing
//! `unify` settled): each package is reduced to its OWN theory over `base ⊔
//! its-resolved-deps` ([`Core::own_over`]), the theories joined via `Core::colimit`
//! (core's confluent, conflict-accumulating n-ary join). Because every package's own
//! theory is **memoized by name**, a **diamond** (`prog→A,B`, `A→D`, `B→D`) links `D`
//! exactly ONCE at the shared apex. **Dependency resolution is confluence.**
//!
//! **Native deps (Phase 5).** chrysalis (a fixed binary) can't link an arbitrary
//! domain crate, so a native dependency's Core is **supplied** to [`resolve_with_natives`]
//! (a generated codegen runner links the crate and calls its `prelude::core()`). The
//! resolver then colimits that Core in exactly like a `.ys` part — one resolver, two
//! Core *sources*. In-process [`resolve`] supplies no native Cores, so a native dep
//! errors with the codegen hint.
//!
//! **Semver.** Each `.ys` edge carries a [`VersionReq`]; a same-name package reached at
//! two incompatible versions is a conflict (one version per name ⇒ the apex is
//! well-defined). Cycles are detected. THIN: the linker is `Core::colimit` (prism), the
//! compiler is `compile_with_core` (lang) — this module only WIRES them ([[feedback_chrysalis_thin_layer]]).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use prism_bigraph::{Core, CoreMergeConflict};

use crate::compile::{compile_with_core, ModuleRegistry};
use crate::manifest::{Dependency, DependencySource, Manifest};
use crate::parse::parse_program_in;
use crate::prelude::{std_core, std_modules};
use crate::registry::{LocalRegistry, Registry};
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
    /// direct-dependency edges. The lockfile's pinned graph.
    pub graph: Vec<ResolvedPackage>,
}

/// One node of the resolved dependency graph (the lockfile section).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: Version,
    /// The package's directory (a `.ys` package dir, or a native crate dir).
    pub source: PathBuf,
    /// The names of this package's DIRECT dependencies (the graph edges).
    pub dependencies: Vec<String>,
}

/// Resolve `manifest`'s dependency DAG (no native dependencies). `base_modules`
/// (`std_modules()` or `std_modules_at(dir)`) seeds the import surface so the caller
/// controls `load()` path resolution. A native dependency errors — those need
/// [`resolve_with_natives`] (a codegen runner supplies their Cores).
pub fn resolve(manifest: &Manifest, base_modules: ModuleRegistry) -> Result<Resolution, String> {
    resolve_with_natives(manifest, base_modules, HashMap::new())
}

/// Resolve `manifest`'s DAG, with the Cores of its native dependencies supplied (keyed
/// by the dependency NAME — the codegen runner links each native crate and hands over
/// its `domain_core()`). The resolver colimits a native Core in exactly like a `.ys`
/// part. Honors the project's `project.lock` pins (reproducible builds).
pub fn resolve_with_natives(
    manifest: &Manifest,
    base_modules: ModuleRegistry,
    native_cores: HashMap<String, Core>,
) -> Result<Resolution, String> {
    resolve_inner(manifest, base_modules, native_cores, false)
}

/// Resolve IGNORING the lockfile pins — re-resolve every registry dependency to the
/// newest satisfying version (`chrysalis update`). The result re-pins the lock.
pub fn resolve_update(
    manifest: &Manifest,
    base_modules: ModuleRegistry,
) -> Result<Resolution, String> {
    resolve_inner(manifest, base_modules, HashMap::new(), true)
}

fn resolve_inner(
    manifest: &Manifest,
    base_modules: ModuleRegistry,
    native_cores: HashMap<String, Core>,
    ignore_lock: bool,
) -> Result<Resolution, String> {
    // The lockfile PINS registry deps for reproducible builds — honored unless we are
    // updating. A malformed / absent lock falls back to a fresh resolve.
    let lock = if ignore_lock {
        None
    } else {
        crate::lockfile::load(&manifest.dir).ok().flatten()
    };
    let mut resolver = Resolver {
        base: std_core(),
        base_modules,
        native_cores,
        registry: manifest.registry_dir().map(LocalRegistry::new),
        lock,
        root_dir: manifest.dir.clone(),
        memo: BTreeMap::new(),
        in_progress: BTreeSet::new(),
    };

    // Resolve each DIRECT dependency (recursively), then surface its exports.
    let mut root_modules = resolver.base_modules.clone();
    for dep in &manifest.dependencies {
        let name = resolver.resolve_one(dep, &manifest.dir)?;
        let exports = resolver.memo[&name].export_processes.clone();
        root_modules = declare_process_exports(root_modules, &dep.name, &exports);
    }

    // The colimit: the shared floor ⊔ every resolved package's own theory, via the
    // n-ary join `Core::colimit` (core's prism primitive — confluent over any order,
    // accumulates ALL conflicts). Each name appears ONCE in the memo, so the diamond's
    // apex is a single part — merged once.
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

/// Convenience: resolve only the linked Core (no native deps; callers with no
/// dependency imports or that build their own import surface).
pub fn resolve_core(manifest: &Manifest) -> Result<Core, String> {
    resolve(manifest, std_modules()).map(|r| r.core)
}

/// One native crate reachable in the dependency DAG (#67 — the transitive-native fix):
/// the `native_cores` key (the declaring package's edge name = the `from <edge> import`
/// module) + the crate directory the codegen runner links.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeCrateRef {
    pub edge_name: String,
    pub crate_dir: PathBuf,
}

/// Collect EVERY native crate edge reachable from `manifest` through the dependency DAG
/// — the project's own native deps PLUS those of its transitive `.ys`/registry deps. The
/// codegen runner links all of these + supplies each `domain_core()` keyed by edge.
///
/// **The transitive-native fix** (the M4 / packages-outside-this-dir enabler): a project
/// depending on a MIXED package (e.g. `synth`, whose manifest has `native: audio`) must
/// route through codegen, but codegen formerly saw only the TOP manifest's direct native
/// edges — so the in-process resolver ran and errored on the *transitive* native. The
/// resolver already walks the full DAG (and `resolve_native` resolves a transitive edge
/// once its Core is supplied); this collects the native crates along the SAME walk so the
/// runner can link them all. Deduped by crate dir (a shared native crate links once).
pub fn collect_native_crates(manifest: &Manifest) -> Result<Vec<NativeCrateRef>, String> {
    let registry = manifest.registry_dir().map(LocalRegistry::new);
    let mut out = Vec::new();
    let mut seen_packages = BTreeSet::new();
    let mut seen_crates = BTreeSet::new();
    collect_native_rec(
        manifest,
        registry.as_ref(),
        &mut out,
        &mut seen_packages,
        &mut seen_crates,
    )?;
    Ok(out)
}

fn collect_native_rec(
    manifest: &Manifest,
    registry: Option<&LocalRegistry>,
    out: &mut Vec<NativeCrateRef>,
    seen_packages: &mut BTreeSet<PathBuf>,
    seen_crates: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    for dep in &manifest.dependencies {
        match &dep.source {
            DependencySource::Native(_) => {
                let crate_dir = dep.resolved_dir(&manifest.dir);
                if seen_crates.insert(crate_dir.clone()) {
                    out.push(NativeCrateRef {
                        edge_name: dep.name.clone(),
                        crate_dir,
                    });
                }
            }
            // Recurse into `.ys` deps (path / registry) — a transitive native lives in
            // one of their manifests. A package is walked once (cycle / diamond safe).
            DependencySource::Path(_) => {
                let dir = dep.resolved_dir(&manifest.dir);
                if seen_packages.insert(dir.clone()) {
                    let dep_manifest = Manifest::load(&dir)
                        .map_err(|e| format!("collecting native deps of `{}`: {e}", dep.name))?;
                    collect_native_rec(&dep_manifest, registry, out, seen_packages, seen_crates)?;
                }
            }
            DependencySource::Registry => {
                // A registry dep is walked through the project's registry (the root's,
                // per the Phase-3 single-registry model). No registry ⇒ unreachable here.
                if let Some(reg) = registry {
                    let (_version, dir) = reg
                        .resolve(&dep.name, &dep.req)
                        .map_err(|e| format!("collecting native deps of `{}`: {e}", dep.name))?;
                    if seen_packages.insert(dir.clone()) {
                        let dep_manifest = Manifest::load(&dir)?;
                        collect_native_rec(&dep_manifest, registry, out, seen_packages, seen_crates)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// The recursive resolution state: the shared std floor, the import-surface seed, the
/// supplied native Cores, the memo (one resolved package per NAME — the colimit dedup),
/// and a cycle guard.
struct Resolver {
    base: Core,
    base_modules: ModuleRegistry,
    native_cores: HashMap<String, Core>,
    /// The project's registry (from `Manifest::registry`), resolving registry deps.
    registry: Option<LocalRegistry>,
    /// The project's `project.lock` (if present + honored) — pins registry deps to a
    /// recorded version for reproducible builds. `None` when updating or absent.
    lock: Option<Vec<crate::lockfile::LockedPackage>>,
    /// The project root (the root manifest's dir) — where `project.lock` lives, so a
    /// pin's relative `source` un-relativizes against it.
    root_dir: PathBuf,
    memo: BTreeMap<String, ResolvedPkg>,
    in_progress: BTreeSet<String>,
}

/// A fully resolved package: its identity, its OWN theory (the part strictly above
/// `base ⊔ its-deps`), its exported processes, and its direct deps.
struct ResolvedPkg {
    name: String,
    version: Version,
    source: PathBuf,
    own_core: Core,
    export_processes: Vec<String>,
    direct_deps: Vec<String>,
}

impl Resolver {
    /// Resolve one dependency edge — dispatching on where its Core comes from. Returns
    /// the package NAME (its memo key).
    fn resolve_one(&mut self, dep: &Dependency, manifest_dir: &Path) -> Result<String, String> {
        match &dep.source {
            DependencySource::Native(_) => self.resolve_native(dep, manifest_dir),
            DependencySource::Registry => self.resolve_registry(dep),
            DependencySource::Path(_) => {
                self.resolve_ys(&dep.resolved_dir(manifest_dir), &dep.req)
            }
        }
    }

    /// Resolve a **registry** dependency: the project's registry resolves `name` + the
    /// edge's `VersionReq` (the version solver) to a source dir, which then loads as a
    /// `.ys` package. So `foo: { version: '^1.0' }` + a registry == a path dep whose
    /// path the registry chose.
    fn resolve_registry(&mut self, dep: &Dependency) -> Result<String, String> {
        // Honor a lockfile PIN if it still satisfies the requirement (reproducible
        // builds) — `self.lock` is `None` when updating, so update re-resolves fresh.
        let pinned = self
            .lock
            .as_ref()
            .and_then(|lock| lock.iter().find(|p| p.name == dep.name))
            .filter(|p| dep.req.matches(&p.version))
            .map(|p| p.source.clone());
        if let Some(source) = pinned {
            let dir = self.root_dir.join(&source);
            return self.resolve_ys(&dir, &dep.req);
        }

        let registry = self.registry.as_ref().ok_or_else(|| {
            format!(
                "`{}` is a registry dependency but the project declares no \
                 `registry: '<dir>'` in its `project.ys`",
                dep.name
            )
        })?;
        let (_version, source_dir) = registry
            .resolve(&dep.name, &dep.req)
            .map_err(|e| format!("registry dependency `{}`: {e}", dep.name))?;
        self.resolve_ys(&source_dir, &dep.req)
    }

    /// Resolve a **native** dependency: its `domain_core()` must have been SUPPLIED
    /// (keyed by the edge name). Colimited like any other part — `own_over` the floor,
    /// its own processes surfaced for import. Memoized by name (a shared native dep
    /// links once).
    fn resolve_native(&mut self, dep: &Dependency, manifest_dir: &Path) -> Result<String, String> {
        let name = dep.name.clone();
        if self.memo.contains_key(&name) {
            return Ok(name);
        }
        let native_core = self.native_cores.get(&name).ok_or_else(|| {
            format!(
                "native dependency `{name}` was not supplied a Core — a project with native \
                 dependencies must be built via codegen (run with `chrysalis run`, which links \
                 the crate and hands over its `domain_core()`)"
            )
        })?;
        let own_core = native_core.own_over(&self.base);
        let export_processes = own_core
            .processes
            .type_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        self.memo.insert(
            name.clone(),
            ResolvedPkg {
                name: name.clone(),
                // The native crate's version (from its own `project.ys`/Cargo.toml) is a
                // refinement; a native dep is Cargo-versioned, not part of the `.ys` poset.
                version: Version::new(0, 0, 0),
                source: dep.resolved_dir(manifest_dir),
                own_core,
                export_processes,
                direct_deps: Vec::new(),
            },
        );
        Ok(name)
    }

    /// Resolve a `.ys` package at `dir`, requiring its version satisfy `req`. Returns
    /// the package's NAME. Idempotent per name: a second edge to an already-resolved
    /// package returns immediately (the diamond), after checking the versions agree.
    fn resolve_ys(
        &mut self,
        dir: &Path,
        req: &crate::version::VersionReq,
    ) -> Result<String, String> {
        let manifest = Manifest::load(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = manifest.name.clone();
        let version = manifest.version.unwrap_or_else(|| Version::new(0, 0, 0));

        if !req.matches(&version) {
            return Err(format!(
                "`{name}` at {} is {version}, which does not satisfy `{req}`",
                dir.display()
            ));
        }

        // Already resolved (a shared / diamond dependency)? It must be the SAME version.
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

        // Cycle guard.
        if !self.in_progress.insert(name.clone()) {
            return Err(format!("dependency cycle through `{name}`"));
        }

        // Resolve this package's OWN dependencies first (depth-first), collecting their
        // theories + the import surface its `lib.ys` sees + the graph edges.
        let mut dep_cores = Vec::new();
        let mut dep_modules = self.base_modules.clone();
        let mut direct_deps = Vec::new();
        for dep in &manifest.dependencies {
            let dep_name = self.resolve_one(dep, &manifest.dir)?;
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

        // Compile THIS package against (base ⊔ its deps) with that surface, then take
        // its OWN theory (the part strictly above what it compiled against).
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
    let src = match std::fs::read_to_string(&entry) {
        Ok(src) => src,
        // No `lib.ys` ⇒ the package has no `.ys` LIBRARY of its own — it is a manifest
        // that only wires DEPENDENCIES (a native-dep wrapper like `synth`, or a deps-only
        // umbrella). Its own theory is empty; the Core is just what it compiled against
        // (its already-linked deps). Distinct from an unreadable file, which errors.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(against.clone()),
        Err(e) => return Err(format!("read library entry {}: {e}", entry.display())),
    };
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

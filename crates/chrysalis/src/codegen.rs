//! The codegen path (decision #10): run a `.ys` that needs a **non-std package**
//! by generating, building, and caching a small *runner crate* that links the
//! package, then exec'ing it. chrysalis (one fixed binary) can't link a
//! package's natives (HiGHS, rapier2d, …) and must never depend on a downstream
//! package — so the package is linked into a generated crate instead, rust-script
//! style (invisible + cached).
//!
//! The trick that keeps it fast: the generated crate builds with
//! `CARGO_TARGET_DIR` pointed at the **workspace target**, so it reuses the
//! already-compiled `chrysalis` + package rlibs and only the tiny runner `main`
//! is compiled.
//!
//! The generated `main` calls [`crate::cli::run_command`] over the package's
//! `prelude::{registry, methods, modules}` — the SAME run path the `chrysalis`
//! binary uses over std. So `run` / `compile` / `server` are one path, not one
//! per host.
//!
//! # `project.ys` manifest grammar
//!
//! Two forms:
//!
//! ```text
//! package <name>                         # co-located: crate at the project dir
//! package <name> at <path>               # decoupled: crate at <path> (relative
//!                                        # to project.ys's dir, or absolute)
//! ```
//!
//! The **co-located** form is the original convention (`spatio-flux`'s
//! `project.ys` sits next to its `Cargo.toml`). The **decoupled** form lets a
//! research / experiment workspace live *outside* the prism monorepo and link
//! a crate that lives inside it (or anywhere on disk) — the test case is
//! [`coda`], whose project.ys is at `/home/prism/code/coda/project.ys` and
//! whose crate is at `/home/prism/code/prism/crates/coda/`. Without `at`,
//! chrysalis would not be a complete language for outside users — they'd have
//! to vendor their project into the monorepo to run it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A `project.ys` manifest: the non-std package a sibling `.ys` runs against.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// The Cargo package name to link (e.g. `spatio-flux`).
    pub package: String,
    /// The directory containing `project.ys` (where sibling `.ys` files are
    /// resolved from).
    pub dir: PathBuf,
    /// The package crate's directory (the dir containing the package's
    /// `Cargo.toml`). Defaults to `dir` (the **co-located** convention used by
    /// `spatio-flux`); a `package <name> at <path>` directive **decouples** the
    /// two, with `<path>` resolved relative to `dir` (or absolute).
    pub crate_dir: PathBuf,
}

impl Manifest {
    /// The crate's Rust identifier (`spatio-flux` → `spatio_flux`).
    fn crate_ident(&self) -> String {
        self.package.replace('-', "_")
    }
}

/// Walk up from `ys_path` looking for a `project.ys` that names a package. Found
/// ⇒ this `.ys` runs via the codegen path; absent ⇒ it's a std `.ys`, run
/// in-process. (`project.ys` is a minimal directive file, not chrysalis source:
/// a `package <name>` line, optionally followed by `at <path>`.)
pub fn find_manifest(ys_path: impl AsRef<Path>) -> Option<Manifest> {
    let start = ys_path.as_ref().canonicalize().ok()?;
    let mut dir = start.parent();
    while let Some(d) = dir {
        let manifest = d.join("project.ys");
        if manifest.is_file() {
            if let Some((package, at_path)) = parse_manifest_package(&manifest) {
                let raw_crate_dir = match at_path {
                    Some(p) if p.is_absolute() => p,
                    Some(p) => d.join(p),
                    None => d.to_path_buf(),
                };
                // Canonicalize for clean output in the generated Cargo.toml, but
                // fall back to the raw join if the dir doesn't exist yet (cargo
                // will produce a clearer error than we would).
                let crate_dir = raw_crate_dir
                    .canonicalize()
                    .unwrap_or(raw_crate_dir);
                return Some(Manifest {
                    package,
                    dir: d.to_path_buf(),
                    crate_dir,
                });
            }
        }
        dir = d.parent();
    }
    None
}

/// Extract the `package <name> [at <path>]` directive from a `project.ys`
/// (ignores comments and blank lines).
fn parse_manifest_package(path: &Path) -> Option<(String, Option<PathBuf>)> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some(rest) = line.strip_prefix("package") {
            let tokens: Vec<&str> = rest.split_whitespace().collect();
            match tokens.as_slice() {
                [name] => return Some(((*name).to_string(), None)),
                [name, "at", path] => {
                    return Some(((*name).to_string(), Some(PathBuf::from(*path))));
                }
                _ => continue,
            }
        }
    }
    None
}

/// Generate + build (cached) + exec the runner for `manifest`, forwarding
/// `subcommand` (`"run"`, …) and `args` to it. Returns the runner's exit code.
pub fn run(manifest: &Manifest, subcommand: &str, args: &[String]) -> i32 {
    let layout = match Layout::resolve(&manifest.package) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("chrysalis codegen: {e}");
            return 1;
        }
    };
    let (cargo_toml, main_rs) = render_runner(manifest, &layout);
    let binary = match ensure_built(&layout, &cargo_toml, &main_rs) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("chrysalis codegen: {e}");
            return 1;
        }
    };
    // Exec the runner: `<runner> <subcommand> <args...>`, inheriting stdio.
    let status = Command::new(&binary).arg(subcommand).args(args).status();
    match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("chrysalis codegen: exec {}: {e}", binary.display());
            1
        }
    }
}

/// The on-disk locations the codegen needs.
struct Layout {
    /// `chrysalis` crate source (baked at compile time).
    chrysalis_dir: PathBuf,
    /// Shared workspace target dir (so the runner reuses compiled deps).
    target_dir: PathBuf,
    /// The generated runner crate's directory.
    gen_dir: PathBuf,
    /// The runner binary name (unique per package).
    bin_name: String,
}

impl Layout {
    /// Resolve the on-disk layout for a runner keyed by `key` (the package name — the
    /// legacy cargo crate name, or a structured manifest's package name). The gen dir +
    /// bin name are unique per key, so distinct packages don't collide.
    fn resolve(key: &str) -> Result<Self, String> {
        // `CARGO_MANIFEST_DIR` of THIS crate = `<workspace>/crates/chrysalis`.
        let chrysalis_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace = chrysalis_dir
            .parent()
            .and_then(|c| c.parent())
            .ok_or("cannot locate workspace root from chrysalis crate dir")?
            .to_path_buf();
        let target_dir = resolve_target_dir(&workspace);
        let ident = key.replace('-', "_");
        let bin_name = format!("chrysalis_runner_{ident}");
        let gen_dir = target_dir.join("chrysalis-gen").join(key);
        Ok(Layout {
            chrysalis_dir,
            target_dir,
            gen_dir,
            bin_name,
        })
    }

    fn binary_path(&self) -> PathBuf {
        self.target_dir.join("debug").join(&self.bin_name)
    }
}

/// The workspace's ACTUAL target directory — the runner must build into it to reuse the
/// already-compiled `chrysalis` + package rlibs. `CARGO_TARGET_DIR` wins if set;
/// otherwise we ask `cargo metadata` (which honours `.cargo/config.toml`'s
/// `build.target-dir` — e.g. a target relocated off the repo volume), falling back to
/// `<workspace>/target` if cargo can't be reached. (A wrong target dir doesn't break
/// the runner — it just duplicates a full rebuild into the wrong place, slowly.)
fn resolve_target_dir(workspace: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(dir);
    }
    cargo_metadata_target_dir(workspace).unwrap_or_else(|| workspace.join("target"))
}

/// Query `cargo metadata` for the resolved `target_directory` (respects
/// `.cargo/config.toml`). `None` on any failure (cargo missing / non-zero / parse error).
fn cargo_metadata_target_dir(workspace: &Path) -> Option<PathBuf> {
    let out = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    json.get("target_directory")?.as_str().map(PathBuf::from)
}

/// Generate the runner crate's `Cargo.toml` + `src/main.rs` content for
/// `manifest`. Pure (no I/O) so it's unit-testable.
fn render_runner(manifest: &Manifest, layout: &Layout) -> (String, String) {
    let cargo_toml = format!(
        "# GENERATED by `chrysalis` — runner crate linking `{pkg}`.\n\
         [package]\n\
         name = \"chrysalis-runner-{pkg}\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         \n\
         [[bin]]\n\
         name = \"{bin}\"\n\
         path = \"src/main.rs\"\n\
         \n\
         [dependencies]\n\
         chrysalis = {{ path = {chrysalis:?} }}\n\
         {pkg} = {{ path = {pkgdir:?} }}\n\
         \n\
         # Standalone — not a member of the parent workspace.\n\
         [workspace]\n",
        pkg = manifest.package,
        bin = layout.bin_name,
        chrysalis = layout.chrysalis_dir,
        pkgdir = manifest.crate_dir,
    );

    let main_rs = format!(
        "// GENERATED by `chrysalis`. Links `{pkg}` and dispatches to the shared\n\
         // chrysalis CLI over its packages — the same run path as the std binary.\n\
         use {krate}::prelude::{{core, modules}};\n\
         \n\
         fn main() {{\n\
         \x20   let argv: Vec<String> = std::env::args().skip(1).collect();\n\
         \x20   let code = match argv.split_first() {{\n\
         \x20       Some((cmd, rest)) if cmd == \"run\" => {{\n\
         \x20           chrysalis::cli::run_command(rest, core(), modules())\n\
         \x20       }}\n\
         \x20       _ => {{\n\
         \x20           eprintln!(\"usage: runner run <file.ys> [--time T] [--<port> SOURCE ...] [--out FILE]\");\n\
         \x20           2\n\
         \x20       }}\n\
         \x20   }};\n\
         \x20   std::process::exit(code);\n\
         }}\n",
        pkg = manifest.package,
        krate = manifest.crate_ident(),
    );

    (cargo_toml, main_rs)
}

/// Ensure the runner crate is generated and built, returning the binary path.
/// Generates the runner sources (rewriting them only when changed, to avoid mtime
/// churn) then ALWAYS invokes `cargo build`: cargo's fingerprinting is the source
/// of truth for staleness, so a change to ANY dependency — `chrysalis`,
/// `prism-std`, the package itself — rebuilds the runner, while an up-to-date
/// build is near-instant. (A content-hash of only the generated files, as before,
/// could not see dependency changes and served stale binaries during development.)
fn ensure_built(layout: &Layout, cargo_toml: &str, main_rs: &str) -> Result<PathBuf, String> {
    let src_dir = layout.gen_dir.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|e| format!("create {}: {e}", src_dir.display()))?;
    write_if_changed(&layout.gen_dir.join("Cargo.toml"), cargo_toml)?;
    write_if_changed(&src_dir.join("main.rs"), main_rs)?;

    let status = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(layout.gen_dir.join("Cargo.toml"))
        // Share the workspace target so already-compiled deps are reused.
        .env("CARGO_TARGET_DIR", &layout.target_dir)
        .status()
        .map_err(|e| format!("spawn cargo: {e}"))?;
    if !status.success() {
        return Err(format!("building runner `{}` failed", layout.bin_name));
    }
    let binary = layout.binary_path();
    if !binary.is_file() {
        return Err(format!(
            "runner built but binary missing at {}",
            binary.display()
        ));
    }
    Ok(binary)
}

/// Write `content` to `path` only if it differs (avoids touching mtimes, which
/// would trigger needless rebuilds).
fn write_if_changed(path: &Path, content: &str) -> Result<(), String> {
    if std::fs::read_to_string(path)
        .map(|c| c == content)
        .unwrap_or(false)
    {
        return Ok(());
    }
    std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))
}

// ──────────────────────────────────────────────────────────────────────────────────
// Structured-manifest native runner (#67 Phase 5c) — the codegen path for a
// `def package = { …, dependencies: { d: { native: '<crate>' } } }` manifest. The
// generated runner LINKS each native crate, calls its `prelude::core()`, and routes
// through the canonical resolver [`crate::resolver::resolve_with_natives`] (one
// resolver, native Cores supplied). It GENERALIZES the legacy single-crate runner above
// to N crates + the resolver; the legacy `package <name>` directive migrates here in a
// later slice (Felleisen — one manifest, one codegen path).
// ──────────────────────────────────────────────────────────────────────────────────

use crate::manifest::Manifest as PackageManifest;

/// A native crate to link into the runner — its Cargo identity + dir.
struct NativeCrate {
    /// The Cargo package name (`spatio-flux`), read from the crate's `Cargo.toml`.
    cargo_name: String,
    /// The crate directory (the dir holding the crate's `Cargo.toml`).
    crate_dir: PathBuf,
}

impl NativeCrate {
    /// The crate's Rust identifier (`spatio-flux` → `spatio_flux`).
    fn ident(&self) -> String {
        self.cargo_name.replace('-', "_")
    }
    /// Read the crate's identity from its dir (its `Cargo.toml` package name).
    fn read(crate_dir: PathBuf) -> Result<Self, String> {
        Ok(NativeCrate {
            cargo_name: read_crate_name(&crate_dir)?,
            crate_dir,
        })
    }
}

/// The native parts of a package: its OWN native crate (top-level `native:` — the mixed
/// shape) and its native DEPENDENCY edges. (`.ys` path deps are resolved from disk by the
/// resolver, not linked here.)
struct NativeParts {
    /// The package's own native crate — its `prelude::core()` is the native BASE, its
    /// `prelude::modules()` the full own import surface.
    own: Option<NativeCrate>,
    /// Native dependency edges: `(edge name, crate)` — each edge's Core is supplied to
    /// the resolver keyed by the edge name (`from <edge> import …`).
    deps: Vec<(String, NativeCrate)>,
}

impl NativeParts {
    /// Every DISTINCT native crate for the runner's `[dependencies]` (own + deps, deduped
    /// by Cargo name — cargo errors on a duplicate path entry).
    fn crates(&self) -> Vec<&NativeCrate> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for c in self.own.iter().chain(self.deps.iter().map(|(_, c)| c)) {
            if seen.insert(c.cargo_name.clone()) {
                out.push(c);
            }
        }
        out
    }
}

/// Does this manifest have native parts (an own `native:` crate, or a `native:`
/// dependency — a Rust crate chrysalis can't link in-process)? If so, running it needs the
/// codegen path.
pub fn has_native_parts(manifest: &PackageManifest) -> bool {
    manifest.native.is_some() || manifest.dependencies.iter().any(|d| d.source.is_native())
}

/// Collect the native parts of `manifest`, reading each crate's Cargo package name.
fn native_parts(manifest: &PackageManifest) -> Result<NativeParts, String> {
    let own = match manifest.native_dir() {
        Some(dir) => Some(NativeCrate::read(dir)?),
        None => None,
    };
    let mut deps = Vec::new();
    for dep in &manifest.dependencies {
        if dep.source.is_native() {
            let krate = NativeCrate::read(dep.resolved_dir(&manifest.dir))?;
            deps.push((dep.name.clone(), krate));
        }
    }
    Ok(NativeParts { own, deps })
}

/// Read `[package].name` from a crate's `Cargo.toml` (dependency-free — a focused scan,
/// matching the legacy directive parser's no-TOML-crate approach). The structured
/// manifest names a native dep by edge-name + path; the Cargo package name (needed for
/// the runner's `[dependencies]` entry + `use` path) lives in the crate's `Cargo.toml`.
fn read_crate_name(crate_dir: &Path) -> Result<String, String> {
    let toml_path = crate_dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&toml_path)
        .map_err(|e| format!("read {}: {e}", toml_path.display()))?;
    let mut in_package = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = line.strip_prefix("name") {
                if let Some(val) = rest.trim_start().strip_prefix('=') {
                    let name = val.trim().trim_matches('"').trim_matches('\'');
                    if !name.is_empty() {
                        return Ok(name.to_string());
                    }
                }
            }
        }
    }
    Err(format!(
        "no `[package] name` in {} — a native dependency must point at a Rust crate dir",
        toml_path.display()
    ))
}

/// Render the structured runner's `Cargo.toml` + `src/main.rs`. Pure (no I/O) so it's
/// unit-testable. Links `chrysalis` + each native crate; the `run` body depends on the
/// part shape (own-native-only → `run_command` directly; native deps → the resolver).
/// Assumes own+deps was rejected upstream (`run_structured`).
fn render_structured_runner(
    manifest_name: &str,
    parts: &NativeParts,
    chrysalis_dir: &Path,
    bin_name: &str,
) -> (String, String) {
    // [dependencies]: chrysalis + each distinct native crate.
    let mut crate_deps = String::new();
    for c in parts.crates() {
        crate_deps.push_str(&format!(
            "{name} = {{ path = {dir:?} }}\n",
            name = c.cargo_name,
            dir = c.crate_dir,
        ));
    }
    let cargo_toml = format!(
        "# GENERATED by `chrysalis` — runner linking the native parts of package `{name}`.\n\
         [package]\n\
         name = \"chrysalis-runner-{name}\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         \n\
         [[bin]]\n\
         name = \"{bin}\"\n\
         path = \"src/main.rs\"\n\
         \n\
         [dependencies]\n\
         chrysalis = {{ path = {chrysalis:?} }}\n\
         {crate_deps}\
         \n\
         # Standalone — not a member of the parent workspace.\n\
         [workspace]\n",
        name = manifest_name,
        bin = bin_name,
        chrysalis = chrysalis_dir,
    );

    let run_body = match &parts.own {
        // Own native crate (own+deps rejected upstream) → run directly against its Core.
        Some(own) => render_own_native_body(own),
        // Native deps (no own crate) → supply each edge's Core to the resolver.
        None => render_native_deps_body(&parts.deps),
    };

    let main_rs = format!(
        "// GENERATED by `chrysalis`. Links the native parts of package `{name}` and runs\n\
         // through the canonical run path (own native Core directly, or the resolver for deps).\n\
         \n\
         fn main() {{\n\
         \x20   let argv: Vec<String> = std::env::args().skip(1).collect();\n\
         \x20   let code = match argv.split_first() {{\n\
         \x20       Some((cmd, rest)) if cmd == \"run\" => run(rest),\n\
         \x20       _ => {{\n\
         \x20           eprintln!(\"usage: runner run <file.ys> [--time T] [--<port> SOURCE ...] [--out FILE]\");\n\
         \x20           2\n\
         \x20       }}\n\
         \x20   }};\n\
         \x20   std::process::exit(code);\n\
         }}\n\
         \n\
         {run_body}",
        name = manifest_name,
        run_body = run_body,
    );

    (cargo_toml, main_rs)
}

/// The `fn run` body for an own-native-only package: the crate's `core()` IS the run-Core,
/// its surface = `std ⊔` the crate's own `modules()` (self-biased — std wins a name clash).
/// No resolver: there is nothing to colimit.
fn render_own_native_body(own: &NativeCrate) -> String {
    format!(
        "fn run(args: &[String]) -> i32 {{\n\
         \x20   let entry = match args.iter().find(|a| !a.starts_with(\"--\")) {{\n\
         \x20       Some(p) => p.clone(),\n\
         \x20       None => {{ eprintln!(\"chrysalis runner: no .ys entry file in args\"); return 2; }}\n\
         \x20   }};\n\
         \x20   let prog_dir = std::path::Path::new(&entry).parent().map(|d| d.to_path_buf());\n\
         \x20   let core = {ident}::prelude::core();\n\
         \x20   let modules = chrysalis::prelude::std_modules_at(prog_dir).merge({ident}::prelude::modules());\n\
         \x20   chrysalis::cli::run_command(args, core, modules)\n\
         }}\n",
        ident = own.ident(),
    )
}

/// The `fn run` body for a native-deps package: build the `native_cores` map (one entry
/// per edge), then `resolve_with_natives` (one resolver, native Cores supplied).
fn render_native_deps_body(deps: &[(String, NativeCrate)]) -> String {
    let mut inserts = String::new();
    for (edge, krate) in deps {
        inserts.push_str(&format!(
            "    native_cores.insert(\"{edge}\".to_string(), {ident}::prelude::core());\n",
            edge = edge,
            ident = krate.ident(),
        ));
    }
    format!(
        "fn run(args: &[String]) -> i32 {{\n\
         \x20   use std::collections::HashMap;\n\
         \x20   let entry = match args.iter().find(|a| !a.starts_with(\"--\")) {{\n\
         \x20       Some(p) => p.clone(),\n\
         \x20       None => {{ eprintln!(\"chrysalis runner: no .ys entry file in args\"); return 2; }}\n\
         \x20   }};\n\
         \x20   let manifest = match chrysalis::manifest::Manifest::find(&entry) {{\n\
         \x20       Some(m) => m,\n\
         \x20       None => {{ eprintln!(\"chrysalis runner: no `def package` manifest above {{entry}}\"); return 1; }}\n\
         \x20   }};\n\
         \x20   // Link each native crate + hand the resolver its `prelude::core()` (keyed by edge).\n\
         \x20   let mut native_cores = HashMap::new();\n\
         {inserts}\
         \x20   let prog_dir = std::path::Path::new(&entry).parent().map(|d| d.to_path_buf());\n\
         \x20   let base_modules = chrysalis::prelude::std_modules_at(prog_dir);\n\
         \x20   let resolution = match chrysalis::resolver::resolve_with_natives(&manifest, base_modules, native_cores) {{\n\
         \x20       Ok(r) => r,\n\
         \x20       Err(e) => {{ eprintln!(\"chrysalis runner: resolving dependencies: {{e}}\"); return 1; }}\n\
         \x20   }};\n\
         \x20   // Pin the resolved graph (best-effort — a read-only project must not fail the run).\n\
         \x20   let _ = chrysalis::lockfile::write(&resolution.graph, &manifest.dir);\n\
         \x20   chrysalis::cli::run_command(args, resolution.core, resolution.modules)\n\
         }}\n",
        inserts = inserts,
    )
}

/// Generate + build (cached) + exec the structured runner for `manifest`, forwarding
/// `subcommand` + `args`. Returns the runner's exit code. The codegen path for a
/// structured manifest with native parts (#67 Phase 5c).
///
/// # Manual verification
/// The render is unit-tested (`render_structured_runner`); this build+exec path has no
/// fast CI test (a nested `cargo build` under `cargo test` contends on the shared target
/// lock — the same reason the legacy runner above is hand-verified). Reproduce: a `.ys`
/// app whose `project.ys` declares `dependencies: { sf: { native:
/// '<repo>/crates/spatio-flux' } }` and imports `from sf import MonodKinetics`, run via
/// `chrysalis run app/main.ys --time 10`, grows a real native process (biomass 0.1→2.46,
/// glucose 10→4.09) through a generated runner. The OWN-native form: `spatio-flux`'s
/// `project.ys` is `def package = { name: 'spatio-flux', native: '.' }` — a `ys/` file
/// runs against `sf_core()` directly (its surface `std ⊔ sf_modules()`), no resolver.
pub fn run_structured(manifest: &PackageManifest, subcommand: &str, args: &[String]) -> i32 {
    let parts = match native_parts(manifest) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("chrysalis codegen: {e}");
            return 1;
        }
    };
    // Own native crate + dependencies needs the resolver to colimit the own Core WITH the
    // deps (`resolve_with_own_native`) — deferred until a mixed consumer exists (the `bio`
    // umbrella). Until then a package is own-native OR has deps, not both.
    if parts.own.is_some() && !parts.deps.is_empty() {
        eprintln!(
            "chrysalis codegen: package `{}` has BOTH an own `native:` crate and \
             dependencies — that needs `resolve_with_own_native` (deferred; no consumer yet).",
            manifest.name
        );
        return 1;
    }
    let layout = match Layout::resolve(&manifest.name) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("chrysalis codegen: {e}");
            return 1;
        }
    };
    let (cargo_toml, main_rs) =
        render_structured_runner(&manifest.name, &parts, &layout.chrysalis_dir, &layout.bin_name);
    let binary = match ensure_built(&layout, &cargo_toml, &main_rs) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("chrysalis codegen: {e}");
            return 1;
        }
    };
    let status = Command::new(&binary).arg(subcommand).args(args).status();
    match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("chrysalis codegen: exec {}: {e}", binary.display());
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_directive() {
        let dir = std::env::temp_dir().join(format!("cg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("project.ys"),
            "# a manifest\npackage spatio-flux\n",
        )
        .unwrap();
        let ys = dir.join("demo.ys");
        std::fs::write(&ys, "composite C ()\n").unwrap();

        let m = find_manifest(&ys).expect("manifest found by walking up");
        assert_eq!(m.package, "spatio-flux");
        assert_eq!(m.crate_ident(), "spatio_flux");
        // Co-located: crate_dir == dir, both the canonicalized project dir.
        assert_eq!(m.dir, m.crate_dir);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parses_decoupled_package_directive_with_at_path() {
        // `package <name> at <path>` decouples project.ys from the crate dir —
        // the test case for a `.ys` project living outside the prism monorepo
        // (see the `coda` project at /home/prism/code/coda).
        let dir = std::env::temp_dir().join(format!("cg-test-at-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("crate")).unwrap();
        std::fs::write(
            dir.join("project.ys"),
            "# decoupled manifest\npackage my-pkg at crate\n",
        )
        .unwrap();
        let ys = dir.join("main.ys");
        std::fs::write(&ys, "composite C ()\n").unwrap();

        let m = find_manifest(&ys).expect("manifest found");
        assert_eq!(m.package, "my-pkg");
        assert_eq!(m.crate_ident(), "my_pkg");
        // The project dir is the dir containing project.ys.
        let canonical_dir = dir.canonicalize().unwrap();
        assert_eq!(m.dir, canonical_dir);
        // The crate dir is `<project_dir>/crate`, canonicalized.
        assert_eq!(m.crate_dir, canonical_dir.join("crate"));
        // And it is NOT the same as the project dir — that's the whole point.
        assert_ne!(m.dir, m.crate_dir);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn at_path_resolves_absolute() {
        // An absolute `at` path is used as-is (not joined under project.ys's dir).
        let dir = std::env::temp_dir().join(format!("cg-test-abs-{}", std::process::id()));
        let abs_crate = std::env::temp_dir().join(format!("cg-test-abs-target-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&abs_crate).unwrap();
        std::fs::write(
            dir.join("project.ys"),
            format!("package other-pkg at {}\n", abs_crate.display()),
        )
        .unwrap();
        let ys = dir.join("main.ys");
        std::fs::write(&ys, "composite C ()\n").unwrap();

        let m = find_manifest(&ys).expect("manifest found");
        assert_eq!(m.package, "other-pkg");
        assert_eq!(m.crate_dir, abs_crate.canonicalize().unwrap());
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&abs_crate).ok();
    }

    #[test]
    fn no_manifest_means_std() {
        let dir = std::env::temp_dir().join(format!("cg-test-std-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ys = dir.join("plain.ys");
        std::fs::write(&ys, "composite C ()\n").unwrap();
        assert!(
            find_manifest(&ys).is_none(),
            "no project.ys ⇒ std (in-process)"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn renders_a_buildable_looking_runner() {
        let m = Manifest {
            package: "spatio-flux".into(),
            dir: "/pkg".into(),
            crate_dir: "/pkg".into(),
        };
        let layout = Layout {
            chrysalis_dir: "/cz".into(),
            target_dir: "/t".into(),
            gen_dir: "/g".into(),
            bin_name: "chrysalis_runner_spatio_flux".into(),
        };
        let (cargo, main) = render_runner(&m, &layout);
        assert!(cargo.contains("spatio-flux = { path = \"/pkg\" }"));
        assert!(cargo.contains("chrysalis = { path = \"/cz\" }"));
        assert!(cargo.contains("[workspace]"), "standalone crate");
        assert!(main.contains("use spatio_flux::prelude::{core, modules}"));
        assert!(main.contains("chrysalis::cli::run_command"));
    }

    #[test]
    fn renders_runner_with_decoupled_crate_dir() {
        // The decoupled case: the runner's Cargo.toml depends on the package at
        // the explicit `crate_dir`, not at the project dir.
        let m = Manifest {
            package: "coda".into(),
            dir: "/work/coda".into(),
            crate_dir: "/prism/crates/coda".into(),
        };
        let layout = Layout {
            chrysalis_dir: "/cz".into(),
            target_dir: "/t".into(),
            gen_dir: "/g".into(),
            bin_name: "chrysalis_runner_coda".into(),
        };
        let (cargo, _main) = render_runner(&m, &layout);
        assert!(
            cargo.contains("coda = { path = \"/prism/crates/coda\" }"),
            "depends on crate_dir, not project dir"
        );
        assert!(
            !cargo.contains("/work/coda"),
            "project dir must not leak into the runner's dependencies"
        );
    }

    // ── structured-manifest native runner (#67 Phase 5c) ──

    fn nc(cargo_name: &str, dir: &str) -> NativeCrate {
        NativeCrate {
            cargo_name: cargo_name.into(),
            crate_dir: dir.into(),
        }
    }

    #[test]
    fn renders_a_native_deps_runner() {
        // Two native deps → both crates linked, each edge supplied to the resolver.
        let parts = NativeParts {
            own: None,
            deps: vec![
                ("sf".into(), nc("spatio-flux", "/crates/spatio-flux")),
                ("audio".into(), nc("prism-audio", "/crates/prism-audio")),
            ],
        };
        let (cargo, main) =
            render_structured_runner("app", &parts, Path::new("/cz"), "chrysalis_runner_app");
        // Cargo.toml links chrysalis + each native crate (by Cargo package name + path).
        assert!(cargo.contains("chrysalis = { path = \"/cz\" }"));
        assert!(cargo.contains("spatio-flux = { path = \"/crates/spatio-flux\" }"));
        assert!(cargo.contains("prism-audio = { path = \"/crates/prism-audio\" }"));
        assert!(cargo.contains("[workspace]"), "standalone crate");
        // main.rs supplies each native edge's Core to the resolver, keyed by edge name,
        // using the crate's Rust ident (`-` → `_`) for the `prelude::core()` call.
        assert!(main
            .contains("native_cores.insert(\"sf\".to_string(), spatio_flux::prelude::core());"));
        assert!(main.contains(
            "native_cores.insert(\"audio\".to_string(), prism_audio::prelude::core());"
        ));
        // …and routes through the canonical resolver + the shared run path.
        assert!(main.contains("resolve_with_natives"));
        assert!(main.contains("chrysalis::cli::run_command"));
    }

    #[test]
    fn renders_an_own_native_runner() {
        // An OWN native crate, no deps (the mixed package) → run directly against its
        // `core()`, surface = std ⊔ its own `modules()`; NO resolver.
        let parts = NativeParts {
            own: Some(nc("spatio-flux", "/crates/spatio-flux")),
            deps: vec![],
        };
        let (cargo, main) = render_structured_runner(
            "spatio-flux",
            &parts,
            Path::new("/cz"),
            "chrysalis_runner_spatio_flux",
        );
        assert!(cargo.contains("spatio-flux = { path = \"/crates/spatio-flux\" }"));
        assert!(main.contains("let core = spatio_flux::prelude::core();"));
        assert!(main.contains(
            "chrysalis::prelude::std_modules_at(prog_dir).merge(spatio_flux::prelude::modules())"
        ));
        assert!(main.contains("chrysalis::cli::run_command(args, core, modules)"));
        assert!(
            !main.contains("resolve_with_natives"),
            "own-native-only does not need the resolver"
        );
    }

    #[test]
    fn dedups_a_shared_native_crate_in_cargo_deps() {
        // Two edges to the SAME crate → listed once in [dependencies] (cargo would error
        // on a duplicate), but BOTH get a native_cores insert (each edge is a distinct
        // resolver key).
        let parts = NativeParts {
            own: None,
            deps: vec![
                ("a".into(), nc("shared", "/c/shared")),
                ("b".into(), nc("shared", "/c/shared")),
            ],
        };
        let (cargo, main) = render_structured_runner("app", &parts, Path::new("/cz"), "bin");
        assert_eq!(
            cargo.matches("shared = { path").count(),
            1,
            "the shared crate is listed once in [dependencies]"
        );
        assert!(main.contains("native_cores.insert(\"a\""));
        assert!(main.contains("native_cores.insert(\"b\""));
    }

    #[test]
    fn has_native_parts_detects_own_and_dep() {
        use crate::manifest::Manifest;
        let own = Manifest::parse("def package = { name: 'p', native: '.' }", "/p").unwrap();
        assert!(has_native_parts(&own), "an own native: crate is a native part");
        let dep = Manifest::parse(
            "def package = { name: 'p', dependencies: { x: { native: '../c' } } }",
            "/p",
        )
        .unwrap();
        assert!(has_native_parts(&dep), "a native dependency is a native part");
        let pure = Manifest::parse(
            "def package = { name: 'p', dependencies: { y: { path: '../d' } } }",
            "/p",
        )
        .unwrap();
        assert!(!has_native_parts(&pure), "pure-.ys deps are not native parts");
    }

    #[test]
    fn reads_a_crate_package_name() {
        let dir = std::env::temp_dir().join(format!("cg-name-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "# a crate\n[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        assert_eq!(read_crate_name(&dir).unwrap(), "my-crate");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_crate_name_is_an_error() {
        let dir = std::env::temp_dir().join(format!("cg-noname-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[dependencies]\nserde = \"1\"\n").unwrap();
        assert!(read_crate_name(&dir).unwrap_err().contains("name"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

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
    let layout = match Layout::resolve(manifest) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("chrysalis codegen: {e}");
            return 1;
        }
    };
    let binary = match ensure_built(manifest, &layout) {
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
    fn resolve(manifest: &Manifest) -> Result<Self, String> {
        // `CARGO_MANIFEST_DIR` of THIS crate = `<workspace>/crates/chrysalis`.
        let chrysalis_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace = chrysalis_dir
            .parent()
            .and_then(|c| c.parent())
            .ok_or("cannot locate workspace root from chrysalis crate dir")?
            .to_path_buf();
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or(workspace.join("target"));
        let bin_name = format!("chrysalis_runner_{}", manifest.crate_ident());
        let gen_dir = target_dir.join("chrysalis-gen").join(&manifest.package);
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
         use {krate}::prelude::{{registry, methods, modules}};\n\
         \n\
         fn main() {{\n\
         \x20   let argv: Vec<String> = std::env::args().skip(1).collect();\n\
         \x20   let code = match argv.split_first() {{\n\
         \x20       Some((cmd, rest)) if cmd == \"run\" => {{\n\
         \x20           chrysalis::cli::run_command(rest, registry(), methods(), modules())\n\
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
fn ensure_built(manifest: &Manifest, layout: &Layout) -> Result<PathBuf, String> {
    let (cargo_toml, main_rs) = render_runner(manifest, layout);

    let src_dir = layout.gen_dir.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|e| format!("create {}: {e}", src_dir.display()))?;
    write_if_changed(&layout.gen_dir.join("Cargo.toml"), &cargo_toml)?;
    write_if_changed(&src_dir.join("main.rs"), &main_rs)?;

    let status = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(layout.gen_dir.join("Cargo.toml"))
        // Share the workspace target so already-compiled deps are reused.
        .env("CARGO_TARGET_DIR", &layout.target_dir)
        .status()
        .map_err(|e| format!("spawn cargo: {e}"))?;
    if !status.success() {
        return Err(format!("building runner for `{}` failed", manifest.package));
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
        assert!(main.contains("use spatio_flux::prelude::{registry, methods, modules}"));
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
}

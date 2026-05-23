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

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A `project.ys` manifest: the non-std package a sibling `.ys` runs against.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// The Cargo package name to link (e.g. `spatio-flux`).
    pub package: String,
    /// The directory containing `project.ys` (the package crate root).
    pub dir: PathBuf,
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
/// a `package <name>` line.)
pub fn find_manifest(ys_path: impl AsRef<Path>) -> Option<Manifest> {
    let start = ys_path.as_ref().canonicalize().ok()?;
    let mut dir = start.parent();
    while let Some(d) = dir {
        let manifest = d.join("project.ys");
        if manifest.is_file() {
            if let Some(package) = parse_manifest_package(&manifest) {
                return Some(Manifest { package, dir: d.to_path_buf() });
            }
        }
        dir = d.parent();
    }
    None
}

/// Extract the `package <name>` directive from a `project.ys` (ignores comments
/// and blank lines).
fn parse_manifest_package(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some(rest) = line.strip_prefix("package") {
            let name = rest.trim();
            if !name.is_empty() {
                return Some(name.to_string());
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
    let status = Command::new(&binary)
        .arg(subcommand)
        .args(args)
        .status();
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
        let target_dir =
            std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or(workspace.join("target"));
        let bin_name = format!("chrysalis_runner_{}", manifest.crate_ident());
        let gen_dir = target_dir.join("chrysalis-gen").join(&manifest.package);
        Ok(Layout { chrysalis_dir, target_dir, gen_dir, bin_name })
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
        pkgdir = manifest.dir,
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
/// Skips the build when the generated sources are unchanged AND the binary
/// exists (cache keyed by a content hash of the generated files).
fn ensure_built(manifest: &Manifest, layout: &Layout) -> Result<PathBuf, String> {
    let (cargo_toml, main_rs) = render_runner(manifest, layout);

    let mut hasher = DefaultHasher::new();
    cargo_toml.hash(&mut hasher);
    main_rs.hash(&mut hasher);
    let hash = format!("{:x}", hasher.finish());

    let src_dir = layout.gen_dir.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|e| format!("create {}: {e}", src_dir.display()))?;
    write_if_changed(&layout.gen_dir.join("Cargo.toml"), &cargo_toml)?;
    write_if_changed(&src_dir.join("main.rs"), &main_rs)?;

    let binary = layout.binary_path();
    let hash_file = layout.gen_dir.join(".chrysalis-hash");
    let fresh = binary.is_file()
        && std::fs::read_to_string(&hash_file).map(|h| h.trim() == hash).unwrap_or(false);
    if fresh {
        return Ok(binary);
    }

    eprintln!("chrysalis: building runner for `{}` (first run / changed)…", manifest.package);
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
    std::fs::write(&hash_file, &hash).map_err(|e| format!("write hash: {e}"))?;
    if !binary.is_file() {
        return Err(format!("runner built but binary missing at {}", binary.display()));
    }
    Ok(binary)
}

/// Write `content` to `path` only if it differs (avoids touching mtimes, which
/// would trigger needless rebuilds).
fn write_if_changed(path: &Path, content: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).map(|c| c == content).unwrap_or(false) {
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
        std::fs::write(dir.join("project.ys"), "# a manifest\npackage spatio-flux\n").unwrap();
        let ys = dir.join("demo.ys");
        std::fs::write(&ys, "composite C ()\n").unwrap();

        let m = find_manifest(&ys).expect("manifest found by walking up");
        assert_eq!(m.package, "spatio-flux");
        assert_eq!(m.crate_ident(), "spatio_flux");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_manifest_means_std() {
        let dir = std::env::temp_dir().join(format!("cg-test-std-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ys = dir.join("plain.ys");
        std::fs::write(&ys, "composite C ()\n").unwrap();
        assert!(find_manifest(&ys).is_none(), "no project.ys ⇒ std (in-process)");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn renders_a_buildable_looking_runner() {
        let m = Manifest { package: "spatio-flux".into(), dir: "/pkg".into() };
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
}

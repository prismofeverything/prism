//! Every `.ys` in the `quantum` PACKAGE (`packages/quantum/ys/`) actually **runs**
//! via `chrysalis run` — the package-local analogue of `ys_files_run.rs` (which
//! sweeps the `crates/chrysalis/ys/` monolith).
//!
//! When the quantum domain broke out of the monolith into `packages/quantum/`
//! (#67 decomposition, `docs/packages-decomposition.md` §7), its demos left the
//! `ys_files_run.rs` sweep behind. This harness restores that smoke coverage in
//! the demos' new home: run each for one tick and assert it exits cleanly with no
//! `error`/`panic`. It is the guard that catches runtime rot — a quantum demo that
//! stops working when the execution model / wiring semantics change — and it
//! confirms the package's pure-`.ys` resolution (std floor + relative `.ast` /
//! `.quantum-system` imports + the `stream:` `path:`) keeps working.
//!
//! The reach to the package (`../../packages/quantum/ys` from this crate's dir) is
//! the same one the per-demo regressions use (`quantum_teleportation.rs` et al.).

use std::process::Command;

#[test]
fn all_quantum_package_examples_run_clean() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/quantum/ys");
    let bin = env!("CARGO_BIN_EXE_chrysalis");

    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read packages/quantum/ys ({}): {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ys"))
        // Skip dot-prefixed entries — chiefly Emacs lock symlinks (`.#name.ys`)
        // that appear while a `.ys` is open in an editor; they would flake a sweep.
        .filter(|p| {
            !p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
        })
        .collect();
    entries.sort();

    // Guard against a wrong/empty path silently passing vacuously (e.g. if the
    // package dir is renamed without updating this reach).
    assert!(
        !entries.is_empty(),
        "no .ys files found in {} — has the quantum package moved?",
        dir.display()
    );

    let mut ran = Vec::new();
    let mut failures = Vec::new();

    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let out = Command::new(bin)
            .args(["run", path.to_str().unwrap(), "--time", "1"])
            .output()
            .expect("spawn chrysalis");
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let bad = !out.status.success()
            || stderr.to_lowercase().contains("error")
            || stderr.to_lowercase().contains("panic")
            // ExprProcess runtime errors print to stdout too.
            || stdout.to_lowercase().contains(" error");
        if bad {
            failures.push(format!(
                "{name}: exit={:?}\n  stderr: {}\n  stdout: {}",
                out.status.code(),
                stderr.trim(),
                stdout.trim()
            ));
        } else {
            ran.push(name);
        }
    }

    eprintln!("ran clean ({}): {ran:?}", ran.len());
    assert!(
        failures.is_empty(),
        "{} quantum package .ys file(s) failed to run clean:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

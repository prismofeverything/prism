//! `chrysalis new <dir>` scaffolds a project that ACTUALLY RUNS end-to-end —
//! `chrysalis run main.ys --time 5` exits clean and emits the expected output.
//! This is the guard against the scaffold drifting out of sync with the
//! current parser / CLI / std library (the same kind of rot ys_files_run.rs
//! catches for shipped examples).

use std::process::Command;

fn chrysalis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chrysalis")
}

/// A scoped tempdir at `target/scaffold-tests/<name>`, deleted on Drop so a
/// failed test doesn't leak directories under target/. Cargo serializes
/// per-test process spawns enough that this doesn't race.
struct ScratchDir(std::path::PathBuf);
impl ScratchDir {
    fn new(name: &str) -> Self {
        let mut p = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        p.push(format!("scaffold-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        Self(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn scaffolded_project_runs_clean() {
    let dir = ScratchDir::new("runs");
    let new_out = Command::new(chrysalis_bin())
        .args(["new", dir.path().to_str().unwrap()])
        .output()
        .expect("spawn chrysalis new");
    assert!(
        new_out.status.success(),
        "chrysalis new failed: {}",
        String::from_utf8_lossy(&new_out.stderr)
    );
    assert!(dir.path().join("project.ys").exists());
    assert!(dir.path().join("main.ys").exists());

    // The scaffolded project must actually run (and the std-only manifest must
    // NOT trigger the codegen path — the regression that originally bit us
    // here was a `project.ys` with a `package <name>` directive in a dir with
    // no Cargo crate).
    let run_out = Command::new(chrysalis_bin())
        .args([
            "run",
            dir.path().join("main.ys").to_str().unwrap(),
            "--time",
            "5",
        ])
        .output()
        .expect("spawn chrysalis run");
    let stdout = String::from_utf8_lossy(&run_out.stdout);
    let stderr = String::from_utf8_lossy(&run_out.stderr);
    assert!(
        run_out.status.success(),
        "chrysalis run on scaffold failed:\nstderr: {stderr}\nstdout: {stdout}"
    );
    // The Tick process accumulates count by 1 per tick; after t=5 → 5.0.
    assert!(
        stdout.contains("\"count\""),
        "expected `count` in output, got: {stdout}"
    );
    assert!(
        stdout.contains("5.0") || stdout.contains("5"),
        "expected count = 5 after 5 ticks, got: {stdout}"
    );
}

#[test]
fn scaffold_refuses_to_overwrite_without_force() {
    let dir = ScratchDir::new("overwrite");
    let first = Command::new(chrysalis_bin())
        .args(["new", dir.path().to_str().unwrap()])
        .status()
        .expect("first new");
    assert!(first.success());

    // Re-running on the same dir must fail (refuse to clobber main.ys).
    let second = Command::new(chrysalis_bin())
        .args(["new", dir.path().to_str().unwrap()])
        .output()
        .expect("second new");
    assert!(
        !second.status.success(),
        "second `new` should refuse: stderr={}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(String::from_utf8_lossy(&second.stderr).contains("--force"));

    // With --force it succeeds.
    let forced = Command::new(chrysalis_bin())
        .args(["new", dir.path().to_str().unwrap(), "--force"])
        .status()
        .expect("forced new");
    assert!(forced.success(), "--force should overwrite");
}

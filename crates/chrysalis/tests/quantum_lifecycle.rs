//! Q4-spirit demo of #36 — quantum entanglement LIFECYCLE.
//!
//! Verifies the place-graph mutates through three distinct phases as a
//! function of the joint state's factorizability:
//!
//!   tick 0 : { ab: <|++⟩> }                  — 1 system, joint state
//!   tick 1 : { a:  <|+⟩>,  b: <|+⟩> }        — 2 systems, split
//!   tick 2+: { ab2: <|++⟩> }                 — 1 system again, merged
//!
//! The mechanism is `_remove` + `_add` intents on the `systems` map,
//! driven by a single `Lifecycle` process that picks the appropriate
//! transition based on the current map's contents. This is the proof
//! that the structural composition algebra (divide/tensor) reflects
//! in the place graph as create/destroy.

use std::process::Command;

fn chrysalis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chrysalis")
}

fn ys_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    // The demo lives in the `quantum` package (#67 decomposition); reach it from
    // this crate's dir (`crates/chrysalis`) via `../../packages/quantum/ys`.
    format!("{manifest}/../../packages/quantum/ys/quantum-lifecycle.ys")
}

fn run_at_time(t: u64) -> String {
    let out = Command::new(chrysalis_bin())
        .args(["run", &ys_path(), "--time", &t.to_string()])
        .output()
        .expect("spawn chrysalis run");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "chrysalis run --time {t} failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    stdout
}

#[test]
fn initial_state_is_one_entangled_style_system() {
    let out = run_at_time(0);
    assert!(
        out.contains("\"ab\""),
        "tick 0: expected `ab` (the initial joint system) in output:\n{out}"
    );
    assert!(
        !out.contains("\"a\":") || !out.contains("\"b\":"),
        "tick 0: should NOT have separate a/b entries yet:\n{out}"
    );
    // The joint state has 4 amplitudes (bitstring keys).
    assert!(
        out.contains("\"00\""),
        "joint state should have bitstring keys:\n{out}"
    );
}

#[test]
fn after_one_tick_split_into_two_systems() {
    let out = run_at_time(1);
    assert!(
        !out.contains("\"ab\":"),
        "tick 1: `ab` should be gone (consumed by split):\n{out}"
    );
    assert!(
        out.contains("\"a\":") && out.contains("\"b\":"),
        "tick 1: should have both `a` and `b` (the factored systems):\n{out}"
    );
    // Each factor state has 2-amp single-qubit keys.
    assert!(
        out.contains("\"0\"") && out.contains("\"1\""),
        "factor states should have single-qubit keys:\n{out}"
    );
}

#[test]
fn after_two_ticks_merged_back_into_one_system() {
    let out = run_at_time(2);
    assert!(
        out.contains("\"ab2\""),
        "tick 2: expected `ab2` (the re-fused system) in output:\n{out}"
    );
    assert!(
        !out.contains("\"a\":") || !out.contains("\"b\":"),
        "tick 2: separate `a`/`b` should be gone (consumed by merge):\n{out}"
    );
    // The merged state has 4-amp bitstring keys again.
    assert!(
        out.contains("\"00\""),
        "merged state should have joint bitstring keys:\n{out}"
    );
}

#[test]
fn lifecycle_settles_after_merge() {
    // After tick 2, the system is stable at `ab2`. Verify it stays so.
    for t in [3, 5, 10] {
        let out = run_at_time(t);
        assert!(
            out.contains("\"ab2\""),
            "tick {t}: lifecycle should be stable at `ab2`:\n{out}"
        );
    }
}

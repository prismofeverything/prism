//! Streaming variant of `quantum_lifecycle.rs` — the entanglement split/merge
//! lifecycle where each `QuantumSystem` runs as its OWN `chrysalis run
//! quantum-system.ys --serve-process` child over Arrow pipes (the `stream:`
//! protocol), not as a local in-thread sub-composite.
//!
//! The ONLY change from `quantum-lifecycle.ys` is `QuantumSystem` →
//! `StreamingQuantumSystem` (a `stream<…>` protocol alias) and the slot type.
//! Per `docs/merge-protocol.md`, the structural `_remove`/`_add` ride the
//! output bridge (already update-based — proven by `grow_divide_stream.rs`),
//! so the split/merge crosses the wire unchanged.
//!
//! Phases (identical to the local lifecycle):
//!   tick 0 : { ab:  <|++⟩> }                  — 1 streaming system, joint
//!   tick 1 : { a:   <|+⟩>,  b: <|+⟩> }        — 2 streaming systems, split
//!   tick 2+: { ab2: <|++⟩> }                  — 1 streaming system, merged
//!
//! The added `is_stream_addressed` assertions guard against a silent
//! local fallback: every system node must carry a `stream:` address.

use std::process::Command;

fn chrysalis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chrysalis")
}

fn ys_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    format!("{manifest}/ys/quantum-lifecycle-stream.ys")
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

/// Every system node carries a `stream:` address — proof the children are real
/// stream subprocesses, not a silent local fallback. The address serializes as
/// `{_type: "stream", path: "…/quantum-system.ys"}`.
fn assert_stream_addressed(out: &str) {
    assert!(
        out.contains("\"_type\": \"stream\""),
        "expected stream-addressed nodes (not local fallback):\n{out}"
    );
    assert!(
        out.contains("quantum-system.ys"),
        "expected the child path quantum-system.ys in the address:\n{out}"
    );
}

#[test]
fn initial_state_is_one_streaming_system() {
    let out = run_at_time(0);
    assert!(
        out.contains("\"ab\""),
        "tick 0: expected `ab` (the initial joint system):\n{out}"
    );
    assert!(
        !out.contains("\"a\":") || !out.contains("\"b\":"),
        "tick 0: should NOT have separate a/b entries yet:\n{out}"
    );
    assert!(
        out.contains("\"00\""),
        "joint state should have bitstring keys:\n{out}"
    );
    assert_stream_addressed(&out);
}

#[test]
fn after_one_tick_split_into_two_streaming_systems() {
    let out = run_at_time(1);
    assert!(
        !out.contains("\"ab\":"),
        "tick 1: `ab` should be gone (consumed by split):\n{out}"
    );
    assert!(
        out.contains("\"a\":") && out.contains("\"b\":"),
        "tick 1: should have both `a` and `b` (the factored systems):\n{out}"
    );
    assert!(
        out.contains("\"0\"") && out.contains("\"1\""),
        "factor states should have single-qubit keys:\n{out}"
    );
    // The two children of |++⟩ are each |+⟩ = (0.7071, 0.7071).
    assert!(
        out.contains("0.7071"),
        "factor amplitudes should be 1/√2 ≈ 0.7071:\n{out}"
    );
    assert_stream_addressed(&out);
}

#[test]
fn after_two_ticks_merged_back_into_one_streaming_system() {
    let out = run_at_time(2);
    assert!(
        out.contains("\"ab2\""),
        "tick 2: expected `ab2` (the re-fused system):\n{out}"
    );
    assert!(
        !out.contains("\"a\":") || !out.contains("\"b\":"),
        "tick 2: separate `a`/`b` should be gone (consumed by merge):\n{out}"
    );
    assert!(
        out.contains("\"00\""),
        "merged state should have joint bitstring keys again:\n{out}"
    );
    assert_stream_addressed(&out);
}

#[test]
fn lifecycle_settles_after_merge_over_stream() {
    for t in [3, 5] {
        let out = run_at_time(t);
        assert!(
            out.contains("\"ab2\""),
            "tick {t}: lifecycle should be stable at `ab2`:\n{out}"
        );
        assert_stream_addressed(&out);
    }
}

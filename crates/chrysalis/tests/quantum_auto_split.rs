//! Autonomous, PHYSICS-DRIVEN split over the `stream:` protocol
//! (`quantum-auto-split.ys`). The decision is driven by `factorize` of each
//! system's live state, not by a key name:
//!
//!   tick 0 : { sep: |++⟩ (separable),  bell: Bell (entangled) }
//!   tick 1+: { sep_0: |+⟩, sep_1: |+⟩,  bell: Bell }      ← sep SPLIT, bell STAYS
//!
//! The place graph's shape comes to MATCH the entanglement structure on its
//! own (docs/bigraphs-all-the-way-down.md §VII–VIII): the separable system
//! splits into its factors; the entangled one cannot be factored, so it
//! remains one joint composite. Each system is its own `chrysalis run
//! quantum-system.ys --serve-process` child.

use std::process::Command;

fn chrysalis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chrysalis")
}

fn ys_path() -> String {
    // The demo lives in the `quantum` package (#67 decomposition); reach it from
    // this crate's dir (`crates/chrysalis`) via `../../packages/quantum/ys`.
    format!(
        "{}/../../packages/quantum/ys/quantum-auto-split.ys",
        env!("CARGO_MANIFEST_DIR")
    )
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
fn initial_two_joint_streaming_systems() {
    let out = run_at_time(0);
    assert!(
        out.contains("\"sep\"") && out.contains("\"bell\""),
        "tick 0: both `sep` and `bell` present, joint:\n{out}"
    );
    assert!(
        out.contains("\"_type\": \"stream\""),
        "systems are stream-addressed children (not a local fallback):\n{out}"
    );
}

#[test]
fn separable_splits_entangled_stays_joint() {
    let out = run_at_time(1);
    // `sep` (|++⟩) factorizes → replaced by its two single-qubit factors.
    assert!(
        !out.contains("\"sep\":"),
        "tick 1: `sep` consumed by the split:\n{out}"
    );
    assert!(
        out.contains("\"sep_0\"") && out.contains("\"sep_1\""),
        "tick 1: the two factor children present:\n{out}"
    );
    // `bell` = (|00⟩+|11⟩)/√2 is rank-2 → `factorize` says NOT separable →
    // it stays one joint composite. The structure mirrors the entanglement.
    assert!(
        out.contains("\"bell\""),
        "tick 1: entangled `bell` stays joint (cannot be factored):\n{out}"
    );
    assert!(
        out.contains("\"00\""),
        "tick 1: `bell` keeps its joint bitstring keys:\n{out}"
    );
}

#[test]
fn amplitudes_are_stable_no_accumulation() {
    // Regression guard for the snapshot-doubling bug. A persisting streaming
    // system republishes its `state` snapshot each tick; an additive parent
    // slot would DOUBLE it (0.7071 → 1.4142 → 2.8284 → …). The fix — a
    // composite's declared `overwrite[map[float]]` output port is honored in
    // `node_data_branches`, so the snapshot REPLACES — keeps it stable. The
    // doubled magnitudes must never appear.
    for t in [1, 2, 3] {
        let out = run_at_time(t);
        assert!(
            out.contains("0.7071"),
            "tick {t}: the 1/√2 amplitude is present:\n{out}"
        );
        assert!(
            !out.contains("1.41421356"),
            "tick {t}: amplitude DOUBLED — snapshot accumulation regressed:\n{out}"
        );
        assert!(
            !out.contains("2.82842712"),
            "tick {t}: amplitude quadrupled — snapshot accumulation regressed:\n{out}"
        );
    }
}

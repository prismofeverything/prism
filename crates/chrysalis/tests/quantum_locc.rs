//! Q2 of #36 — Classical wire (LOCC) regression.
//!
//! Runs `quantum-locc.ys` through the chrysalis bin and asserts the LOCC
//! invariant: Bob's prepared state always matches Alice's measured bit.
//! No quantum entanglement crosses the composite boundary — only a
//! classically-typed value on a shared slot.

use std::process::Command;

fn chrysalis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chrysalis")
}

fn ys_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    format!("{manifest}/ys/quantum-locc.ys")
}

#[test]
fn locc_bob_state_matches_alice_bit() {
    // 3 BSP ticks: H → Measure → ConditionalPrepare.
    let out = Command::new(chrysalis_bin())
        .args(["run", &ys_path(), "--time", "3"])
        .output()
        .expect("spawn chrysalis run");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "chrysalis run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );

    // The output is the LOCC composite's outputs: alice_state,
    // classical_wire, bob_state. With default seed=42, classical_wire
    // resolves to 'b1' (verified empirically) and Bob prepares |1⟩.
    assert!(
        stdout.contains("\"classical_wire\": \"b1\""),
        "expected classical_wire = 'b1' with default seed=42:\n{stdout}"
    );
    // Bob's state should be |1⟩ = {amp_0: 0.0, amp_1: 1.0}.
    assert!(
        stdout.contains("\"amp_0\": 0.0"),
        "Bob's amp_0 should be 0.0 (|1⟩):\n{stdout}"
    );
    assert!(
        stdout.contains("\"amp_1\": 1.0"),
        "Bob's amp_1 should be 1.0 (|1⟩):\n{stdout}"
    );
}

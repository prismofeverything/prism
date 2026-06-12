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
    // The demo lives in the `quantum` package (#67 decomposition); reach it from
    // this crate's dir (`crates/chrysalis`) via `../../packages/quantum/ys`.
    format!("{manifest}/../../packages/quantum/ys/quantum-locc.ys")
}

#[test]
fn locc_bob_state_matches_alice_bit() {
    let out = Command::new(chrysalis_bin())
        .args(["run", &ys_path(), "--time", "0"])
        .output()
        .expect("spawn chrysalis run");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "chrysalis run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );

    // LOCC invariant: Bob recovers Alice's classical bit (alice_bit == bob_bit)
    // and prepares |alice_bit⟩. With default seed 42 both bits are "1".
    assert!(
        stdout.contains("\"alice_bit\": \"1\""),
        "expected alice_bit = '1' with seed=42:\n{stdout}"
    );
    assert!(
        stdout.contains("\"bob_bit\": \"1\""),
        "Bob's bit should MATCH Alice's (classical correlation):\n{stdout}"
    );
    assert!(
        stdout.contains("\"1\": 1.0"),
        "Bob's state should be |1⟩:\n{stdout}"
    );
}

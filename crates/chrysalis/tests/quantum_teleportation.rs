//! Q6 of #36 — Quantum teleportation regression.
//!
//! Runs `quantum-teleportation.ys` and verifies that Bob's final state
//! matches Alice's original |ψ⟩ = 0.6|0⟩ + 0.8|1⟩ regardless of which
//! Bell-measurement outcome occurred. Each seed produces a different
//! `bits` value; the conditional Pauli correction reconstructs |ψ⟩.

use std::process::Command;

fn chrysalis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chrysalis")
}

fn ys_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    format!("{manifest}/ys/quantum-teleportation.ys")
}

#[test]
fn teleportation_reconstructs_psi_through_classical_channel() {
    // 6 BSP ticks for the chain CNOT → H → Measure → ExtractBob → BobCorrect
    // to fully propagate (5 processes, with the seam between Extract and
    // Correct needing one tick to settle bob_raw before z sees it).
    let out = Command::new(chrysalis_bin())
        .args(["run", &ys_path(), "--time", "6"])
        .output()
        .expect("spawn chrysalis run");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "chrysalis run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );

    // The original state |ψ⟩ = 0.6|0⟩ + 0.8|1⟩. Bob's final state should
    // match this within float tolerance, regardless of bits outcome —
    // the conditional Pauli correction handles all 4 cases.
    let alpha = extract_amp(&stdout, "bob_final", "amp_0");
    let beta = extract_amp(&stdout, "bob_final", "amp_1");
    assert!(
        (alpha - 0.6).abs() < 0.01,
        "bob_final.amp_0 should be 0.6 (α from |ψ⟩); got {alpha}\nfull:\n{stdout}"
    );
    assert!(
        (beta - 0.8).abs() < 0.01,
        "bob_final.amp_1 should be 0.8 (β from |ψ⟩); got {beta}\nfull:\n{stdout}"
    );
    // Sanity: norm preserved (Bob's qubit is properly renormalized).
    let norm = (alpha * alpha + beta * beta).sqrt();
    assert!(
        (norm - 1.0).abs() < 0.01,
        "bob_final norm should be 1.0; got {norm}"
    );
}

/// Pull out a specific numeric amp_X from a slot in the JSON-like output.
fn extract_amp(stdout: &str, slot: &str, key: &str) -> f64 {
    let slot_pos = stdout
        .find(&format!("\"{slot}\""))
        .unwrap_or_else(|| panic!("no slot {slot} in output:\n{stdout}"));
    let from_slot = &stdout[slot_pos..];
    let key_pos = from_slot
        .find(&format!("\"{key}\""))
        .unwrap_or_else(|| panic!("no key {key} in slot {slot}:\n{from_slot}"));
    let from_key = &from_slot[key_pos..];
    let colon = from_key.find(':').expect("colon after key");
    let after = &from_key[colon + 1..];
    let end = after
        .find(|c: char| c == ',' || c == '}' || c == '\n')
        .unwrap_or(after.len());
    after[..end].trim().parse().unwrap_or_else(|_| {
        panic!(
            "failed to parse number for slot={slot} key={key}: {:?}",
            &after[..end]
        )
    })
}

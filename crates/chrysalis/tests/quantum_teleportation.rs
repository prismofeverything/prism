//! Q6 of #36 — Quantum teleportation regression.
//!
//! Runs `quantum-teleportation.ys` and verifies Bob reconstructs Alice's state
//! |ψ⟩ = 0.6|0⟩ + 0.8|1⟩ EXACTLY. The demo reports a `fidelity` (1.0 iff Bob's
//! qubit un-prepares back to |0⟩), which holds for whichever Bell-measurement
//! outcome occurred — the conditional Pauli correction handles all four cases.
//! Only two classical bits cross; `observe` (measurement-with-collapse) does the
//! rest (Alice's measurement genuinely collapses the joint state).

use std::process::Command;

fn chrysalis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chrysalis")
}

fn ys_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    // The demo lives in the `quantum` package (#67 decomposition).
    format!("{manifest}/../../packages/quantum/ys/quantum-teleportation.ys")
}

#[test]
fn teleportation_reconstructs_psi_through_classical_channel() {
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

    let fid = field(&stdout, "fidelity");
    assert!(
        (fid - 1.0).abs() < 1e-3,
        "teleportation fidelity should be 1.0 (Bob reconstructs |ψ⟩); got {fid}\n{stdout}"
    );
}

/// Parse a top-level scalar `"name": <number>` from the JSON-ish output.
fn field(out: &str, name: &str) -> f64 {
    let pos = out
        .find(&format!("\"{name}\""))
        .unwrap_or_else(|| panic!("no field {name} in:\n{out}"));
    let after = &out[pos + name.len() + 2..];
    let colon = after.find(':').expect("colon after field");
    let rest = &after[colon + 1..];
    let end = rest
        .find(|c: char| c == ',' || c == '}' || c == '\n')
        .unwrap_or(rest.len());
    rest[..end]
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("not a number for {name}: {:?}", &rest[..end]))
}

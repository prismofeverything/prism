//! Task #4 — the load-bearing child (`quantum-self-decohere.ys`). A quantum
//! system DECODES its own entanglement: `Decohere` applies `state.cnot(0,1)`
//! while `state.separable(1)` is false, so factorizability becomes a
//! CONSEQUENCE of the child's own gate computation, not a static seed.
//!
//!   t=0 : Bell (|00⟩+|11⟩)/√2 , separable=false   (entangled)
//!   t=1 : decoded to |+⟩⊗|0⟩  , separable=false   (verdict lags one BSP tick)
//!   t=2+: |+⟩⊗|0⟩             , separable=true     (decoded; fixed point)
//!
//! Uses the `Qubits` type's `.cnot` / `.separable` METHODS — the quantum
//! operations as the type's algebra, dispatched on the live state.

use std::process::Command;

fn chrysalis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chrysalis")
}

fn ys_path() -> String {
    format!("{}/ys/quantum-self-decohere.ys", env!("CARGO_MANIFEST_DIR"))
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
fn starts_entangled() {
    let out = run_at_time(0);
    // Bell state: amplitude on |11⟩; reports NOT separable.
    assert!(
        out.contains("\"11\": 0.7071"),
        "tick 0: Bell state has |11⟩ amplitude:\n{out}"
    );
    assert!(
        out.contains("\"separable\": false"),
        "tick 0: entangled → separable false:\n{out}"
    );
}

#[test]
fn decodes_itself_to_separable() {
    // By tick 2 the child has applied CNOT (|11⟩ amplitude moved to |10⟩) AND
    // its self-verdict has caught up: separable. The decode is the child's
    // OWN computation — `state.cnot(0,1)` — not a re-read of the seed.
    let out = run_at_time(2);
    assert!(
        out.contains("\"10\": 0.7071"),
        "tick 2: decoded to |+⟩⊗|0⟩ (amplitude on |10⟩):\n{out}"
    );
    assert!(
        out.contains("\"separable\": true"),
        "tick 2: decoded → separable true:\n{out}"
    );
}

#[test]
fn stable_at_the_separable_fixed_point() {
    // Once separable, `.separable(1)` guards the gate → it holds (no re-entangle).
    for t in [3, 5] {
        let out = run_at_time(t);
        assert!(
            out.contains("\"separable\": true") && out.contains("\"10\": 0.7071"),
            "tick {t}: stable at the decoded fixed point:\n{out}"
        );
    }
}

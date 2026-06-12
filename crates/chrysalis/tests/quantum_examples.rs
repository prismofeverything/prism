//! Behavior tests for the quantum `.ys` EXAMPLES — the `.ys` file is the source
//! of truth for the science, and this runs it and asserts the golden VALUES.
//!
//! This is the Rust↔`.ys` boundary applied to tests: the Rust unit tests in
//! `crates/chrysalis/src/quantum.rs` verify the PRIMITIVES (the gate kernel's
//! amplitudes, `factorize`'s numerics, the `correlation` parity), while the
//! ALGORITHMS / compositions (a circuit builds Bell, the CHSH inequality is
//! violated, the tensor/factorize duality round-trips) live in `.ys` and are
//! verified by *running the `.ys`* — not by a Rust reimplementation. The science
//! is `.ys`; Rust is the floor it calls.
//!
//! (Sibling of `quantum_package_runs.rs`, which only smoke-tests that every demo
//! runs; this asserts the actual output values.)

use std::process::Command;

/// Run a quantum-package `.ys` demo and return its stdout (asserting it ran).
fn run_demo(file: &str) -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let path = format!("{manifest}/../../packages/quantum/ys/{file}");
    let out = Command::new(env!("CARGO_BIN_EXE_chrysalis"))
        .args(["run", &path, "--time", "0"])
        .output()
        .expect("spawn chrysalis run");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{file} failed:\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// Parse a top-level scalar field `"name": <number>` from the JSON-ish output.
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

/// Parse a nested scalar `"slot": { … "key": <number> … }`.
fn nested(out: &str, slot: &str, key: &str) -> f64 {
    let slot_pos = out
        .find(&format!("\"{slot}\""))
        .unwrap_or_else(|| panic!("no slot {slot} in:\n{out}"));
    field(&out[slot_pos..], key)
}

/// Read the `separable` boolean within a Factorization `slot`.
fn separable_flag(out: &str, slot: &str) -> bool {
    let pos = out
        .find(&format!("\"{slot}\""))
        .unwrap_or_else(|| panic!("no slot {slot} in:\n{out}"));
    let after = &out[pos..];
    let sep = after.find("\"separable\"").expect("separable field");
    let val = &after[sep + "\"separable\"".len()..];
    let colon = val.find(':').expect("colon");
    val[colon + 1..].trim_start().starts_with("true")
}

#[test]
fn chsh_ys_violates_the_classical_bound() {
    // quantum-chsh.ys is the source of truth: S = 2√2 ≈ 2.828 > 2 (the classical
    // local-hidden-variable bound). The proof that entanglement ≠ classical.
    let out = run_demo("quantum-chsh.ys");
    let s = field(&out, "chsh_S");
    let classical = field(&out, "classical_bound");
    assert!(s > classical + 0.01, "CHSH S={s} must exceed the classical bound {classical}");
    assert!((s - 2.0 * std::f64::consts::SQRT_2).abs() < 1e-4, "CHSH S={s} should be 2√2");
}

#[test]
fn circuit_ys_composes_bell_and_ghz_from_gates() {
    // quantum-circuit.ys: Bell = [h(0), cnot(0,1)] and GHZ = [h, cnot, cnot] as
    // gate compositions — the entangled states are circuits, not hand-rolled.
    let out = run_demo("quantum-circuit.ys");
    let s = 1.0 / std::f64::consts::SQRT_2;
    assert!((nested(&out, "bell", "00") - s).abs() < 1e-4);
    assert!((nested(&out, "bell", "11") - s).abs() < 1e-4);
    assert!((nested(&out, "ghz", "000") - s).abs() < 1e-4);
    assert!((nested(&out, "ghz", "111") - s).abs() < 1e-4);
}

#[test]
fn gates_ys_shows_identities_and_complex_amplitudes() {
    // quantum-gates.ys: S² = Z (so S²|1⟩ = -|1⟩) and Z|1⟩ = -|1⟩ — real-valued
    // identities; plus genuinely complex amplitudes (S/T/Y produce [re, im]).
    let out = run_demo("quantum-gates.ys");
    assert!((nested(&out, "s_squared", "1") + 1.0).abs() < 1e-4, "S² = Z");
    assert!((nested(&out, "z_one", "1") + 1.0).abs() < 1e-4, "Z|1⟩ = -|1⟩");
    // The complex gates yield `[re, im]` array amplitudes (not just bare floats).
    let s_one_pos = out.find("\"s_one\"").expect("s_one slot");
    assert!(out[s_one_pos..].trim_start().contains('['), "S|1⟩ must be a complex amplitude");
}

#[test]
fn duality_ys_round_trips_and_detects_entanglement() {
    // quantum-duality.ys: a complex separable state factorizes (round-trips), and
    // a complex entangled state does not.
    let out = run_demo("quantum-duality.ys");
    assert!(separable_flag(&out, "split"), "the separable state must factorize");
    assert!(!separable_flag(&out, "bell_split"), "the entangled state must NOT factorize");
    // round-trip: factorize-then-re-tensor reproduces the joint amplitude.
    let s = 0.7071;
    assert!((nested(&out, "joint", "00") - s).abs() < 1e-3);
    assert!((nested(&out, "recombined", "00") - s).abs() < 1e-3);
}

#[test]
fn deutsch_jozsa_ys_decides_constant_vs_balanced_in_one_query() {
    // quantum-deutsch-jozsa.ys: P(inputs = 00) = 1 for a CONSTANT f, 0 for a
    // BALANCED f — decided in a single query (classically up to 2ⁿ⁻¹+1).
    let out = run_demo("quantum-deutsch-jozsa.ys");
    assert!((field(&out, "const0_p00") - 1.0).abs() < 1e-4, "constant f → P(00)=1");
    assert!((field(&out, "const1_p00") - 1.0).abs() < 1e-4, "constant f → P(00)=1");
    assert!(field(&out, "bal_x0_p00").abs() < 1e-4, "balanced f → P(00)=0");
    assert!(field(&out, "bal_xor_p00").abs() < 1e-4, "balanced f → P(00)=0");
}

#[test]
fn grover_ys_finds_the_marked_item_with_certainty() {
    // quantum-grover.ys: one Grover iteration (n=2) finds the marked item w with
    // probability 1 — for each of three different marks.
    let out = run_demo("quantum-grover.ys");
    assert!((field(&out, "found_11") - 1.0).abs() < 1e-4, "Grover should find |11⟩");
    assert!((field(&out, "found_10") - 1.0).abs() < 1e-4, "Grover should find |10⟩");
    assert!((field(&out, "found_01") - 1.0).abs() < 1e-4, "Grover should find |01⟩");
}

#[test]
fn qft_ys_is_uniform_on_zero_and_round_trips() {
    // quantum-qft.ys: QFT|000⟩ is uniform (P of any basis = 1/8), and QFT then
    // inverse-QFT recovers the input |001⟩ exactly.
    let out = run_demo("quantum-qft.ys");
    assert!((field(&out, "uniform_p000") - 0.125).abs() < 1e-3, "QFT|000⟩ uniform → P=1/8");
    assert!((field(&out, "round_trip") - 1.0).abs() < 1e-3, "QFT⁻¹∘QFT = identity");
}

#[test]
fn superdense_ys_sends_two_bits_with_one_qubit() {
    // quantum-superdense.ys: each of the four 2-bit messages is recovered with
    // certainty — two classical bits delivered via one transmitted qubit.
    let out = run_demo("quantum-superdense.ys");
    for msg in ["recv_00", "recv_01", "recv_10", "recv_11"] {
        assert!((field(&out, msg) - 1.0).abs() < 1e-4, "{msg} should recover with P=1");
    }
}

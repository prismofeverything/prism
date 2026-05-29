//! The cross-language typed seam, the RELIABLE way: prism's `RestProcess`
//! connects to a RUNNING `process-server` (a service) by URL — it does NOT spawn
//! or manage the python lifecycle. That inline path (uv-run + the libpython
//! re-exec + reading the port off piped stdout) is fragile and rebuild-prone; a
//! service started once and connected to by URL is not.
//!
//! Start the service, then point this at it:
//!
//!   process-server/serve.sh 8765 &
//!   PRISM_SIDECAR_URL=http://127.0.0.1:8765 \
//!     cargo test -p prism-bigraph --test rest_sidecar -- --ignored --nocapture
//!
//! Skips cleanly when `PRISM_SIDECAR_URL` is unset. The typed bridge itself is
//! also covered Rust↔Rust by `rest_schema`; this confirms it Rust↔Python.

use prism_bigraph::process::Process;
use prism_bigraph::protocols::RestProcess;
use prism_bigraph::{Schema, Update, Value};

/// A → B at rate 0.7 (mass action), as the `EulerIntegrator`'s network config.
fn crn() -> Value {
    Value::tree([
        (
            "species",
            Value::List(vec![Value::String("A".into()), Value::String("B".into())]),
        ),
        (
            "reactions",
            Value::List(vec![Value::tree([
                ("reactants", Value::tree([("A", Value::float(1.0))])),
                ("products", Value::tree([("B", Value::float(1.0))])),
                ("k", Value::float(0.7)),
            ])]),
        ),
    ])
}

#[test]
#[ignore = "integration: needs a running process-server (set PRISM_SIDECAR_URL)"]
fn prism_drives_python_sidecar_over_the_typed_bridge() {
    let Ok(url) = std::env::var("PRISM_SIDECAR_URL") else {
        eprintln!(
            "skipping: start `process-server/serve.sh PORT` and set \
             PRISM_SIDECAR_URL=http://127.0.0.1:PORT"
        );
        return;
    };

    let rp = RestProcess::initialize(url, "EulerIntegrator", Value::tree([("network", crn())]))
        .expect("initialize EulerIntegrator over rest");

    // SAME TYPE SYSTEM BOTH ENDS: python's `{"state": "map[float]"}` is parsed
    // back into the real Rust Schema — not Schema::Any.
    assert_eq!(
        rp.inputs().get("state"),
        Some(&Schema::map(Schema::float())),
        "python's `map[float]` input arrives as Schema::map(float)"
    );
    assert_eq!(
        rp.outputs().get("state"),
        Some(&Schema::map(Schema::float())),
        "python's `map[float]` output arrives as Schema::map(float)"
    );

    // The computation crosses the boundary: one Euler step (A:10→9.3, B:0→0.7).
    let state = Value::tree([(
        "state",
        Value::tree([("A", Value::float(10.0)), ("B", Value::float(0.0))]),
    )]);
    let Update::Value(out) = rp.update(&state, 0.1) else {
        panic!("expected a Value update from the sidecar");
    };
    let a = out
        .get_path(&["state".into(), "A".into()])
        .and_then(|v| v.as_f64())
        .expect("state.A in the sidecar's reply");
    let b = out
        .get_path(&["state".into(), "B".into()])
        .and_then(|v| v.as_f64())
        .expect("state.B in the sidecar's reply");
    assert!((a - 9.3).abs() < 1e-9, "A: expected 9.3, got {a}");
    assert!((b - 0.7).abs() < 1e-9, "B: expected 0.7, got {b}");
}

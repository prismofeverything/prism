//! The instantiation unification, end to end: `RunProcess` drives a REST-addressed
//! proc over the running process-server, so `RunProcess[proc: <rest>]` behaves
//! exactly like `RunProcess[proc: <local>]`. This confirms that closing the
//! local-only instantiation gap (`Core::instantiate`) lets COPASI/Tellurium run
//! through the same RunProcess path as Rk4/ForwardEuler — the inner proc is now
//! instantiated protocol-aware, not via the local registry only.
//!
//! Needs a running service:
//!   process-server/serve.sh 8767 &
//!   PRISM_SIDECAR_URL=http://127.0.0.1:8767 \
//!     cargo test -p prism-std --test run_process_rest -- --ignored --nocapture
//! Skips cleanly when PRISM_SIDECAR_URL is unset.

use std::sync::Arc;

use prism_bigraph::process::Step;
use prism_bigraph::protocol::ProtocolRegistry;
use prism_bigraph::protocols::RestProtocol;
use prism_bigraph::{Core, Update, Value};
use prism_std::RunProcess;

/// A → B at rate 0.7 — the remote EulerIntegrator's network config.
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
fn run_process_drives_a_rest_proc() {
    let Ok(url) = std::env::var("PRISM_SIDECAR_URL") else {
        eprintln!("skipping: serve.sh PORT && PRISM_SIDECAR_URL=http://127.0.0.1:PORT");
        return;
    };
    let (host, port) = url
        .trim_start_matches("http://")
        .split_once(':')
        .unwrap_or(("127.0.0.1", "8765"));

    // A Core whose protocols include `rest` — exactly what the engine builds.
    let mut protocols = ProtocolRegistry::new();
    protocols.register(Arc::new(RestProtocol));
    let core = Core::new().with_protocols(Arc::new(protocols));

    // RunProcess wrapping a REST-addressed EulerIntegrator: same spec shape the
    // engine hands RunProcess for a native, but the proc is remote.
    let cfg = Value::tree([
        (
            "proc",
            Value::tree([
                (
                    "address",
                    Value::tree([
                        ("_type", Value::String("rest".into())),
                        ("process", Value::String("EulerIntegrator".into())),
                        ("host", Value::String(host.into())),
                        ("port", Value::String(port.into())),
                    ]),
                ),
                ("config", Value::tree([("network", crn())])),
            ]),
        ),
        ("runtime", Value::float(0.5)),
        ("timestep", Value::float(0.1)),
    ]);

    let mut rp = RunProcess::from_config(&cfg);
    rp.set_core(core);

    let init = Value::tree([(
        "state",
        Value::tree([("A", Value::float(10.0)), ("B", Value::float(0.0))]),
    )]);
    let Update::Value(out) = rp.update(&init) else {
        panic!("RunProcess produced no update — did it reach the rest proc?");
    };

    // The trajectory came back from the REMOTE EulerIntegrator: A decays from 10
    // across the 5 steps (runtime 0.5 / timestep 0.1).
    let a = out
        .get_path(&["timeseries".into(), "columns".into(), "A".into()])
        .and_then(|v| v.as_list())
        .expect("timeseries.columns.A from the remote trajectory");
    let first = a.first().and_then(|v| v.as_f64()).expect("first A");
    let last = a.last().and_then(|v| v.as_f64()).expect("last A");
    assert!((first - 10.0).abs() < 1e-9, "starts at A=10, got {first}");
    assert!(
        last < first - 1.0,
        "A decayed across the remote trajectory: {first} -> {last}"
    );
}

//! `chrysalis server`'s core (`std_core`) served over the rest-process protocol:
//! a remote client can build std processes / composites on it, and the lifecycle
//! cleans up. The CLI `server` subcommand is a thin wrapper around exactly this.

use std::time::Duration;

use chrysalis::prelude::std_core;
use prism_bigraph::protocols::{RestProcess, RestProcessServer};
use prism_schema::Value;

#[test]
fn std_core_has_runprocess_and_composite() {
    let core = std_core();
    assert!(
        core.processes.contains("RunProcess"),
        "std core serves RunProcess"
    );
    assert!(
        core.processes.contains("Composite"),
        "std core can build composites"
    );
}

#[test]
fn std_core_serves_a_composite_over_rest_with_cleanup() {
    let core = std_core();
    let server = RestProcessServer::start(core.clone()).expect("start server");
    std::thread::sleep(Duration::from_millis(50));

    // A trivial composite document (empty inner state, no ports) — proves the
    // server builds a composite from a doc remotely via the std core.
    let config = Value::tree([
        ("state", Value::map()),
        (
            "bridge",
            Value::tree([("inputs", Value::map()), ("outputs", Value::map())]),
        ),
    ]);
    let proc = RestProcess::initialize(server.base_url(), "Composite", config, core.clone())
        .expect("initialize a composite on the server");
    assert_eq!(
        server.live_count(),
        1,
        "the composite is live on the server"
    );

    drop(proc); // sends `end`
    assert_eq!(server.live_count(), 0, "ending the composite cleans it up");
}

#[test]
fn server_rejects_a_doc_referencing_an_unknown_process() {
    let core = std_core();
    let server = RestProcessServer::start(core.clone()).expect("start server");
    std::thread::sleep(Duration::from_millis(50));

    // A composite whose inner cell names a process the std core lacks → rejected.
    let config = Value::tree([
        (
            "state",
            Value::tree([(
                "ghost",
                Value::tree([
                    ("address", Value::String("local:NoSuchProcess".into())),
                    ("config", Value::None),
                    ("inputs", Value::map()),
                    ("outputs", Value::map()),
                ]),
            )]),
        ),
        (
            "bridge",
            Value::tree([("inputs", Value::map()), ("outputs", Value::map())]),
        ),
    ]);
    let result = RestProcess::initialize(server.base_url(), "Composite", config, core.clone());
    assert!(
        result.is_err(),
        "a doc referencing an unknown inner process must be rejected"
    );
    assert_eq!(
        server.live_count(),
        0,
        "nothing is created for a rejected doc"
    );
}

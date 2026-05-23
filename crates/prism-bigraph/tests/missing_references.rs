//! A document that references a process the core can't build is an ERROR, not a
//! silently-dropped node — `Core::missing_process_refs` finds them (recursing
//! into composite `config`/`state`), and `Engine::from_state` rejects the doc.

use prism_bigraph::core::missing_process_refs;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::{Core, Engine, Schema, Value};

/// A bare process-spec node addressed at `addr`.
fn node(addr: &str) -> Value {
    Value::tree([
        ("address", Value::String(addr.to_string())),
        ("config", Value::None),
        ("inputs", Value::map()),
        ("outputs", Value::map()),
    ])
}

#[test]
fn missing_refs_found_recursively_through_composite_config() {
    let registry = ProcessRegistry::new(); // empty — nothing is registered
    // A composite-shaped doc whose inner cell is addressed `local:Ghost`, nested
    // down inside `config.state` (where a composite carries its subdocument).
    let doc = Value::tree([(
        "env",
        Value::tree([(
            "cell",
            Value::tree([
                ("address", Value::String("local:Composite".to_string())),
                (
                    "config",
                    Value::tree([(
                        "state",
                        Value::tree([("inner", node("local:Ghost"))]),
                    )]),
                ),
            ]),
        )]),
    )]);

    let missing = missing_process_refs(&doc, &registry);
    assert!(missing.contains(&"Composite".to_string()), "top class flagged; got {missing:?}");
    assert!(
        missing.contains(&"Ghost".to_string()),
        "recurses into composite config.state to find the inner reference; got {missing:?}"
    );
}

#[test]
fn remote_addresses_are_not_reported_as_missing() {
    // A `rest:` node is validated by the remote server, not this registry.
    let registry = ProcessRegistry::new();
    let doc = Value::tree([(
        "remote",
        Value::Map(indexmap::IndexMap::from([
            (
                "address".into(),
                Value::Map(indexmap::IndexMap::from([
                    ("protocol".into(), Value::String("rest".to_string())),
                    (
                        "data".into(),
                        Value::Map(indexmap::IndexMap::from([
                            ("process".into(), Value::String("Cell".to_string())),
                            ("host".into(), Value::String("127.0.0.1".to_string())),
                            ("port".into(), Value::String("9999".to_string())),
                        ])),
                    ),
                ])),
            ),
        ])),
    )]);
    assert!(
        missing_process_refs(&doc, &registry).is_empty(),
        "remote (rest:) references are the remote side's to validate"
    );
}

#[test]
fn from_state_rejects_a_document_referencing_an_unregistered_process() {
    let state = Value::tree([("p", node("local:Ghost"))]);
    let err = Engine::from_state(Schema::Any, state, Core::new())
        .expect_err("from_state must reject a doc referencing an unregistered process");
    assert!(err.contains("Ghost"), "error names the missing process; got: {err}");
    assert!(err.contains("unregistered"), "error explains the problem; got: {err}");
}

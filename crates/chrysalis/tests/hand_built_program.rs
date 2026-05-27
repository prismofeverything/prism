//! The north star (#34): build a program AS DATA — every named entity,
//! every body expression, every port — and run it. The compile+run pipeline
//! sees no difference between a parsed Program and a Value-constructed one.
//!
//! This test builds the equivalent of:
//!
//!     process Tick ~{count :: Float} ->{count :: Float} (
//!       {count: 1.0}
//!     )
//!     composite Main ->{count :: Float} (
//!       count: 0.0 |
//!       tick: Tick ~{count: count} ->{count: count}
//!     )
//!
//! …purely from map literals, then calls `compile_value(prog).run(5.0)` and
//! asserts the result. No `.ys` source. No parser. Just data and the
//! interpreter.

use indexmap::IndexMap;
use prism_schema::{Key, Value};

/// Helper: tag a Value::Map with `_type` + extra fields.
fn map(tag: &str, fields: &[(&str, Value)]) -> Value {
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    m.insert(Key::from("_type"), Value::String(tag.into()));
    for (k, v) in fields {
        m.insert(Key::from(*k), v.clone());
    }
    Value::Map(m)
}

fn raw_map(fields: &[(&str, Value)]) -> Value {
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    for (k, v) in fields {
        m.insert(Key::from(*k), v.clone());
    }
    Value::Map(m)
}

fn port(schema: &str) -> Value {
    raw_map(&[("schema", Value::String(schema.into()))])
}

#[test]
fn hand_built_program_compiles_and_runs() {
    // The Tick body: `{count: 1.0}` — a Record literal.
    let tick_body = map(
        "Record",
        &[(
            "fields",
            raw_map(&[("count", map("Float", &[("value", Value::float(1.0))]))]),
        )],
    );

    // The Tick entity — a process with one input + one output port.
    let tick = map(
        "EntityDef",
        &[
            ("name", Value::String("Tick".into())),
            (
                "process",
                raw_map(&[
                    ("params", Value::List(vec![])),
                    ("inputs", raw_map(&[("count", port("float"))])),
                    ("outputs", raw_map(&[("count", port("float"))])),
                    ("body", tick_body),
                ]),
            ),
        ],
    );

    // The Main composite body: `count: 0.0 | tick: Tick ~{count: count} ->{count: count}`.
    let var_count = map("Var", &[("name", Value::String("count".into()))]);
    let tick_term = map(
        "Term",
        &[
            ("control", Value::String("Tick".into())),
            ("args", Value::List(vec![])),
            (
                "ports",
                raw_map(&[
                    ("inputs", raw_map(&[("count", var_count.clone())])),
                    ("outputs", raw_map(&[("count", var_count.clone())])),
                ]),
            ),
        ],
    );
    let count_zero = map(
        "KeyedEntry",
        &[
            ("key", Value::String("count".into())),
            ("value", map("Float", &[("value", Value::float(0.0))])),
        ],
    );
    let tick_entry = map(
        "KeyedEntry",
        &[
            ("key", Value::String("tick".into())),
            ("value", tick_term),
        ],
    );
    let main_body = map(
        "Parallel",
        &[("items", Value::List(vec![count_zero, tick_entry]))],
    );

    // The Main composite — no inputs, one output port.
    let main = map(
        "EntityDef",
        &[
            ("name", Value::String("Main".into())),
            (
                "composite",
                raw_map(&[
                    ("params", Value::List(vec![])),
                    ("inputs", raw_map(&[])),
                    ("outputs", raw_map(&[("count", port("float"))])),
                    ("body", main_body),
                ]),
            ),
        ],
    );

    // The whole program — two entities, hand-built.
    let program_value = map("Program", &[("entities", Value::List(vec![tick, main]))]);

    // Compile + run. No parser was involved; the program is pure data.
    let doc = chrysalis::prelude::compile_value(&program_value)
        .expect("compile_value");
    let methods = std::sync::Arc::new(chrysalis::prelude::std_methods());
    let result = methods
        .dispatch(&doc, "run", std::slice::from_ref(&Value::float(5.0)))
        .expect("doc.run(5)");

    // After 5 ticks of count += 1 (interval = 1.0 default), count = 5.0.
    let count = result.get_field("count").and_then(|v| v.as_f64());
    assert_eq!(
        count,
        Some(5.0),
        "hand-built program ran 5 ticks; count = {count:?} (expected 5.0)"
    );
}

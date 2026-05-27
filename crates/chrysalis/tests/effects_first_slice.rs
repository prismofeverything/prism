//! Algebraic effects via the homoiconic substrate (#35 slice 1).
//!
//! The same expression evaluates to different results under different
//! handler bundles. Today's eval-time effects only — handlers are
//! synthetic `Def::Function`s scoped to the `handle(expr, handlers)`
//! call, picked up by the existing call-resolution path.

use indexmap::IndexMap;
use prism_schema::{Key, Value};

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

#[test]
fn same_expr_different_handlers_different_results() {
    // Build the expression `choose(2.0, 3.0)` as data — a Call to the
    // `choose` operation with two literal arguments.
    let two = map("Float", &[("value", Value::float(2.0))]);
    let three = map("Float", &[("value", Value::float(3.0))]);
    let expr = map(
        "Call",
        &[
            ("func", map("Var", &[("name", Value::String("choose".into()))])),
            ("args", Value::List(vec![two.clone(), three.clone()])),
        ],
    );

    // Handler bundle A — "first": `choose(a, b)` returns `a`.
    let first_handler = raw_map(&[
        ("params", Value::List(vec![Value::String("a".into()), Value::String("b".into())])),
        ("body", map("Var", &[("name", Value::String("a".into()))])),
    ]);

    // Handler bundle B — "second": `choose(a, b)` returns `b`.
    let second_handler = raw_map(&[
        ("params", Value::List(vec![Value::String("a".into()), Value::String("b".into())])),
        ("body", map("Var", &[("name", Value::String("b".into()))])),
    ]);

    let handlers_a = raw_map(&[("choose", first_handler.clone())]);
    let handlers_b = raw_map(&[("choose", second_handler.clone())]);

    let result_a = chrysalis::prelude::handle(&expr, &handlers_a).expect("first");
    let result_b = chrysalis::prelude::handle(&expr, &handlers_b).expect("second");

    assert_eq!(result_a.as_f64(), Some(2.0), "first-handler returns a → 2.0");
    assert_eq!(
        result_b.as_f64(),
        Some(3.0),
        "second-handler returns b → 3.0; same expression, different handler, different result"
    );
}

#[test]
fn handlers_scope_through_nested_expressions() {
    // The handler intercepts EVERY call to `choose` no matter how deep in
    // the expression tree. Test: `add(choose(2.0, 3.0), 10.0)` — the choose
    // is one level down inside an add; the handler still catches it.
    let two = map("Float", &[("value", Value::float(2.0))]);
    let three = map("Float", &[("value", Value::float(3.0))]);
    let ten = map("Float", &[("value", Value::float(10.0))]);
    let choose_call = map(
        "Call",
        &[
            ("func", map("Var", &[("name", Value::String("choose".into()))])),
            ("args", Value::List(vec![two, three])),
        ],
    );
    // The outer expression: `choose(2.0, 3.0) + 10.0` as a BinOp tree.
    let outer = map(
        "BinOp",
        &[
            ("op", Value::String("Add".into())),
            ("lhs", choose_call),
            ("rhs", ten),
        ],
    );

    let pick_second = raw_map(&[
        ("params", Value::List(vec![Value::String("a".into()), Value::String("b".into())])),
        ("body", map("Var", &[("name", Value::String("b".into()))])),
    ]);
    let handlers = raw_map(&[("choose", pick_second)]);

    let result = chrysalis::prelude::handle(&outer, &handlers).expect("eval");
    assert_eq!(
        result.as_f64(),
        Some(13.0),
        "the handler intercepts the inner `choose(2,3)` → 3; then 3 + 10 = 13. The\n\
         outer BinOp doesn't need to know that `choose` is handled — handlers\n\
         scope through the whole expression tree."
    );
}

#[test]
fn multiple_handlers_in_one_bundle() {
    // The handler bundle is itself a Value::Map keyed by operation name. A
    // single bundle can intercept multiple effects in one expression.
    // Test: `combine(choose(1.0, 2.0), pick(3.0, 4.0))` with both `choose`
    // and `pick` handled, plus `combine` mapped to multiplication.
    let one = map("Float", &[("value", Value::float(1.0))]);
    let two = map("Float", &[("value", Value::float(2.0))]);
    let three = map("Float", &[("value", Value::float(3.0))]);
    let four = map("Float", &[("value", Value::float(4.0))]);
    let choose_call = map(
        "Call",
        &[
            ("func", map("Var", &[("name", Value::String("choose".into()))])),
            ("args", Value::List(vec![one, two])),
        ],
    );
    let pick_call = map(
        "Call",
        &[
            ("func", map("Var", &[("name", Value::String("pick".into()))])),
            ("args", Value::List(vec![three, four])),
        ],
    );
    let combine_call = map(
        "Call",
        &[
            ("func", map("Var", &[("name", Value::String("combine".into()))])),
            ("args", Value::List(vec![choose_call, pick_call])),
        ],
    );

    let first_arg = raw_map(&[
        ("params", Value::List(vec![Value::String("a".into()), Value::String("b".into())])),
        ("body", map("Var", &[("name", Value::String("a".into()))])),
    ]);
    let second_arg = raw_map(&[
        ("params", Value::List(vec![Value::String("a".into()), Value::String("b".into())])),
        ("body", map("Var", &[("name", Value::String("b".into()))])),
    ]);
    let multiply = raw_map(&[
        ("params", Value::List(vec![Value::String("x".into()), Value::String("y".into())])),
        (
            "body",
            map(
                "BinOp",
                &[
                    ("op", Value::String("Mul".into())),
                    ("lhs", map("Var", &[("name", Value::String("x".into()))])),
                    ("rhs", map("Var", &[("name", Value::String("y".into()))])),
                ],
            ),
        ),
    ]);
    let handlers = raw_map(&[
        ("choose", first_arg),
        ("pick", second_arg),
        ("combine", multiply),
    ]);

    // choose(1,2) → 1, pick(3,4) → 4, combine(1, 4) → 1 * 4 = 4.
    let result = chrysalis::prelude::handle(&combine_call, &handlers).expect("eval");
    assert_eq!(
        result.as_f64(),
        Some(4.0),
        "three effects handled at once: choose→first, pick→second,\n\
         combine→multiply; the result composes naturally."
    );
}

#[test]
fn handler_body_can_compute() {
    // `choose(a, b)` under a handler that adds them: returns a + b.
    let two = map("Float", &[("value", Value::float(2.0))]);
    let three = map("Float", &[("value", Value::float(3.0))]);
    let expr = map(
        "Call",
        &[
            ("func", map("Var", &[("name", Value::String("choose".into()))])),
            ("args", Value::List(vec![two, three])),
        ],
    );

    // Handler body: `a + b` (a BinOp).
    let add_body = map(
        "BinOp",
        &[
            ("op", Value::String("Add".into())),
            ("lhs", map("Var", &[("name", Value::String("a".into()))])),
            ("rhs", map("Var", &[("name", Value::String("b".into()))])),
        ],
    );
    let add_handler = raw_map(&[
        ("params", Value::List(vec![Value::String("a".into()), Value::String("b".into())])),
        ("body", add_body),
    ]);
    let handlers = raw_map(&[("choose", add_handler)]);

    let result = chrysalis::prelude::handle(&expr, &handlers).expect("eval");
    assert_eq!(
        result.as_f64(),
        Some(5.0),
        "the handler body is itself an expression; `a + b` evaluated against the call args."
    );
}

//! `.divide()` on a composite cell emits a binary `_divide` DIRECTIVE.
//!
//! `?c.divide()` does NOT split the snapshot itself — it returns
//! `{_divide: [{}, {}]}` (split into two, no per-daughter overrides). The actual
//! schema-driven split (`divide_by_schema(CompositeLink)`: extensive `mass`
//! halves, intensive + the spec shared) happens LATE, at apply time, via the
//! `_divide` sentinel — the SAME path the Form-3 `Divider` uses. So the two
//! division paths are ONE mechanism, and the apply splits the LIVE node
//! (including this tick's growth), conserving mass across a divide.
//!
//! The split itself is proven in `prism-bigraph/tests/cells_division.rs`; the
//! end-to-end reaction form (matching, firing, keying, CONSERVATION across the
//! dividing tick, termination) in `reaction_divide.rs` + `grow_divide_glucose.rs`.

use prism_schema::Value;

use chrysalis::fixtures::grow_divide_homoiconic as gd;

#[test]
fn divide_method_emits_a_binary_divide_directive() {
    let program = gd::program();
    let result = chrysalis::compile::compile(&program).expect("compile");

    // The cell's brand (`_type: Cell`) dispatches its `divide` method.
    let cell = Value::tree([
        ("_type", Value::String("Cell".to_string())),
        ("mass", Value::float(2.0)),
        ("body", Value::map()),
    ]);
    let directive = result
        .methods
        .dispatch(&cell, "divide", &[])
        .expect("divide dispatch");
    let parts = directive
        .get_field("_divide")
        .and_then(|v| v.as_list())
        .expect("divide emits a `{_divide: [...]}` directive");
    assert_eq!(parts.len(), 2, "binary divide: two daughters, no overrides");
    assert!(
        parts.iter().all(|p| p.as_map().is_some_and(|m| m.is_empty())),
        "no per-daughter overrides — the apply does the schema-driven split"
    );
}

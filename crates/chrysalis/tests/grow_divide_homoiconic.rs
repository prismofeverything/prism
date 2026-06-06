//! Type-relative, schema-driven `.divide()` on a composite cell.
//!
//! `.divide()` is the cell's OWN schema-driven split: it divides the cell by its
//! `CompositeLink` representation (mass = extensive → halves; intensive fields +
//! the spec are shared), returning the division PRODUCTS as a list — it invents
//! no keys, the container owns them. This is the SAME `divide_by_schema(
//! CompositeLink)` the Form-3 `_divide`/Divider path uses, so the two division
//! paths are ONE mechanism.
//!
//! The end-to-end reaction-driven form — `?c :: Cell[mass:?m] => ?c.divide()` in
//! a parent BRS, the firing consuming the mother + keying daughters + conserving
//! mass + terminating — is proven in `reaction_divide.rs`. Form-3 / streamed
//! division is proven in `grow_divide_stream.rs` and
//! `prism-bigraph/tests/cells_division.rs`.

use prism_schema::Value;

use chrysalis::fixtures::grow_divide_homoiconic as gd;

#[test]
fn divide_method_is_type_relative_and_schema_driven() {
    // `.divide()` dispatched on the cell's brand (`_type: Cell`), divides by the
    // cell's REAL `CompositeLink` representation (mass = extensive → Delta), and
    // runs the schema-driven split: mass halves, the rest shared. No literal
    // `mass / 2`, no sentinel — divide is type-relative. The method SPLITS,
    // returning the PRODUCTS as a list; it invents no keys — the container
    // (the firing reaction) owns the daughters' keys.
    let program = gd::program();
    let result = chrysalis::compile::compile(&program).expect("compile");

    let cell = Value::tree([
        ("_type", Value::String("Cell".to_string())),
        ("mass", Value::float(2.0)),
        ("body", Value::map()),
    ]);
    let daughters = result
        .methods
        .dispatch(&cell, "divide", &[])
        .expect("divide dispatch");
    let ds = daughters.as_list().expect("daughters are a list of products");
    assert_eq!(ds.len(), 2, "two daughters");
    for d in ds {
        assert_eq!(
            d.get_field("mass").and_then(|v| v.as_f64()),
            Some(1.0),
            "extensive mass halved by the schema"
        );
        assert_eq!(
            d.get_field("_type").and_then(|v| v.as_str()),
            Some("Cell"),
            "_type (the brand) shared"
        );
    }
}

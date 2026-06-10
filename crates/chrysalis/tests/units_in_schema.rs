//! #71 Step 4 — END-TO-END: a `.ys` `Quantity[unit: …, extensive]` carries its
//! DIMENSION into the derived schema, not just its extensivity.
//!
//! `ys/units.ys` declares `unit pg : [mass] = 1e-12 kg` and types the cell's
//! `mass` as `Quantity[unit: pg, extensive]`. Before #71 the dimension was erased
//! at lowering (mass became a bare `Delta`); now the program-aware lowering
//! resolves the unit's dimension and carries it (`Schema::delta_dim([mass])`), so
//! the schema records *what the magnitude measures* — and therefore serializes,
//! wires, and checks dimensionally (`docs/units-in-the-schema.md`).

use chrysalis::compile::compile;
use chrysalis::parse::parse_program;
use prism_schema::Schema;
use prism_schema::units::Dimension;

const UNITS_YS: &str = include_str!("../ys/units.ys");

/// The schema of the first `mass`-keyed `Tree` branch anywhere in `s`.
fn find_mass(s: &Schema) -> Option<&Schema> {
    use Schema::*;
    match s {
        Tree { branches } => branches
            .iter()
            .find(|(k, _)| k.as_str() == "mass")
            .map(|(_, v)| v)
            .or_else(|| branches.values().find_map(find_mass)),
        Map { value } => find_mass(value),
        List { element } => find_mass(element),
        Array { element, .. } => find_mass(element),
        Overwrite { inner } | Maybe { inner } | Const { inner } | Quote { inner } => {
            find_mass(inner)
        }
        _ => None,
    }
}

#[test]
fn ys_quantity_carries_its_dimension_into_the_schema() {
    let program = parse_program(UNITS_YS).expect("parse units.ys");
    let result = compile(&program).expect("compile units.ys");
    let schema = &result.topology.state_schema;

    let mass =
        find_mass(schema).unwrap_or_else(|| panic!("no `mass` slot in derived schema: {schema:#?}"));
    match mass {
        Schema::Delta { dimension, .. } => assert_eq!(
            dimension.as_ref(),
            Some(&Dimension::base("mass")),
            "the `mass` Delta carries the [mass] dimension (#71 Step 4): {mass:?}"
        ),
        other => panic!("expected `mass` to be a dimensioned Delta, got: {other:#?}"),
    }
}

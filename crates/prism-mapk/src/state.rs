//! Initial cell state for the MAPK BRS.

use prism_schema::Value;

fn val_str(s: &str) -> Value {
    Value::String(s.to_string())
}

fn erk(name: &str) -> Value {
    Value::tree([
        ("_type", val_str("ERK")),
        ("name", val_str(name)),
    ])
}

/// Nested-compartment cell with MEK in the cytoplasm and an
/// assortment of ERK / pERK copies seeded to drive the cycle.
///
/// ```text
/// Cell
/// └── cytoplasm    (MEK, erk0=pERK, erk1=ERK, erk2=ERK)
///     ├── nucleus  (empty initially)
///     └── er_lumen (erk3=ERK)
/// ```
pub fn initial_mapk_state() -> Value {
    Value::tree([
        ("_type", val_str("Cell")),
        (
            "cytoplasm",
            Value::tree([
                ("_type", val_str("Compartment")),
                ("kind", Value::tree([("_type", val_str("Cytoplasm"))])),
                ("mek", Value::tree([("_type", val_str("MEK"))])),
                (
                    "erk0",
                    Value::tree([
                        ("_type", val_str("pERK")),
                        ("name", val_str("erk0")),
                    ]),
                ),
                ("erk1", erk("erk1")),
                ("erk2", erk("erk2")),
                (
                    "nucleus",
                    Value::tree([
                        ("_type", val_str("Compartment")),
                        ("kind", Value::tree([("_type", val_str("Nucleus"))])),
                    ]),
                ),
                (
                    "er_lumen",
                    Value::tree([
                        ("_type", val_str("Compartment")),
                        ("kind", Value::tree([("_type", val_str("ERLumen"))])),
                        ("erk3", erk("erk3")),
                    ]),
                ),
            ]),
        ),
    ])
}

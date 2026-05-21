//! Schema derivation for chrysalis — making the language fully
//! schema-aware (your `project_schema_dispatch` direction).
//!
//! Today the runtime lowering ([`crate::runtime::expr_process::lower_schema`])
//! stamps `Schema::Any` on custom/composite types, so the engine can't
//! see `ProcessLink`/`CompositeLink`, falls back to address-scanning,
//! can't dispatch methods cleanly, and can't validate wirings at compile
//! time. This module derives the *real* schema from the AST:
//!
//!   - `process P …`  → [`Schema::ProcessLink`]
//!   - `step S …`     → [`Schema::StepLink`]
//!   - `composite C …`→ [`Schema::CompositeLink`] (with the inner-state Tree)
//!   - data slots     → their value schema (`Float`/`Map`/`Custom`/…)
//!
//! Step 1 of the plan: derive + test. Threading this into
//! `topology.state_schema` (so the engine uses the typed path) is the
//! next, careful step.

use indexmap::IndexMap;

use prism_schema::{Key, Schema};

use crate::ast::{CompositeDef, Def, Expr, PortDecl, Program, SchemaExpr};

/// Lower a [`SchemaExpr`] **with the program in scope**, so a `custom(Name)`
/// that names a *definer* (composite / process / step) expands to that
/// definer's schema instead of an opaque [`Schema::Custom`].
///
/// This is the fix for the composite-as-`Custom` category error: a composite
/// is **not** a registered data type (it lives in the `ProcessRegistry`, not
/// the `TypeRegistry`), so leaving `custom("Cell")` as `Schema::Custom` makes
/// `apply` try a `TypeRegistry` dispatch that finds nothing and falls back to
/// a **blind replace** — destroying cell state (mass stops growing, division
/// never arms). A composite has its own type:
///   - as a *map element* (a data instance sitting in a parent, e.g.
///     `cells: map[Cell]`) → its **instance schema**
///     ([`composite_instance_schema`]): the exported, matchable scalar fields
///     (extensive → `Delta`, intensive → `Float`), which is exactly what
///     patterns match and `.divide()` dispatches against;
///   - a process / step → its [`Schema::ProcessLink`] / [`Schema::StepLink`].
/// Genuine data types (no matching definer) stay [`Schema::Custom`].
pub fn lower_schema_in_program(s: &SchemaExpr, program: &Program) -> Schema {
    match s {
        SchemaExpr::Map(inner) => Schema::Map {
            value: Box::new(lower_schema_in_program(inner, program)),
        },
        SchemaExpr::List(inner) => Schema::List {
            element: Box::new(lower_schema_in_program(inner, program)),
        },
        SchemaExpr::Array { shape, element } => Schema::Array {
            shape: shape.clone(),
            element: Box::new(lower_schema_in_program(element, program)),
        },
        SchemaExpr::Custom { name, .. } => match program.lookup(name) {
            Some(Def::Composite(c)) => composite_instance_schema(c),
            Some(d @ (Def::Process(_) | Def::Step(_))) => def_schema(d, program),
            // Genuine data type (or unknown) → the opaque-but-dispatchable form.
            _ => lower_schema(s),
        },
        // Non-container, non-custom: identical to the program-free lowering.
        _ => lower_schema(s),
    }
}

/// Lower a surface [`SchemaExpr`] to a prism [`Schema`], schema-complete:
/// custom types become [`Schema::Custom`] (so the `TypeRegistry`
/// dispatches their methods) rather than `Any`.
///
/// Program-unaware: a `custom(Name)` always becomes [`Schema::Custom`]. When a
/// program is in scope (schema derivation for `topology.state_schema`), prefer
/// [`lower_schema_in_program`] so composite/process/step references expand to
/// their real schema instead of an opaque data-type reference.
pub fn lower_schema(s: &SchemaExpr) -> Schema {
    match s {
        SchemaExpr::Any => Schema::Any,
        SchemaExpr::Bool => Schema::Bool { default: None },
        SchemaExpr::Int => Schema::Integer { default: None },
        SchemaExpr::Float => Schema::Float { default: None },
        SchemaExpr::String => Schema::String { default: None },
        SchemaExpr::Map(inner) => Schema::Map {
            value: Box::new(lower_schema(inner)),
        },
        SchemaExpr::List(inner) => Schema::List {
            element: Box::new(lower_schema(inner)),
        },
        SchemaExpr::Custom { name, params } => Schema::Custom {
            name: name.clone(),
            parameters: params
                .iter()
                .enumerate()
                .map(|(i, p)| (Key::from(i.to_string().as_str()), lower_schema(p)))
                .collect(),
        },
        // `@`-typed (self) slots resolve against the enclosing composite;
        // left to the threading step.
        SchemaExpr::SelfType => Schema::Any,
        // Unit *scale* erases to a bare magnitude at runtime (the
        // dimensional check is the chrysalis check pass). But EXTENSIVITY is
        // a divide-time property, not arithmetic — it must survive into the
        // schema so `divide_by_schema` splits extensive quantities and
        // shares intensive ones. Extensive → `Delta` (additive, halves on
        // divide); intensive → `Float` (shares).
        SchemaExpr::Quantity { extensive, .. } => {
            if *extensive {
                Schema::Delta { default: None }
            } else {
                Schema::Float { default: None }
            }
        }
        SchemaExpr::Array { shape, element } => Schema::Array {
            shape: shape.clone(),
            element: Box::new(lower_schema(element)),
        },
    }
}

/// The schema of a composite INSTANCE as it sits in a parent map — its
/// exported scalar data fields (the bridged outputs that land as siblings,
/// e.g. `mass`). These carry the divide semantics: an extensive `Mass`
/// output is a `Delta` (halves), an intensive one a `Float` (shares). Other
/// keys (`_type`, `id`, the subengine body) aren't in the branch set, so
/// `divide_by_schema` shares them. This is what a value method like
/// `.divide()` dispatches against — relative to the value's type.
pub fn composite_instance_schema(def: &CompositeDef) -> Schema {
    let mut branches: IndexMap<Key, Schema> = IndexMap::new();
    for (name, decl) in &def.interface.outputs {
        let s = lower_schema(&decl.schema);
        if matches!(
            s,
            Schema::Float { .. } | Schema::Delta { .. } | Schema::Integer { .. }
        ) {
            branches.insert(Key::from(name.as_str()), s);
        }
    }
    Schema::Tree { branches }
}

fn port_schemas(ports: &IndexMap<crate::ast::Name, PortDecl>) -> IndexMap<Key, Schema> {
    ports
        .iter()
        .map(|(n, d)| (Key::from(n.as_str()), lower_schema(&d.schema)))
        .collect()
}

/// The link schema for a definition as it sits in the state tree.
pub fn def_schema(def: &Def, program: &Program) -> Schema {
    match def {
        Def::Process(p) => Schema::ProcessLink {
            inputs: port_schemas(&p.interface.inputs),
            outputs: port_schemas(&p.interface.outputs),
            interval: 1.0,
        },
        Def::Step(s) => Schema::StepLink {
            inputs: port_schemas(&s.interface.inputs),
            outputs: port_schemas(&s.interface.outputs),
            priority: 0.0,
        },
        Def::Composite(c) => Schema::CompositeLink {
            inputs: port_schemas(&c.interface.inputs),
            outputs: port_schemas(&c.interface.outputs),
            interval: 1.0,
            inner_schema: Box::new(composite_inner_schema(c, program)),
        },
        _ => Schema::Any,
    }
}

/// A composite's INNER state schema: a `Tree` whose branches are the
/// body's slots. Data slots are typed from the composite's params /
/// output ports; sub-process / sub-composite slots get their link
/// schema.
pub fn composite_inner_schema(def: &CompositeDef, program: &Program) -> Schema {
    // Slot types we can resolve by name: config params + declared ports.
    // Program-aware so a `custom(CompositeName)` param (e.g.
    // `cells: map[Cell]`) expands to the composite's instance schema rather
    // than an opaque `Custom` — see [`lower_schema_in_program`].
    let mut known: IndexMap<String, Schema> = IndexMap::new();
    for p in &def.params {
        known.insert(p.name.clone(), lower_schema_in_program(&p.schema, program));
    }
    for (n, d) in def.interface.inputs.iter().chain(def.interface.outputs.iter()) {
        known
            .entry(n.clone())
            .or_insert_with(|| lower_schema_in_program(&d.schema, program));
    }

    let mut branches: IndexMap<Key, Schema> = IndexMap::new();
    collect_branches(&def.body, program, &known, &mut branches);
    Schema::Tree { branches }
}

fn collect_branches(
    e: &Expr,
    program: &Program,
    known: &IndexMap<String, Schema>,
    out: &mut IndexMap<Key, Schema>,
) {
    match e {
        Expr::Parallel(items) => {
            for i in items {
                collect_branches(i, program, known, out);
            }
        }
        Expr::KeyedEntry { key, value } => {
            if let Some(k) = key.as_plain() {
                out.insert(Key::from(k.as_str()), branch_schema(value, program, known));
            }
        }
        _ => {}
    }
}

/// Schema of a single body-entry value.
fn branch_schema(e: &Expr, program: &Program, known: &IndexMap<String, Schema>) -> Schema {
    match e {
        // A sub-term whose control names a definer → its link schema.
        Expr::Term { control, .. } => match program.lookup(control) {
            Some(d @ (Def::Process(_) | Def::Step(_) | Def::Composite(_))) => def_schema(d, program),
            _ => Schema::Any, // built-ins (BRS), plain ions, etc.
        },
        // A bare reference (`mass: mass`) → the param/port's schema.
        Expr::Var(n) => known.get(n).cloned().unwrap_or(Schema::Any),
        Expr::Float(_) => Schema::Float { default: None },
        Expr::Int(_) => Schema::Integer { default: None },
        Expr::Bool(_) => Schema::Bool { default: None },
        Expr::Str(_) => Schema::String { default: None },
        Expr::Map(_) => Schema::Map {
            value: Box::new(Schema::Any),
        },
        _ => Schema::Any,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::grow_divide;

    fn k(s: &str) -> Key {
        Key::from(s)
    }

    #[test]
    fn extensive_quantity_lowers_to_delta_intensive_to_float() {
        use crate::ast::{SchemaExpr, UnitExpr};
        // Extensive (mass) must carry into the schema as `Delta` so divide
        // halves it; intensive (rate) as `Float` so divide shares it.
        let mass = SchemaExpr::quantity(UnitExpr::named("kg"), true, false);
        let rate = SchemaExpr::quantity(UnitExpr::per("s"), false, false);
        assert!(
            matches!(lower_schema(&mass), Schema::Delta { .. }),
            "extensive Quantity → Delta"
        );
        assert!(
            matches!(lower_schema(&rate), Schema::Float { .. }),
            "intensive Quantity → Float"
        );
    }

    #[test]
    fn process_becomes_process_link() {
        let prog = grow_divide::program();
        let grow = def_schema(prog.lookup("Grow").unwrap(), &prog);
        match grow {
            Schema::ProcessLink { inputs, outputs, .. } => {
                assert!(inputs.contains_key(&k("mass")));
                assert!(inputs.contains_key(&k("interval")));
                assert!(outputs.contains_key(&k("mass")));
                assert!(matches!(inputs.get(&k("mass")), Some(Schema::Float { .. })));
            }
            other => panic!("Grow should be a ProcessLink, got {other:?}"),
        }
    }

    #[test]
    fn composite_becomes_composite_link_with_typed_inner_tree() {
        let prog = grow_divide::program();
        let cell = def_schema(prog.lookup("Cell").unwrap(), &prog);
        let Schema::CompositeLink {
            outputs,
            inner_schema,
            ..
        } = cell
        else {
            panic!("Cell should be a CompositeLink");
        };
        // Internal-division Cell exposes `environment` (where it deposits
        // daughters), not `mass` — its mass is encapsulated.
        assert!(outputs.contains_key(&k("environment")));
        // Inner state: `mass` is a Float data slot; `grow` is a nested
        // ProcessLink — no `Any` in sight.
        match inner_schema.as_ref() {
            Schema::Tree { branches } => {
                assert!(
                    matches!(branches.get(&k("mass")), Some(Schema::Float { .. })),
                    "mass slot should be Float, got {:?}",
                    branches.get(&k("mass"))
                );
                assert!(
                    matches!(branches.get(&k("grow")), Some(Schema::ProcessLink { .. })),
                    "grow slot should be a ProcessLink, got {:?}",
                    branches.get(&k("grow"))
                );
            }
            other => panic!("inner schema should be a Tree, got {other:?}"),
        }
    }

    #[test]
    fn environment_inner_tree_types_the_cells_map() {
        let prog = grow_divide::program();
        let env = def_schema(prog.lookup("Environment").unwrap(), &prog);
        let Schema::CompositeLink { inner_schema, .. } = env else {
            panic!("Environment should be a CompositeLink");
        };
        let Schema::Tree { branches } = inner_schema.as_ref() else {
            panic!("inner should be a Tree");
        };
        // `cells: map[Cell]` → Map of the Cell composite's INSTANCE schema
        // (its exported scalar fields), NOT an opaque `Custom("Cell")`. A
        // composite is not a registered data type, so threading `Custom` makes
        // `apply` blind-replace the cell; expanding to the instance schema
        // keeps cell updates structural (exported `mass` is a `Delta` that
        // grows additively and halves on divide). The internal-division Cell
        // here exposes only the non-scalar `environment`, so its instance
        // schema is a (currently empty) `Tree` — the point is it's a `Tree`,
        // not `Custom`.
        match branches.get(&k("cells")) {
            Some(Schema::Map { value }) => {
                assert!(
                    matches!(value.as_ref(), Schema::Tree { .. }),
                    "cells element should be the composite instance schema (a Tree), \
                     not an opaque Custom; got {value:?}"
                );
            }
            other => panic!("cells should be a Map, got {other:?}"),
        }
    }
}

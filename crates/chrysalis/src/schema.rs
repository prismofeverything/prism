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

/// Lower a surface [`SchemaExpr`] to a prism [`Schema`], schema-complete:
/// custom types become [`Schema::Custom`] (so the `TypeRegistry`
/// dispatches their methods) rather than `Any`.
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
        // Units erase to a Float magnitude at runtime; the dimensional
        // metadata lives in the check pass (`crate::units`), not the
        // runtime schema.
        SchemaExpr::Quantity { .. } => Schema::Float { default: None },
        SchemaExpr::Array { shape, element } => Schema::Array {
            shape: shape.clone(),
            element: Box::new(lower_schema(element)),
        },
    }
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
    let mut known: IndexMap<String, Schema> = IndexMap::new();
    for p in &def.params {
        known.insert(p.name.clone(), lower_schema(&p.schema));
    }
    for (n, d) in def.interface.inputs.iter().chain(def.interface.outputs.iter()) {
        known.entry(n.clone()).or_insert_with(|| lower_schema(&d.schema));
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
        // `cells: cells` → Map[Cell] → Map of Custom("Cell").
        match branches.get(&k("cells")) {
            Some(Schema::Map { value }) => {
                assert!(
                    matches!(value.as_ref(), Schema::Custom { name, .. } if name == "Cell"),
                    "cells element should be Custom(Cell), got {value:?}"
                );
            }
            other => panic!("cells should be a Map, got {other:?}"),
        }
    }
}

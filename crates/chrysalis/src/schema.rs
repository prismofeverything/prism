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

use crate::ast::{CompositeDef, ContractRef, Def, Expr, PortDecl, Program, SchemaExpr};

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
    lower_in_prog(s, program, &mut Vec::new())
}

/// [`lower_schema_in_program`] threading a `building` stack of composites
/// currently being expanded — so a composite reference builds its REAL inner
/// schema, with only a back-edge to an ancestor composite (e.g. `Cell`'s
/// `environment` → the enclosing `Environment`) marked, never looping. (#9.)
fn lower_in_prog(s: &SchemaExpr, program: &Program, building: &mut Vec<crate::ast::Name>) -> Schema {
    match s {
        SchemaExpr::Map(inner) => Schema::Map {
            value: Box::new(lower_in_prog(inner, program, building)),
        },
        SchemaExpr::List(inner) => Schema::List {
            element: Box::new(lower_in_prog(inner, program, building)),
        },
        SchemaExpr::Record(fields) => Schema::Tree {
            branches: fields
                .iter()
                .map(|(k, v)| (Key::from(k.as_str()), lower_in_prog(v, program, building)))
                .collect(),
        },
        SchemaExpr::Array { shape, element } => Schema::Array {
            shape: shape.clone(),
            element: Box::new(lower_in_prog(element, program, building)),
        },
        SchemaExpr::Custom { name, .. } => match program.lookup(name) {
            // A composite as a map ELEMENT (e.g. `cells: map[Cell]`) → its
            // [`Schema::CompositeLink`] (schema-first). A cell VALUE is an
            // addressed node (`{address, config, inputs, outputs}` from
            // `build_composite_outer`), so it is discovered AND divided by schema —
            // no address-scan. (Retires the `composite_instance_schema` dodge: the
            // homoiconic `{_type, mass, body}` container it guarded against no
            // longer exists — composites compile to addressed process nodes, and a
            // protocol-bound cell carries a typed `{_type: stream, …}` address.)
            Some(Def::Composite(c)) => composite_link(c, program, building),
            // A protocol-bound control (`map[StreamingCell]` where
            // `protocol StreamingCell = stream<Cell, …>`) → the WRAPPED composite's
            // CompositeLink: the interface/divide semantics are the wrapped
            // composite's; only the address (on the value) differs.
            Some(Def::Protocol(pd)) => match program.lookup(&pd.wrapped) {
                Some(Def::Composite(c)) => composite_link(c, program, building),
                _ => lower_schema(s),
            },
            Some(d @ (Def::Process(_) | Def::Step(_))) => def_schema_in(d, program, building),
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
        SchemaExpr::Record(fields) => Schema::Tree {
            branches: fields
                .iter()
                .map(|(k, v)| (Key::from(k.as_str()), lower_schema(v)))
                .collect(),
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
        SchemaExpr::Overwrite(inner) => Schema::Overwrite {
            inner: Box::new(lower_schema(inner)),
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

/// A parameter-free nominal axis value. `resolve` keeps a same-named
/// `Custom` and prefers the update for a mismatched one, so the
/// target/method/advance axes get nominal equality for free.
fn nominal_axis(name: &str) -> Schema {
    Schema::Custom {
        name: name.to_string(),
        parameters: IndexMap::new(),
    }
}

/// Lower a [`ContractRef`] to its schema — a `Tree` of nominal axes (the
/// named contract's base axes, with the ref's pins layered on). Process-
/// contract substitutability is `prism_schema::algebra::refines` over these
/// trees: a fulfiller's contract must refine the demanded one. No new algebra
/// op — see docs/process-contracts.md. `program` supplies the named contract's
/// base axes.
pub fn contract_ref_schema(r: &ContractRef, program: &Program) -> Schema {
    let mut branches: IndexMap<Key, Schema> = IndexMap::new();
    if let Some(Def::Contract(base)) = program.lookup(&r.name) {
        for (axis, value) in &base.axes {
            branches.insert(Key::from(axis.as_str()), nominal_axis(value));
        }
    }
    for (axis, value) in &r.pins {
        branches.insert(Key::from(axis.as_str()), nominal_axis(value));
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
    def_schema_in(def, program, &mut Vec::new())
}

fn def_schema_in(def: &Def, program: &Program, building: &mut Vec<crate::ast::Name>) -> Schema {
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
        Def::Composite(c) => composite_link(c, program, building),
        _ => Schema::Any,
    }
}

/// A composite's `CompositeLink` (interface + inner-state schema), with CYCLE
/// DETECTION: if `c` is already being built (a back-edge to an ancestor
/// composite — e.g. `Environment[cells: map[Cell]]` ↔ `Cell`'s `environment`),
/// emit the interface with an EMPTY inner instead of recursing (which would
/// loop forever). Forward edges build the real inner. (#9 — the dynamic-
/// structure schema that lets a cell in a Map be discovered schema-first with a
/// working subengine body.)
fn composite_link(
    c: &CompositeDef,
    program: &Program,
    building: &mut Vec<crate::ast::Name>,
) -> Schema {
    let inner = if building.contains(&c.name) {
        Schema::Tree {
            branches: IndexMap::new(),
        }
    } else {
        building.push(c.name.clone());
        let inner = composite_inner_in(c, program, building);
        building.pop();
        inner
    };
    Schema::CompositeLink {
        inputs: port_schemas(&c.interface.inputs),
        outputs: port_schemas(&c.interface.outputs),
        interval: 1.0,
        inner_schema: Box::new(inner),
    }
}

/// A composite's INNER state schema: a `Tree` whose branches are the
/// body's slots. Data slots are typed from the composite's params /
/// output ports; sub-process / sub-composite slots get their link
/// schema.
pub fn composite_inner_schema(def: &CompositeDef, program: &Program) -> Schema {
    // Seed the `building` stack with this composite so a self/back reference in
    // the body doesn't loop (see [`composite_link`]).
    let mut building = vec![def.name.clone()];
    composite_inner_in(def, program, &mut building)
}

fn composite_inner_in(
    def: &CompositeDef,
    program: &Program,
    building: &mut Vec<crate::ast::Name>,
) -> Schema {
    // Slot types we can resolve by name: config params + declared ports.
    // Program-aware so a `custom(CompositeName)` param (e.g. `cells: map[Cell]`)
    // expands to the composite's CompositeLink (cycle-broken) — see
    // [`lower_in_prog`] / [`composite_link`].
    let mut known: IndexMap<String, Schema> = IndexMap::new();
    for p in &def.params {
        known.insert(p.name.clone(), lower_in_prog(&p.schema, program, building));
    }
    for (n, d) in def
        .interface
        .inputs
        .iter()
        .chain(def.interface.outputs.iter())
    {
        if !known.contains_key(n) {
            let ty = lower_in_prog(&d.schema, program, building);
            known.insert(n.clone(), ty);
        }
    }

    let mut branches: IndexMap<Key, Schema> = IndexMap::new();
    collect_branches(&def.body, program, &known, building, &mut branches);
    // The interface PORTS are inner-state slots too — the body's wires read/write
    // them — typed by their declarations, at their bridge paths. The body's keyed
    // nodes alone MISS them (a port isn't a keyed entry), which left a composite
    // run via Simulate carrying `Any` at its port paths (`Composite.outputs()` →
    // Any → no schema-driven plot). Fill them (without clobbering a body branch) so
    // the composite's output schema is honest.
    for (n, d) in def
        .interface
        .inputs
        .iter()
        .chain(def.interface.outputs.iter())
    {
        let ty = lower_in_prog(&d.schema, program, building);
        let path = d.bridge.clone().unwrap_or_else(|| vec![n.clone()]);
        insert_at_path(&mut branches, &path, ty);
    }
    Schema::Tree { branches }
}

/// Overlay the DECLARED port type `schema` at `path`, building nested `Tree`s.
/// The declared type is **authoritative**: it is `resolve`d over any body-derived
/// branch (so e.g. a `cells :: map[Cell]` port refines a body `Tree{c0:
/// CompositeLink}` to `Map{CompositeLink}` — the Link-value Map wins). Uses the
/// closed algebra's `resolve`, not a hand-rolled merge.
fn insert_at_path(branches: &mut IndexMap<Key, Schema>, path: &[String], schema: Schema) {
    let Some((head, rest)) = path.split_first() else {
        return;
    };
    let key = Key::from(head.as_str());
    if rest.is_empty() {
        let resolved = match branches.get(&key) {
            Some(existing) => prism_schema::algebra::resolve(existing, &schema),
            None => schema,
        };
        branches.insert(key, resolved);
    } else {
        let child = branches.entry(key).or_insert_with(|| Schema::Tree {
            branches: IndexMap::new(),
        });
        if let Schema::Tree { branches: inner } = child {
            insert_at_path(inner, rest, schema);
        }
    }
}

/// Collect the body entries whose schema `algebra::infer` can't recover — only
/// those (each a `Some` from [`branch_schema`]) are inserted; pure-data entries
/// (a `None`) are left for `infer` to type from the evaluated value.
fn collect_branches(
    e: &Expr,
    program: &Program,
    known: &IndexMap<String, Schema>,
    building: &mut Vec<crate::ast::Name>,
    out: &mut IndexMap<Key, Schema>,
) {
    match e {
        Expr::Parallel(items) => {
            for i in items {
                collect_branches(i, program, known, building, out);
            }
        }
        Expr::KeyedEntry { key, value } => {
            if let Some(k) = key.as_plain() {
                if let Some(s) = branch_schema(value, program, known, building) {
                    out.insert(Key::from(k.as_str()), s);
                }
            }
        }
        _ => {}
    }
}

/// The DECLARED schema for a body entry that `algebra::infer` **cannot recover**
/// from the evaluated value — and ONLY that. This is NOT a parallel inference: it
/// returns `Some` for a process/composite/step **Link** (infer sees only the
/// `{address, config, …}` map), a param/port's **declared** type (infer can't
/// recover `Delta`/`Array`/`Custom` from a runtime float/list), or a collection
/// CONTAINING those; and `None` for pure data (literals, `map[float]`, records),
/// which the algebra infers from the value at the engine's `resolve(infer(state),
/// declared)`. (Retires the old hand-rolled data arms — task #24.)
fn branch_schema(
    e: &Expr,
    program: &Program,
    known: &IndexMap<String, Schema>,
    building: &mut Vec<crate::ast::Name>,
) -> Option<Schema> {
    match e {
        // A sub-term whose control names a definer → its Link schema (infer sees
        // only `{address, config, …}`, never the Link).
        Expr::Term { control, ports, .. } => match program.lookup(control) {
            Some(d @ (Def::Process(_) | Def::Step(_) | Def::Composite(_))) => {
                Some(def_schema_in(d, program, building))
            }
            // A protocol-bound control (`StreamingCell[…]`) → the WRAPPED
            // composite's CompositeLink (the address is on the value; the schema
            // is the wrapped interface).
            Some(Def::Protocol(pd)) => match program.lookup(&pd.wrapped) {
                Some(d @ Def::Composite(_)) => Some(def_schema_in(d, program, building)),
                _ => None,
            },
            // A native process control (no user def) with a `~{}->{}` interface →
            // a kind-agnostic base `Link` MARKER, so the engine sees a process
            // node (schema-first discovery) rather than an opaque `Any`. Port
            // types are best-effort from the wired slots (`known`); the engine
            // reconciles the precise `ProcessLink`/`StepLink` from the instance.
            _ if !ports.outputs.is_empty() || !ports.inputs.is_empty() => {
                let keyed = |binds: &IndexMap<crate::ast::Name, Expr>| -> IndexMap<Key, Schema> {
                    binds
                        .iter()
                        .map(|(n, w)| {
                            let ty = match w {
                                Expr::Var(v) => known.get(v).cloned().unwrap_or(Schema::Any),
                                _ => Schema::Any,
                            };
                            (Key::from(n.as_str()), ty)
                        })
                        .collect()
                };
                Some(Schema::Link {
                    inputs: keyed(&ports.inputs),
                    outputs: keyed(&ports.outputs),
                    temporal: None,
                })
            }
            // Genuine data (a plain ion / value node) → `infer` types it.
            _ => None,
        },
        // A bare reference (`mass: mass0`) → the param/port's DECLARED type — infer
        // can't recover `Delta`/`Array`/`Custom`/`Link` from a runtime value.
        Expr::Var(n) => known.get(n).cloned(),
        // A collection LITERAL: recurse for any DECLARED element/fields (e.g. a
        // `cells` map of `Cell`s → `Map{CompositeLink}`). Pure-data collections
        // (`map[float]`, plain records) → `None`: `infer` recovers them faithfully.
        Expr::Map(entries) => entries
            .first()
            .and_then(|(_, v)| branch_schema(v, program, known, building))
            .map(|elem| Schema::Map {
                value: Box::new(elem),
            }),
        Expr::Record(fields) => {
            let mut branches: IndexMap<Key, Schema> = IndexMap::new();
            for (k, v) in fields {
                if let Some(s) = branch_schema(v, program, known, building) {
                    branches.insert(Key::from(k.as_str()), s);
                }
            }
            (!branches.is_empty()).then_some(Schema::Tree { branches })
        }
        Expr::List(items) => items
            .first()
            .and_then(|v| branch_schema(v, program, known, building))
            .map(|elem| Schema::List {
                element: Box::new(elem),
            }),
        // Pure data literals → `None`: `algebra::infer` types them from the value.
        _ => None,
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
            Schema::ProcessLink {
                inputs, outputs, ..
            } => {
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
        // `cells: map[Cell]` → `Map{CompositeLink}` (schema-first), NOT the old
        // `composite_instance_schema` Tree dodge and NOT an opaque `Custom("Cell")`.
        // A cell VALUE is an addressed node, so it's discovered + divided by schema;
        // the CompositeLink carries the Cell's interface (`outputs` = the matchable/
        // divisible face) + its `inner_schema`.
        match branches.get(&k("cells")) {
            Some(Schema::Map { value }) => {
                assert!(
                    matches!(value.as_ref(), Schema::CompositeLink { .. }),
                    "cells element should be a CompositeLink (schema-first cells); got {value:?}"
                );
            }
            other => panic!("cells should be a Map, got {other:?}"),
        }
    }
}

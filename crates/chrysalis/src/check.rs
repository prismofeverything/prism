//! Compile-time connection validation — the safe guardrail before the
//! state/schema unification (see docs/state-schema-unification.md).
//!
//! For every port wiring `~{port: target}` / `->{port: target}` on a
//! child term, the declared port schema and the target slot's schema
//! must be compatible — both **structurally** (a scalar can't wire to a
//! map) and **dimensionally** (a `[mass]` port can't wire to a `[time]`
//! slot). Illegal connections become visible errors instead of silent
//! runtime nonsense.
//!
//! This works at the surface `SchemaExpr` level (which still carries unit
//! information, unlike the erased runtime `Schema`), reusing
//! [`crate::units`] for the dimensional check.

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::ast::{Def, Expr, Interface, Name, PortDecl, Program, SchemaExpr};
use crate::units::UnitEnv;

/// A wiring whose endpoints don't agree.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionError {
    pub composite: String,
    pub child: String,
    pub port: String,
    pub message: String,
}

/// Validate every port wiring in the program. Empty result = valid.
pub fn validate_connections(program: &Program) -> Vec<ConnectionError> {
    let env = UnitEnv::from_program(program).ok();
    let mut errors = Vec::new();
    for def in &program.defs {
        if let Def::Composite(c) = def {
            // Slot schemas resolvable by name: config params + ports.
            let mut slots: HashMap<String, SchemaExpr> = HashMap::new();
            for p in &c.params {
                slots.insert(p.name.clone(), p.schema.clone());
            }
            for (n, d) in c.interface.inputs.iter().chain(c.interface.outputs.iter()) {
                slots.entry(n.clone()).or_insert_with(|| d.schema.clone());
            }
            check_body(&c.body, &c.name, program, env.as_ref(), &slots, &mut errors);
        }
    }
    errors
}

fn interface_of(def: &Def) -> Option<&Interface> {
    match def {
        Def::Process(p) => Some(&p.interface),
        Def::Step(s) => Some(&s.interface),
        Def::Composite(c) => Some(&c.interface),
        Def::Extern(e) => Some(&e.interface),
        _ => None,
    }
}

fn check_body(
    e: &Expr,
    composite: &str,
    program: &Program,
    env: Option<&UnitEnv>,
    slots: &HashMap<String, SchemaExpr>,
    errors: &mut Vec<ConnectionError>,
) {
    match e {
        Expr::Parallel(items) => {
            for i in items {
                check_body(i, composite, program, env, slots, errors);
            }
        }
        Expr::KeyedEntry { value, .. } => check_body(value, composite, program, env, slots, errors),
        Expr::Block(b) => {
            for (_, v) in &b.bindings {
                check_body(v, composite, program, env, slots, errors);
            }
            check_body(&b.value, composite, program, env, slots, errors);
        }
        Expr::Term { control, ports, .. } => {
            if let Some(iface) = program.lookup(control).and_then(interface_of) {
                for (port, target) in &ports.inputs {
                    check_wire(
                        composite, control, port, &iface.inputs, target, slots, env, errors,
                    );
                }
                for (port, target) in &ports.outputs {
                    check_wire(
                        composite, control, port, &iface.outputs, target, slots, env, errors,
                    );
                }
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn check_wire(
    composite: &str,
    child: &str,
    port: &str,
    iface_ports: &IndexMap<Name, PortDecl>,
    target: &Expr,
    slots: &HashMap<String, SchemaExpr>,
    env: Option<&UnitEnv>,
    errors: &mut Vec<ConnectionError>,
) {
    // The child's declared schema for this port.
    let port_se = match iface_ports.get(port) {
        Some(d) => &d.schema,
        None => return, // undeclared port — out of scope here
    };
    // Resolve the target to a local slot schema (the common case:
    // `~{mass: mass}`). Non-local paths (`cytoplasm.tf`) are left to the
    // unification step.
    let slot_se = match target {
        Expr::Var(n) => match slots.get(n) {
            Some(s) => s,
            None => return,
        },
        _ => return,
    };

    if !kinds_compatible(port_se, slot_se) {
        errors.push(ConnectionError {
            composite: composite.to_string(),
            child: child.to_string(),
            port: port.to_string(),
            message: format!(
                "structural mismatch: port is {} but slot is {}",
                kind_family(port_se),
                kind_family(slot_se)
            ),
        });
        return;
    }

    if let Some(env) = env {
        if is_dimensioned(port_se) && is_dimensioned(slot_se) {
            let pd = env
                .unit_of_schema(port_se)
                .map(|u| u.dimension)
                .unwrap_or_default();
            let sd = env
                .unit_of_schema(slot_se)
                .map(|u| u.dimension)
                .unwrap_or_default();
            if pd != sd {
                errors.push(ConnectionError {
                    composite: composite.to_string(),
                    child: child.to_string(),
                    port: port.to_string(),
                    message: format!("dimension mismatch: port {pd:?} vs slot {sd:?}"),
                });
            }
        }
    }
}

fn kind_family(s: &SchemaExpr) -> &'static str {
    match s {
        SchemaExpr::Float | SchemaExpr::Int | SchemaExpr::Quantity { .. } => "number",
        SchemaExpr::Bool => "bool",
        SchemaExpr::String => "string",
        SchemaExpr::Map(_) => "map",
        SchemaExpr::List(_) => "list",
        SchemaExpr::Array { .. } => "array",
        SchemaExpr::Custom { .. } => "custom",
        SchemaExpr::Any | SchemaExpr::SelfType => "any",
    }
}

fn kinds_compatible(a: &SchemaExpr, b: &SchemaExpr) -> bool {
    let (fa, fb) = (kind_family(a), kind_family(b));
    // `any`/`custom` are lenient (resolved against the TypeRegistry in
    // the unification step); otherwise families must match.
    fa == "any" || fb == "any" || fa == "custom" || fb == "custom" || fa == fb
}

fn is_dimensioned(s: &SchemaExpr) -> bool {
    match s {
        SchemaExpr::Quantity { .. } => true,
        SchemaExpr::Array { element, .. } => is_dimensioned(element),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{CompositeDef, Param, ProcessDef, UnitExpr};
    use crate::fixtures::{grow_divide, nuclear_shuttle};

    #[test]
    fn good_programs_validate_clean() {
        assert!(validate_connections(&grow_divide::program()).is_empty());
        assert!(
            validate_connections(&nuclear_shuttle::program()).is_empty(),
            "{:?}",
            validate_connections(&nuclear_shuttle::program())
        );
    }

    #[test]
    fn dimension_mismatch_on_a_wire_is_caught() {
        // process Sink ~{m: Quantity[kg]} ->{}
        // composite Bad[t: Quantity[s]] ( t: t | Sink ~{m: t} )
        // Wiring a [mass] port to a [time] slot must error.
        let mass = || SchemaExpr::quantity(UnitExpr::named("kg"), false, false);
        let time = || SchemaExpr::quantity(UnitExpr::named("s"), false, false);

        let mut p = Program::new();
        p.push(Def::Process(ProcessDef {
            name: "Sink".into(),
            params: vec![],
            interface: Interface::new().with_input("m", PortDecl::required(mass())),
            body: Expr::Record(IndexMap::new()),
        }));
        p.push(Def::Composite(CompositeDef {
            name: "Bad".into(),
            params: vec![Param::with_default("t", time(), Expr::float(1.0))],
            interface: Interface::new(),
            using: vec![],
            body: Expr::parallel(vec![
                Expr::entry("t", Expr::var("t")),
                Expr::entry("sink", Expr::term("Sink").input("m", Expr::var("t")).build()),
            ]),
        }));

        let errs = validate_connections(&p);
        assert!(
            errs.iter().any(|e| e.message.contains("dimension")),
            "expected a dimension mismatch, got {errs:?}"
        );
    }

    #[test]
    fn structural_mismatch_is_caught() {
        // Wiring a scalar port to a Map slot.
        let mut p = Program::new();
        p.push(Def::Process(ProcessDef {
            name: "Sink".into(),
            params: vec![],
            interface: Interface::new().with_input("x", PortDecl::required(SchemaExpr::Float)),
            body: Expr::Record(IndexMap::new()),
        }));
        p.push(Def::Composite(CompositeDef {
            name: "Bad".into(),
            params: vec![Param::required("bag", SchemaExpr::map_of(SchemaExpr::Float))],
            interface: Interface::new(),
            using: vec![],
            body: Expr::parallel(vec![
                Expr::entry("bag", Expr::var("bag")),
                Expr::entry("sink", Expr::term("Sink").input("x", Expr::var("bag")).build()),
            ]),
        }));

        let errs = validate_connections(&p);
        assert!(
            errs.iter().any(|e| e.message.contains("structural")),
            "expected a structural mismatch, got {errs:?}"
        );
    }
}

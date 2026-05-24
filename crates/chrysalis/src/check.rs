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

use crate::ast::{ContractRef, Def, Expr, Interface, Name, PathRoot, PortDecl, Program, SchemaExpr};
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
            // slot name → control, so a producer path `r.trajectory` resolves
            // to the contract on sub-process `r`'s output port.
            let mut bindings: HashMap<String, String> = HashMap::new();
            collect_bindings(&c.body, &mut bindings);
            // slot name → contract, for a producer output wired to a slot
            // (`Rk4 ->{trajectory: rk4_traj}` makes `rk4_traj` carry the
            // trajectory port's contract), so a consumer reading the slot is
            // checked against it.
            let mut slot_contracts: HashMap<String, ContractRef> = HashMap::new();
            collect_slot_contracts(&c.body, program, &mut slot_contracts);
            check_body(
                &c.body, &c.name, program, env.as_ref(), &slots, &bindings, &slot_contracts,
                &mut errors,
            );
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

#[allow(clippy::too_many_arguments)]
fn check_body(
    e: &Expr,
    composite: &str,
    program: &Program,
    env: Option<&UnitEnv>,
    slots: &HashMap<String, SchemaExpr>,
    bindings: &HashMap<String, String>,
    slot_contracts: &HashMap<String, ContractRef>,
    errors: &mut Vec<ConnectionError>,
) {
    match e {
        Expr::Parallel(items) => {
            for i in items {
                check_body(i, composite, program, env, slots, bindings, slot_contracts, errors);
            }
        }
        Expr::KeyedEntry { value, .. } => {
            check_body(value, composite, program, env, slots, bindings, slot_contracts, errors)
        }
        Expr::Block(b) => {
            for (_, v) in &b.bindings {
                check_body(v, composite, program, env, slots, bindings, slot_contracts, errors);
            }
            check_body(&b.value, composite, program, env, slots, bindings, slot_contracts, errors);
        }
        Expr::Term { control, ports, .. } => {
            if let Some(iface) = program.lookup(control).and_then(interface_of) {
                for (port, target) in &ports.inputs {
                    check_wire(
                        composite, control, port, &iface.inputs, target, slots, bindings,
                        slot_contracts, program, env, true, errors,
                    );
                }
                for (port, target) in &ports.outputs {
                    check_wire(
                        composite, control, port, &iface.outputs, target, slots, bindings,
                        slot_contracts, program, env, false, errors,
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
    bindings: &HashMap<String, String>,
    slot_contracts: &HashMap<String, ContractRef>,
    program: &Program,
    env: Option<&UnitEnv>,
    is_input: bool,
    errors: &mut Vec<ConnectionError>,
) {
    // The child's declared port (schema + optional contract).
    let port_decl = match iface_ports.get(port) {
        Some(d) => d,
        None => return, // undeclared port — out of scope here
    };
    let port_se = &port_decl.schema;

    // ── Contract substitutability (input wires) ──
    // An input port may DEMAND a contract (`a :: DeterministicMassAction`); the
    // wired source — typically a producer sub-process output, `r.trajectory` —
    // may CARRY one. The wire is legal only if the source's contract REFINES the
    // demanded one: `resolve(provided, demanded) == provided`, the existing
    // schema join (docs/process-contracts.md). No new algebra op; the negative
    // case is exactly what fails to compile.
    if is_input {
        if let Some(demanded) = port_decl.contract.as_ref() {
            check_contract(
                composite, child, port, demanded, target, bindings, slot_contracts, program, errors,
            );
        }
    }

    // Resolve the target to a local slot schema (the common case:
    // `~{mass: mass}`). Non-local paths (`cytoplasm.tf`, `r.trajectory`) are
    // left to the unification step / handled by the contract check above.
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

/// Enforce that the source wired into a contract-demanding input port carries
/// a contract that **refines** the demanded one (`resolve(provided, demanded)
/// == provided`). The teeth of the process-contract layer — see
/// docs/process-contracts.md.
#[allow(clippy::too_many_arguments)]
fn check_contract(
    composite: &str,
    child: &str,
    port: &str,
    demanded: &ContractRef,
    target: &Expr,
    bindings: &HashMap<String, String>,
    slot_contracts: &HashMap<String, ContractRef>,
    program: &Program,
    errors: &mut Vec<ConnectionError>,
) {
    let push = |errors: &mut Vec<ConnectionError>, message: String| {
        errors.push(ConnectionError {
            composite: composite.to_string(),
            child: child.to_string(),
            port: port.to_string(),
            message,
        });
    };
    match provider_contract(target, bindings, slot_contracts, program) {
        Some(provided) => {
            let dem = crate::schema::contract_ref_schema(demanded, program);
            let prov = crate::schema::contract_ref_schema(&provided, program);
            if !prism_schema::algebra::refines(&prov, &dem) {
                push(
                    errors,
                    format!(
                        "contract mismatch: source fulfills `{}` but port demands `{}` \
                         (its target semantics do not refine the demanded contract)",
                        provided.name, demanded.name
                    ),
                );
            }
        }
        None => push(
            errors,
            format!("port demands contract `{}` but the wired source declares none", demanded.name),
        ),
    }
}

/// The contract carried by a wire's source, if any. Primary case: a producer
/// path `r.trajectory` → the contract on sub-process `r`'s output port.
fn provider_contract(
    target: &Expr,
    bindings: &HashMap<String, String>,
    slot_contracts: &HashMap<String, ContractRef>,
    program: &Program,
) -> Option<ContractRef> {
    match target {
        // `r.trajectory` — a producer sub-process's output port.
        Expr::Path(p) => {
            if let (PathRoot::Local(binding), [seg]) = (&p.root, p.segments.as_slice()) {
                let control = bindings.get(binding.as_str())?;
                let iface = interface_of(program.lookup(control)?)?;
                return iface.outputs.get(seg.as_str())?.contract.clone();
            }
            None
        }
        // A slot a producer's contracted output was wired into
        // (`Rk4 ->{trajectory: rk4_traj}`).
        Expr::Var(slot) => slot_contracts.get(slot.as_str()).cloned(),
        _ => None,
    }
}

/// Collect `slot name → control` for every sub-term bound in a composite body
/// (`r = Rk4 …` / `r: Rk4 …`), so a producer path `r.trajectory` resolves to
/// sub-process `r`'s definer.
fn collect_bindings(e: &Expr, out: &mut HashMap<String, String>) {
    match e {
        Expr::Parallel(items) => {
            for i in items {
                collect_bindings(i, out);
            }
        }
        Expr::KeyedEntry { key, value } => {
            if let (Some(k), Some(ctrl)) = (key.as_plain(), term_control(value)) {
                out.insert(k.to_string(), ctrl);
            }
            collect_bindings(value, out);
        }
        Expr::Block(b) => {
            for (name, v) in &b.bindings {
                if let Some(ctrl) = term_control(v) {
                    out.insert(name.clone(), ctrl);
                }
                collect_bindings(v, out);
            }
            collect_bindings(&b.value, out);
        }
        _ => {}
    }
}

/// Collect `slot name → contract` for outputs wired to a slot: a term
/// `Ctrl ->{port: slot}` whose output `port` carries a contract makes `slot`
/// carry it, so a later consumer `~{a: slot}` can be checked against it. This is
/// how a process-contract flows through the place graph (not only via a direct
/// `r.trajectory` path).
fn collect_slot_contracts(e: &Expr, program: &Program, out: &mut HashMap<String, ContractRef>) {
    match e {
        Expr::Parallel(items) => {
            for i in items {
                collect_slot_contracts(i, program, out);
            }
        }
        Expr::KeyedEntry { value, .. } => collect_slot_contracts(value, program, out),
        Expr::Block(b) => {
            for (_, v) in &b.bindings {
                collect_slot_contracts(v, program, out);
            }
            collect_slot_contracts(&b.value, program, out);
        }
        Expr::Term { control, ports, .. } => {
            if let Some(iface) = program.lookup(control).and_then(interface_of) {
                for (port, target) in &ports.outputs {
                    if let (Some(decl), Expr::Var(slot)) = (iface.outputs.get(port), target) {
                        if let Some(c) = &decl.contract {
                            out.insert(slot.clone(), c.clone());
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// The definer a binding's value refers to: a term's control (`Rk4 ~{…}`) or a
/// bare capitalized reference (`Rk4`, which parses as a `Var`). Over-approximate
/// — a name that isn't a definer resolves to `None` via `program.lookup` later.
fn term_control(e: &Expr) -> Option<String> {
    match e {
        Expr::Term { control, .. } => Some(control.clone()),
        Expr::Var(name) => Some(name.clone()),
        _ => None,
    }
}

fn kind_family(s: &SchemaExpr) -> &'static str {
    match s {
        SchemaExpr::Float | SchemaExpr::Int | SchemaExpr::Quantity { .. } => "number",
        SchemaExpr::Bool => "bool",
        SchemaExpr::String => "string",
        SchemaExpr::Map(_) => "map",
        SchemaExpr::List(_) => "list",
        SchemaExpr::Record(_) => "record",
        SchemaExpr::Array { .. } => "array",
        SchemaExpr::Custom { .. } => "custom",
        // `overwrite[T]` is a delta-semantics wrapper — its kind is its inner's.
        SchemaExpr::Overwrite(inner) => kind_family(inner),
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

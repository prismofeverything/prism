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

use crate::ast::{
    ContractRef, Def, Expr, Interface, Name, PathRoot, PortDecl, Program, SchemaExpr, TermArg,
};

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
                &c.body,
                &c.name,
                program,
                &slots,
                &bindings,
                &slot_contracts,
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
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn check_body(
    e: &Expr,
    composite: &str,
    program: &Program,
    slots: &HashMap<String, SchemaExpr>,
    bindings: &HashMap<String, String>,
    slot_contracts: &HashMap<String, ContractRef>,
    errors: &mut Vec<ConnectionError>,
) {
    match e {
        Expr::Parallel(items) => {
            for i in items {
                check_body(
                    i,
                    composite,
                    program,
                    slots,
                    bindings,
                    slot_contracts,
                    errors,
                );
            }
        }
        Expr::KeyedEntry { value, .. } => check_body(
            value,
            composite,
            program,
            slots,
            bindings,
            slot_contracts,
            errors,
        ),
        Expr::Block(b) => {
            for (_, v) in &b.bindings {
                check_body(
                    v,
                    composite,
                    program,
                    slots,
                    bindings,
                    slot_contracts,
                    errors,
                );
            }
            check_body(
                &b.value,
                composite,
                program,
                slots,
                bindings,
                slot_contracts,
                errors,
            );
        }
        Expr::Term { control, ports, .. } => {
            if let Some(iface) = program.lookup(control).and_then(interface_of) {
                for (port, target) in &ports.inputs {
                    check_wire(
                        composite,
                        control,
                        port,
                        &iface.inputs,
                        target,
                        slots,
                        bindings,
                        slot_contracts,
                        program,
                        true,
                        errors,
                    );
                }
                for (port, target) in &ports.outputs {
                    check_wire(
                        composite,
                        control,
                        port,
                        &iface.outputs,
                        target,
                        slots,
                        bindings,
                        slot_contracts,
                        program,
                        false,
                        errors,
                    );
                }
            }
        }
        // A `mesh` link (#62) is REPLICATED across peers; it converges with no
        // coordinator iff its merge is a CRDT (a join-semilattice). Gate it through
        // `algebra::mesh_safety` HERE, at compile, so an unsafe replicated link
        // (additive scalar, last-writer-wins, sequence) is a compile error — illegal
        // distributed states are unrepresentable, not a runtime divergence.
        Expr::LinkDecl { name, schema, mesh: true, .. } => match schema {
            Some(s) => {
                let lowered = crate::schema::lower_schema_in_program(s, program);
                if let Err(unsafe_) = prism_schema::algebra::mesh_safety(&lowered) {
                    errors.push(ConnectionError {
                        composite: composite.to_string(),
                        child: name.clone(),
                        port: "mesh".into(),
                        message: format!(
                            "mesh link `{name}` is not CRDT-safe ({}). A replicated \
                             link must converge with no coordinator — its merge must be \
                             a join-semilattice.",
                            unsafe_.reason
                        ),
                    });
                }
            }
            None => errors.push(ConnectionError {
                composite: composite.to_string(),
                child: name.clone(),
                port: "mesh".into(),
                message: format!(
                    "mesh link `{name}` must declare its value-schema \
                     (`link {name} :: T mesh = …`) so its CRDT-safety can be checked."
                ),
            }),
        },
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
                composite,
                child,
                port,
                demanded,
                target,
                bindings,
                slot_contracts,
                program,
                errors,
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

    // Dimensional compatibility (#71 units-in-schema): a `[mass]` port wired to a
    // `[length]` slot is an illegal link. Now that the schema CARRIES the dimension
    // (lowered from the port/slot's unit), this is the FIRST-CLASS schema-algebra
    // check `dimension_conflict` on the lowered schemas — the schema is the one
    // dimension source, replacing the bespoke parallel-unit-AST comparison.
    let port_schema = crate::schema::lower_schema_in_program(port_se, program);
    let slot_schema = crate::schema::lower_schema_in_program(slot_se, program);
    if let Some((pd, sd)) = prism_schema::algebra::dimension_conflict(&port_schema, &slot_schema) {
        errors.push(ConnectionError {
            composite: composite.to_string(),
            child: child.to_string(),
            port: port.to_string(),
            message: format!("dimension mismatch: port [{pd:?}] cannot wire to slot [{sd:?}]"),
        });
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
            format!(
                "port demands contract `{}` but the wired source declares none",
                demanded.name
            ),
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

/// Native wrappers that FORWARD a contract from a config-argument process's
/// output onto their own output(s). `RunProcess` (`from core import RunProcess`,
/// registered in `prelude.rs`) runs its `proc` argument over time, so its
/// `timeseries` / `trace` outputs carry whatever contract `proc`'s `state`
/// output declares — *a contract must survive a generic wrapper*
/// (docs/process-contracts.md). Native processes have NO chrysalis `Interface`
/// (they live in `ResolvedImports.processes`, a bare name set — see compile.rs),
/// which is exactly why the carried contract is otherwise lost at the wrapper.
/// Declared here as DATA and interpreted generically by [`forwarded_contract`] —
/// not a control-name special-case baked into the checking algorithm.
struct ContractForward {
    /// The wrapper control that forwards a contract (`RunProcess`).
    control: &'static str,
    /// The wrapper's config argument naming the inner process (`proc`).
    from_arg: &'static str,
    /// The inner process's output port whose contract is forwarded (`state`).
    from_output: &'static str,
    /// The wrapper outputs that carry the forwarded contract.
    to_outputs: &'static [&'static str],
}

const CONTRACT_FORWARDS: &[ContractForward] = &[ContractForward {
    control: "RunProcess",
    from_arg: "proc",
    from_output: "state",
    to_outputs: &["timeseries", "trace"],
}];

/// If `control` is a forwarding wrapper (see [`CONTRACT_FORWARDS`]), resolve the
/// contract it forwards: find its `from_arg` argument — the inner process term
/// (`proc: Rk4[…]`) — look up that process's definer, and read the contract on
/// its `from_output` port. Returns the contract and the wrapper outputs that
/// carry it. This reuses the inner process's REAL declared contract, so a
/// wrong-target inner is rejected exactly as a directly-wired one would be — no
/// new algebra op, just `refines` over the forwarded contract downstream.
fn forwarded_contract(
    control: &str,
    args: &[TermArg],
    program: &Program,
) -> Option<(ContractRef, &'static [&'static str])> {
    let rule = CONTRACT_FORWARDS.iter().find(|f| f.control == control)?;
    let inner = args.iter().find_map(|a| match a {
        TermArg::Named { name, value } if name.as_str() == rule.from_arg => Some(value),
        _ => None,
    })?;
    let inner_control = term_control(inner)?;
    // A protocol alias (a rest-addressed `CopasiCvode`) forwards its WRAPPED
    // process's contract — resolve through it, so a remote engine's trajectory
    // carries DeterministicMassAction exactly as a native's does.
    let def = match program.lookup(&inner_control)? {
        crate::ast::Def::Protocol(pd) => program.lookup(&pd.wrapped)?,
        other => other,
    };
    let iface = interface_of(def)?;
    let contract = iface.outputs.get(rule.from_output)?.contract.clone()?;
    Some((contract, rule.to_outputs))
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
        Expr::Term { control, args, ports, .. } => {
            // A producer with a DECLARED interface contract on an output → slot.
            if let Some(iface) = program.lookup(control).and_then(interface_of) {
                for (port, target) in &ports.outputs {
                    if let (Some(decl), Expr::Var(slot)) = (iface.outputs.get(port), target) {
                        if let Some(c) = &decl.contract {
                            out.insert(slot.clone(), c.clone());
                        }
                    }
                }
            }
            // A native forwarding wrapper (`RunProcess`): the contract lives on
            // the inner `proc:` process, not the wrapper's (absent) interface —
            // forward it onto the configured outputs so a downstream consumer
            // (`Compare ~{a: slot :: C}`) is still checked. Slot wiring is the
            // shape the flagship uses (`->{timeseries: rk4_traj}`).
            if let Some((contract, fwd_outputs)) = forwarded_contract(control, args, program) {
                for (port, target) in &ports.outputs {
                    if let Expr::Var(slot) = target {
                        if fwd_outputs.contains(&port.as_str()) {
                            out.insert(slot.clone(), contract.clone());
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
                Expr::entry(
                    "sink",
                    Expr::term("Sink").input("m", Expr::var("t")).build(),
                ),
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
            params: vec![Param::required(
                "bag",
                SchemaExpr::map_of(SchemaExpr::Float),
            )],
            interface: Interface::new(),
            using: vec![],
            body: Expr::parallel(vec![
                Expr::entry("bag", Expr::var("bag")),
                Expr::entry(
                    "sink",
                    Expr::term("Sink").input("x", Expr::var("bag")).build(),
                ),
            ]),
        }));

        let errs = validate_connections(&p);
        assert!(
            errs.iter().any(|e| e.message.contains("structural")),
            "expected a structural mismatch, got {errs:?}"
        );
    }
}

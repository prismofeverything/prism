//! The process-contract layer's teeth, end to end through chrysalis's
//! connection checker (`check::validate_connections`, the same pass `compile`
//! runs up front). A `Compare` step *demands* a contract on its input port
//! (`a :: DeterministicMassAction`); a producer wired into it must carry a
//! contract that *refines* the demanded one, or the program fails to compile.
//!
//! This is exactly the layer `biocompose` lacks: it would happily MSE any two
//! trajectories of matching shape. Here the comparison is typed by the shared
//! contract, so an illegitimate wiring is a compile error — and crucially the
//! check is `prism_schema::algebra::refines` (= `resolve(a,b)==a`), no new op.
//! See docs/process-contracts.md.

use indexmap::IndexMap;

use chrysalis::ast::*;
use chrysalis::check::validate_connections;

fn contract(name: &str, axes: &[(&str, &str)]) -> Def {
    Def::Contract(ContractDef {
        name: name.into(),
        axes: axes
            .iter()
            .map(|&(a, v)| (a.to_string(), v.to_string()))
            .collect(),
    })
}

/// `process Name ~{} ->{out: <out_type> :: contract}` (empty body — the test
/// exercises connection/contract checking, not execution).
fn producer(name: &str, out: &str, out_type: &str, contract_name: &str) -> Def {
    Def::Process(ProcessDef {
        name: name.into(),
        params: vec![],
        interface: Interface::new().with_output(
            out,
            PortDecl::required(SchemaExpr::custom(out_type))
                .with_contract(ContractRef::new(contract_name)),
        ),
        body: Expr::Record(IndexMap::new()),
    })
}

/// `step Compare ~{a: TimeSeries :: DeterministicMassAction} ->{mse: Float}`
fn compare_step() -> Def {
    Def::Step(StepDef {
        name: "Compare".into(),
        params: vec![],
        interface: Interface::new()
            .with_input(
                "a",
                PortDecl::required(SchemaExpr::custom("TimeSeries"))
                    .with_contract(ContractRef::new("DeterministicMassAction")),
            )
            .with_output("mse", PortDecl::required(SchemaExpr::Float)),
        body: Expr::Record(IndexMap::new()),
    })
}

/// A workflow composite: bind `slot = Producer`, then wire
/// `Compare ~{a: slot.out_port}`.
fn workflow(name: &str, slot: &str, producer_ctrl: &str, out_port: &str) -> Def {
    Def::Composite(CompositeDef {
        name: name.into(),
        params: vec![],
        interface: Interface::new(),
        using: vec![],
        body: Expr::parallel(vec![
            Expr::entry(slot, Expr::term(producer_ctrl).build()),
            Expr::term("Compare")
                .input("a", Expr::Path(PlacePath::local(slot).dot(out_port)))
                .build(),
        ]),
    })
}

fn base_program() -> Program {
    let mut p = Program::new();
    p.push(contract(
        "DeterministicMassAction",
        &[
            ("target", "MassActionODE"),
            ("claims", "Deterministic"),
            ("advance", "Continuous"),
        ],
    ));
    p.push(contract(
        "ConstraintBasedFlux",
        &[
            ("target", "SteadyStateFlux"),
            ("claims", "OptimalFlux"),
            ("advance", "SteadyState"),
        ],
    ));
    p.push(producer(
        "Rk4",
        "trajectory",
        "TimeSeries",
        "DeterministicMassAction",
    ));
    p.push(producer("Fba", "flux", "FluxVector", "ConstraintBasedFlux"));
    p.push(compare_step());
    p
}

#[test]
fn fulfilling_producer_wires_clean() {
    let mut p = base_program();
    p.push(workflow("Good", "r", "Rk4", "trajectory"));
    let errs = validate_connections(&p);
    assert!(
        errs.is_empty(),
        "Rk4 fulfills DeterministicMassAction — should wire clean; got {errs:?}"
    );
}

#[test]
fn wrong_target_producer_is_rejected() {
    let mut p = base_program();
    p.push(workflow("Bad", "f", "Fba", "flux"));
    let errs = validate_connections(&p);
    assert!(
        errs.iter().any(|e| e.message.contains("contract mismatch")),
        "Fba targets steady-state flux, not the mass-action ODE — must be rejected; got {errs:?}"
    );
}

#[test]
fn uncontracted_producer_is_rejected() {
    // A producer that declares no contract on its output cannot satisfy a
    // port that demands one — underspecification is a compile error.
    let mut p = base_program();
    p.push(Def::Process(ProcessDef {
        name: "Mystery".into(),
        params: vec![],
        interface: Interface::new().with_output(
            "trajectory",
            PortDecl::required(SchemaExpr::custom("TimeSeries")),
        ),
        body: Expr::Record(IndexMap::new()),
    }));
    p.push(workflow("Murky", "m", "Mystery", "trajectory"));
    let errs = validate_connections(&p);
    assert!(
        errs.iter().any(|e| e.message.contains("declares none")),
        "an uncontracted source must be rejected by a demanding port; got {errs:?}"
    );
}

// ── Contract forwarding through a generic wrapper (RunProcess), task #14 ──
// A native wrapper like `RunProcess` has no chrysalis interface of its own; the
// contract lives on the inner `proc:` process. The wrapper must FORWARD it so a
// demanding consumer is still checked — "a contract survives a generic wrapper".

/// `process Name ~{state: Float} ->{state: Float :: contract}` — the shape of a
/// real integrator: its `fulfills` rides the `state` output, the port a
/// `RunProcess` wrapping it (`proc: Name[…]`) forwards onto its `timeseries`.
fn integrator(name: &str, contract_name: &str) -> Def {
    Def::Process(ProcessDef {
        name: name.into(),
        params: vec![],
        interface: Interface::new()
            .with_input("state", PortDecl::required(SchemaExpr::Float))
            .with_output(
                "state",
                PortDecl::required(SchemaExpr::Float)
                    .with_contract(ContractRef::new(contract_name)),
            ),
        body: Expr::Record(IndexMap::new()),
    })
}

/// The flagship's shape: `rk = RunProcess[proc: <inner>] ->{timeseries: traj}`
/// wired into `Compare ~{a: traj}` (which demands `DeterministicMassAction`).
/// `traj` carries whatever contract RunProcess forwards from the inner's
/// `state` output. `RunProcess` is left undefined (a native import has no Def),
/// exactly as in a real program.
fn run_process_workflow(name: &str, inner_ctrl: &str) -> Def {
    Def::Composite(CompositeDef {
        name: name.into(),
        params: vec![],
        interface: Interface::new(),
        using: vec![],
        body: Expr::parallel(vec![
            Expr::entry(
                "rk",
                Expr::term("RunProcess")
                    .arg_named("proc", Expr::term(inner_ctrl).build())
                    .output("timeseries", Expr::Var("traj".into()))
                    .build(),
            ),
            Expr::term("Compare")
                .input("a", Expr::Var("traj".into()))
                .build(),
        ]),
    })
}

#[test]
fn runprocess_forwards_inner_contract() {
    // RunProcess has no chrysalis interface; the contract lives on the inner
    // `proc:` process. Forwarding must carry it across the wrapper so the
    // demanding Compare accepts it.
    let mut p = base_program();
    p.push(integrator("Rk4Step", "DeterministicMassAction"));
    p.push(run_process_workflow("WrapGood", "Rk4Step"));
    let errs = validate_connections(&p);
    assert!(
        errs.is_empty(),
        "RunProcess wrapping a DeterministicMassAction integrator must satisfy \
         Compare's demand via the forwarded contract; got {errs:?}"
    );
}

#[test]
fn runprocess_with_wrong_target_inner_is_rejected() {
    // The forwarded contract is the INNER process's real contract, so a
    // wrong-target inner is rejected exactly as a directly-wired one would be.
    let mut p = base_program();
    p.push(integrator("FbaStep", "ConstraintBasedFlux"));
    p.push(run_process_workflow("WrapBad", "FbaStep"));
    let errs = validate_connections(&p);
    assert!(
        errs.iter().any(|e| e.message.contains("contract mismatch")),
        "RunProcess wrapping a steady-state-flux process must NOT satisfy a \
         DeterministicMassAction demand; got {errs:?}"
    );
}

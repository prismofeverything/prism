//! The process-contract surface, parsed from `.ys` source and enforced —
//! `contract …`, `:: C` on a port, and `fulfills C[…]` sugar. The wrong-target
//! wire is a *compile* error, the layer biocompose lacks. See
//! docs/process-contracts.md.

use chrysalis::ast::Def;
use chrysalis::check::validate_connections;
use chrysalis::parse::parse_program;

const SRC: &str = r#"
contract DeterministicMassAction (target: MassActionODE, claims: Deterministic, advance: Continuous)
contract ConstraintBasedFlux (target: SteadyStateFlux, claims: OptimalFlux, advance: SteadyState)

extern Rk4 ->{trajectory: TimeSeries} fulfills DeterministicMassAction[method: Rk4]
extern Fba ->{flux: FluxVector} fulfills ConstraintBasedFlux

step Compare ~{a: TimeSeries :: DeterministicMassAction} ->{mse: Float} ( {mse: 0.0} )

composite Good ( r = Rk4 | Compare ~{a: r.trajectory} )
composite Bad  ( f = Fba | Compare ~{a: f.flux} )
"#;

#[test]
fn contract_surface_parses() {
    let prog = parse_program(SRC).expect("contract surface should parse");

    let contracts = prog
        .defs
        .iter()
        .filter(|d| matches!(d, Def::Contract(_)))
        .count();
    assert_eq!(contracts, 2, "two `contract` declarations");

    // `fulfills C[method: Rk4]` lowered onto Rk4's output port.
    let Def::Extern(rk4) = prog.lookup("Rk4").expect("Rk4 def") else {
        panic!("Rk4 should be extern");
    };
    let traj = rk4.interface.outputs.get("trajectory").unwrap();
    let c = traj.contract.as_ref().expect("trajectory carries a contract");
    assert_eq!(c.name, "DeterministicMassAction");
    assert_eq!(c.pins.get("method").map(String::as_str), Some("Rk4"));
}

#[test]
fn parsed_program_enforces_contracts() {
    let prog = parse_program(SRC).expect("should parse");
    let errs = validate_connections(&prog);

    // Bad wires Fba (ConstraintBasedFlux) into a port demanding the mass-action
    // ODE — a contract mismatch.
    assert!(
        errs.iter()
            .any(|e| e.composite == "Bad" && e.message.contains("contract mismatch")),
        "Bad must be rejected (Fba ⋣ DeterministicMassAction); got {errs:?}"
    );
    // Good wires Rk4, which fulfills the demanded contract — clean.
    assert!(
        !errs.iter().any(|e| e.composite == "Good"),
        "Good must wire clean (Rk4 fulfills DeterministicMassAction); got {errs:?}"
    );
}

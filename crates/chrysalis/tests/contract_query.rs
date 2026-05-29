//! The contract-indexed process library: `fulfillers(C)` returns every definer
//! whose declared contract `refines` C — the multi-axis substitutability query
//! that turns a hand-wired pair into a queried set. A method-open demand
//! returns every method realizing the shared target; a wrong-target definer is
//! excluded; pinning an axis narrows the result. Matching is the existing
//! `prism_schema::algebra::refines`, not a new rule.

use indexmap::IndexMap;

use chrysalis::ast::*;
use chrysalis::contract::fulfillers;

fn contract(name: &str, axes: &[(&str, &str)]) -> Def {
    Def::Contract(ContractDef {
        name: name.into(),
        axes: axes
            .iter()
            .map(|&(a, v)| (a.to_string(), v.to_string()))
            .collect(),
    })
}

/// `process Name ~{state} ->{state :: C[method: <method>]}` — a fulfiller that
/// pins its method, exactly as a real `process … fulfills C[method: …]` does.
fn fulfiller(name: &str, contract_name: &str, method: &str) -> Def {
    Def::Process(ProcessDef {
        name: name.into(),
        params: vec![],
        interface: Interface::new().with_output(
            "state",
            PortDecl::required(SchemaExpr::Float)
                .with_contract(ContractRef::new(contract_name).pin("method", method)),
        ),
        body: Expr::Record(IndexMap::new()),
    })
}

/// A small library: two deterministic-mass-action integrators (different
/// methods) and one constraint-based-flux process (a different target).
fn library() -> Program {
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
    p.push(fulfiller("Rk4", "DeterministicMassAction", "Rk4"));
    p.push(fulfiller("ForwardEuler", "DeterministicMassAction", "ForwardEuler"));
    p.push(fulfiller("Fba", "ConstraintBasedFlux", "Highs"));
    p
}

#[test]
fn fulfillers_returns_every_method_open_refiner() {
    // Demand pins target+claims+advance, leaves `method` OPEN → both
    // (method-pinned) integrators qualify; the flux process targets a different
    // object and is excluded. This is the "run every fulfiller of C" set.
    let p = library();
    let mut got = fulfillers(&p, &ContractRef::new("DeterministicMassAction"));
    got.sort();
    assert_eq!(
        got,
        vec!["ForwardEuler", "Rk4"],
        "both DeterministicMassAction methods qualify; Fba (a different target) is excluded"
    );
}

#[test]
fn fulfillers_of_a_different_target_are_disjoint() {
    let p = library();
    let got = fulfillers(&p, &ContractRef::new("ConstraintBasedFlux"));
    assert_eq!(
        got,
        vec!["Fba"],
        "only the steady-state-flux process fulfills ConstraintBasedFlux"
    );
}

// ── T4 (quick proof): the fan-out is driven by the library query ─────────
// The surface "run every fulfiller of C" is compile-time codegen (deferred to
// the packages work). But the DRIVER — `fulfillers(C)` over the real workflow's
// library — yields exactly the set the demo runs and compares. The fan-out is
// "replace the hand-wired control list with this query"; the demo already
// runs+compares that set (ys_files_run), so only the auto-wiring is outstanding.
#[test]
fn fulfillers_query_drives_the_agreement_demos_comparison_set() {
    // parse_FILE resolves the `import` so the library's fulfillers are merged in.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys/agreement.ys");
    let program = chrysalis::parse::parse_file(&path).expect("parse_file agreement.ys");

    let mut deterministic = fulfillers(&program, &ContractRef::new("DeterministicMassAction"));
    deterministic.sort();
    assert_eq!(
        deterministic,
        vec!["ForwardEuler", "Rk4"],
        "the deterministic lane's fulfillers, DERIVED from the contract (not hand-listed)"
    );

    let mut cme = fulfillers(&program, &ContractRef::new("ExactCME"));
    cme.sort();
    assert_eq!(
        cme,
        vec!["Ssa", "SsaEnsemble"],
        "the CME lane's fulfillers, derived from the ExactCME contract"
    );
}

#[test]
fn pinning_a_method_narrows_the_result() {
    // A demand that ALSO pins `method: Rk4` selects exactly that method —
    // ForwardEuler's pinned method conflicts and is dropped.
    let p = library();
    let got = fulfillers(
        &p,
        &ContractRef::new("DeterministicMassAction").pin("method", "Rk4"),
    );
    assert_eq!(
        got,
        vec!["Rk4"],
        "a method-pinned demand selects exactly the matching method"
    );
}

#[test]
fn fulfillers_queries_an_imported_library() {
    // The package payoff under EXPLICIT named selection (#50): `cme-gillespie.ys`
    // declares no processes of its own — it imports exactly the two it uses,
    // `from chrysalis.lib.simulators import SsaEnsemble, DistCompare`. After
    // `parse_file` merges them, `fulfillers(C)` queries the IMPORTED processes by
    // contract — finding only what was imported, never a whole-file dump.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys/cme-gillespie.ys");
    let program = chrysalis::parse::parse_file(&path).expect("parse_file cme-gillespie.ys");

    // `SsaEnsemble` fulfills ExactCME; `DistCompare` DEMANDS it (not a fulfiller).
    // `Ssa` also fulfills ExactCME but is NOT imported, so it is NOT queryable.
    let cme = fulfillers(&program, &ContractRef::new("ExactCME"));
    assert_eq!(
        cme,
        vec!["SsaEnsemble"],
        "only the imported CME fulfiller — explicit selection, not a whole-file dump"
    );

    // No deterministic fulfiller (Rk4/ForwardEuler) is imported, so none is
    // queryable — the inverse of the old whole-file merge, which pulled them all.
    let det = fulfillers(&program, &ContractRef::new("DeterministicMassAction"));
    assert!(
        det.is_empty(),
        "no deterministic fulfiller imported ⇒ none queryable (#50); got {det:?}"
    );
}

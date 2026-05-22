//! Process-contract substitutability **is the existing schema join** —
//! no new algebra operation. (Design: `docs/process-contracts.md`.)
//!
//! A contract is encoded as a `Schema`: a nominal `Custom` record whose
//! parameters are the four axes, each axis a sort chosen so its
//! refinement order falls out of `resolve`:
//!
//!   - target / advance / method → nominal `Custom` (parameter-free).
//!     `resolve` keeps a same-named `Custom` and prefers the *update* for a
//!     differently-named one, so two same-named axes resolve to themselves
//!     and a mismatched one does not — i.e. nominal equality, exactly what
//!     these axes need.
//!   - claims → `Enum` carrying its *downset* (the claim plus everything
//!     weaker it implies). The existing Enum-union `resolve` then yields the
//!     chain: a stronger claim's downset ⊇ a weaker one's.
//!
//! "A process with contract A may fill a slot demanding contract B" is the
//! lattice order `A ⊒ B`, which the join characterizes as
//! `resolve(A, B) == A`. That predicate is *derived from* `resolve`; it
//! adds nothing to the algebra. `fulfills` / `::` thread the contract into
//! the port type as a parameter, so the compiler's existing wire-schema
//! resolution enforces all of this — the negative cases below are exactly
//! what fails to compile.

use indexmap::IndexMap;
use prism_schema::resolve::resolve;
use prism_schema::{Key, Schema};

/// Substitutability: `A ⊒ B  ⟺  resolve(A, B) == A`. Derived from the
/// join — not a new operation.
fn refines(a: &Schema, b: &Schema) -> bool {
    resolve(a, b) == *a
}

/// A parameter-free nominal axis value (target / method / advance).
fn nominal(name: &str) -> Schema {
    Schema::Custom {
        name: name.into(),
        parameters: IndexMap::new(),
    }
}

/// An ordered axis value (an equivalence claim) as its downset: the claim
/// plus everything weaker it implies. Listing order is irrelevant to the
/// passing direction (a superset resolves to itself).
fn claim(downset: &[&str]) -> Schema {
    Schema::Enum {
        values: downset.iter().map(|s| s.to_string()).collect(),
        default: None,
    }
}

// The claim chain, strongest → weakest.
fn pathwise() -> Schema {
    claim(&["pathwise", "distributional", "weak_order"])
}
fn distributional() -> Schema {
    claim(&["distributional", "weak_order"])
}
fn weak_order() -> Schema {
    claim(&["weak_order"])
}
/// A single-trajectory determinism claim — its own point in claim-space.
fn deterministic() -> Schema {
    claim(&["deterministic"])
}

/// A contract: a nominal `Custom` whose parameters are the axes. `method`
/// is optional — a *demanded* contract that leaves it open admits any
/// method (the candidate's method unions in under `resolve`).
fn contract(
    name: &str,
    target: Schema,
    claims: Schema,
    advance: Schema,
    method: Option<Schema>,
) -> Schema {
    let mut params: IndexMap<Key, Schema> = IndexMap::new();
    params.insert(Key::from("target"), target);
    params.insert(Key::from("claims"), claims);
    params.insert(Key::from("advance"), advance);
    if let Some(m) = method {
        params.insert(Key::from("method"), m);
    }
    Schema::Custom {
        name: name.into(),
        parameters: params,
    }
}

/// The contract both integrators fulfill. `method` open ⇒ a demanded slot;
/// `Some(_)` ⇒ a concrete fulfiller.
fn dma(method: Option<Schema>) -> Schema {
    contract(
        "DeterministicMassAction",
        nominal("MassActionODE"),
        deterministic(),
        nominal("Continuous"),
        method,
    )
}

#[test]
fn integrators_fulfill_the_shared_contract() {
    let demanded = dma(None); // Compare[under: DeterministicMassAction]
    let rk4 = dma(Some(nominal("Rk4")));
    let euler = dma(Some(nominal("ForwardEuler")));
    assert!(refines(&rk4, &demanded), "Rk4 fulfills the contract");
    assert!(refines(&euler, &demanded), "ForwardEuler fulfills the contract");
}

#[test]
fn fba_is_rejected_for_a_different_target() {
    // HiGHS/FBA: constraint-based steady-state flux — a different target.
    let demanded = dma(None);
    let fba = contract(
        "ConstraintBasedFlux",
        nominal("SteadyStateFlux"),
        claim(&["optimal_flux"]),
        nominal("SteadyState"),
        Some(nominal("Simplex")),
    );
    assert!(
        !refines(&fba, &demanded),
        "FBA targets steady-state flux, not the mass-action ODE"
    );
}

#[test]
fn the_label_is_not_enough_axes_are_checked() {
    // A process labelled DeterministicMassAction but actually targeting the
    // CME is still rejected — substitutability checks the axes, not the name.
    let demanded = dma(None);
    let liar = contract(
        "DeterministicMassAction",
        nominal("CME"), // ← wrong target, right label
        deterministic(),
        nominal("Continuous"),
        Some(nominal("Rk4")),
    );
    assert!(
        !refines(&liar, &demanded),
        "wrong target axis ⇒ not substitutable, label notwithstanding"
    );
}

#[test]
fn an_underspecified_candidate_is_rejected() {
    // A slot demanding a Deterministic claim won't take a process that never
    // declares its claim.
    let demanded = dma(None);
    let mut params = IndexMap::new();
    params.insert(Key::from("target"), nominal("MassActionODE"));
    params.insert(Key::from("advance"), nominal("Continuous"));
    let candidate = Schema::Custom {
        name: "DeterministicMassAction".into(),
        parameters: params, // no `claims`
    };
    assert!(
        !refines(&candidate, &demanded),
        "missing the claims axis ⇒ not substitutable"
    );
}

#[test]
fn a_pinned_method_must_match() {
    // If a slot pins method: Rk4, ForwardEuler cannot fill it.
    let demand_rk4 = dma(Some(nominal("Rk4")));
    assert!(refines(&dma(Some(nominal("Rk4"))), &demand_rk4));
    assert!(
        !refines(&dma(Some(nominal("ForwardEuler"))), &demand_rk4),
        "Euler ≠ the pinned Rk4 method"
    );
}

#[test]
fn the_claim_chain_is_directional() {
    // A stronger claim refines a slot demanding a weaker one — not the
    // reverse. This is the whole point of an *ordered* axis.
    assert!(refines(&pathwise(), &distributional()), "pathwise ⇒ distributional");
    assert!(refines(&distributional(), &weak_order()), "distributional ⇒ weak-order");
    assert!(refines(&pathwise(), &weak_order()), "transitive");
    assert!(!refines(&distributional(), &pathwise()), "distributional ⇏ pathwise");
    assert!(!refines(&weak_order(), &distributional()), "weak-order ⇏ distributional");
}

#[test]
fn refinement_is_reflexive() {
    let c = dma(Some(nominal("Rk4")));
    assert!(refines(&c, &c));
}

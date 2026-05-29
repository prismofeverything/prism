//! The contract-indexed process library — querying *which processes fulfill a
//! given process-contract*. This is the "multi-axis enum query": a `demanded`
//! contract pins some axes (e.g. `target` + `claims`) and leaves others open
//! (e.g. `method`), and [`fulfillers`] returns every definer whose declared
//! contract `refines` it.
//!
//! Matching is `prism_schema::algebra::refines` (= the schema join
//! `resolve(a,b)==a`), REUSED — the library adds an index/query, not a new
//! matching rule (CLAUDE.md: the schema layer is a closed algebra; chrysalis is
//! a thin layer over prism). A `process` / `step` / `composite` declares the
//! contract it fulfills with `fulfills C`, which rides its output ports (see
//! `parse::apply_fulfills` and `check`). So the "library" is just the program's
//! contract-bearing definers; the query scans them and keeps the refiners.
//!
//! A precomputed by-axis index is a later perf concern (`project_ys_primary_
//! language`: features first, perf sweep after) — a linear scan over `refines`
//! is correct and the demo's libraries are small.

use crate::ast::{ContractRef, Def, Interface, Program};
use crate::schema::contract_ref_schema;

/// Name + interface of a definer that can carry a `fulfills` contract. Mirrors
/// `check::interface_of` but also yields the name (the control usable in term
/// position) — kept local so the query module stands alone.
fn def_interface(def: &Def) -> Option<(&str, &Interface)> {
    match def {
        Def::Process(p) => Some((p.name.as_str(), &p.interface)),
        Def::Step(s) => Some((s.name.as_str(), &s.interface)),
        Def::Composite(c) => Some((c.name.as_str(), &c.interface)),
        _ => None,
    }
}

/// Every definer in `program` whose declared contract `refines` the `demanded`
/// one — the contract-indexed process-library query, and the substrate for the
/// "run every fulfiller of C" workflow (the structural fan-out).
///
/// A method-open `demanded` (e.g. `DeterministicMassAction` with no `method`
/// pin) returns every method realizing that target+claims; pinning an axis
/// narrows the result; a wrong-target definer is excluded. The returned names
/// are controls usable in term position (`<Name>[…]`).
///
/// A definer fulfills `demanded` if ANY of its output ports carries a contract
/// that refines it (`fulfills C` normally stamps the same `C` on every output).
pub fn fulfillers<'a>(program: &'a Program, demanded: &ContractRef) -> Vec<&'a str> {
    let dem = contract_ref_schema(demanded, program);
    program
        .defs
        .iter()
        .filter_map(|def| {
            let (name, iface) = def_interface(def)?;
            let fulfills = iface.outputs.values().any(|pd| {
                pd.contract
                    .as_ref()
                    .map(|c| {
                        prism_schema::algebra::refines(&contract_ref_schema(c, program), &dem)
                    })
                    .unwrap_or(false)
            });
            fulfills.then_some(name)
        })
        .collect()
}

//! **Axis-A completeness** — definition-as-data round-trips across the value-form
//! kinds (`docs/homoiconic-unification.md` gap 3). Before this, `EntityDef` /
//! `Program` `from_value` rebuilt only process / step / composite / binding;
//! `reaction` + `function` were *serialized but dropped on reify*, and `type`
//! wasn't serialized at all. Now all six round-trip, so the "definition-as-data
//! is uniform across every (value-form) kind" premise the Stage-4 FLAT/RICH
//! dissolution leans on is actually true for them.
//!
//! The strong witness is `quote ∘ reify ∘ quote == quote`: if any kind is
//! dropped on reify, re-quoting the reconstruction diverges from the original
//! quote — a total, structural check, not a sampled one.
//!
//! Now also `pattern`, `contract`, `protocol`. Remaining (each needs a sub-type
//! codec, noted in `from_value`): `unit` (`Dimension`/`UnitExpr`/`Ratio`) and
//! `context` (`ContextRule`) still emit a slot-name marker only.

use chrysalis::ast::Program;
use chrysalis::parse::parse_program;

/// One program touching every round-trippable kind: a bare `type`, a `type` WITH
/// a method, a `function`, a `process`, a `reaction` (redex/reactum over a sorted
/// site), a `pattern`, a `composite`, a `contract`, and a `protocol`.
const ALL_KINDS: &str = "\
type Mass = float

type Box = { items: list[string] }
with {
  add(item: string) = { items: { _add: [item] } }
}

def helper(x) = x + 1.0

process Tick ~{count :: Float} ->{count :: Float} (
  {count: 1.0}
)

reaction Grow (
  ?c :: Cell => ?c
)

pattern Held[item] (
  Holder (contents: item)
)

composite Cell ->{count :: Float} (
  count: 0.0 |
  tick: Tick ~{count: count} ->{count: count}
)

contract Fast (target: MassAction, claims: Deterministic, advance: Continuous)

protocol StreamingCell = stream<Cell, path: 'cell.ys'>
";

#[test]
fn every_value_form_kind_round_trips_as_data() {
    let prog = parse_program(ALL_KINDS).expect("parse");
    let quoted = prog.to_value();
    let reified = Program::from_value(&quoted).expect("reify the quoted program");
    // The total round-trip: any dropped kind makes the re-quote differ.
    assert_eq!(
        reified.to_value(),
        quoted,
        "quote ∘ reify ∘ quote == quote — every value-form kind reconstructs"
    );
}

#[test]
fn reified_program_carries_each_kind() {
    // Guard against a vacuous pass (both sides dropping the same thing): the
    // reconstruction must actually hold each kind's slot.
    let prog = parse_program(ALL_KINDS).expect("parse");
    let reified = Program::from_value(&prog.to_value()).expect("reify");
    let ents = reified.owned_entities();
    let by = |n: &str| ents.iter().find(|e| e.name == n);

    assert!(by("Mass").is_some_and(|e| e.type_def.is_some()), "type Mass reconstructed");
    assert!(
        by("Box").is_some_and(|e| e.type_def.as_ref().is_some_and(|t| !t.methods.is_empty())),
        "type Box reconstructed WITH its method"
    );
    assert!(by("helper").is_some_and(|e| e.function.is_some()), "function helper reconstructed");
    assert!(by("Tick").is_some_and(|e| e.process.is_some()), "process Tick reconstructed");
    assert!(by("Grow").is_some_and(|e| e.reaction.is_some()), "reaction Grow reconstructed");
    assert!(by("Held").is_some_and(|e| e.pattern.is_some()), "pattern Held reconstructed");
    assert!(by("Cell").is_some_and(|e| e.composite.is_some()), "composite Cell reconstructed");
    assert!(by("Fast").is_some_and(|e| e.contract.is_some()), "contract Fast reconstructed");
    assert!(
        by("StreamingCell").is_some_and(|e| e.protocol.is_some()),
        "protocol StreamingCell reconstructed"
    );
}

#[test]
fn reaction_guard_and_rate_survive_quote() {
    // A guarded + rated reaction's `where` / `rate` are part of its data form;
    // dropping them (the pre-Axis-A shape did) would silently change a reaction's
    // dynamics through `quote`. Round-trip must preserve both.
    const GUARDED: &str =
        "reaction Pour[k :: float = 0.5] ( (?c :: Cell) where k > 0.0 => ?c ) rate ( k )\n";
    let prog = parse_program(GUARDED).expect("parse guarded reaction");
    let reified = Program::from_value(&prog.to_value()).expect("reify");
    let ents = reified.owned_entities();
    let rxn = ents
        .iter()
        .find(|e| e.name == "Pour")
        .and_then(|e| e.reaction.as_ref())
        .expect("reaction Pour reconstructed");
    assert!(rxn.guard.is_some(), "the `where` guard survived quote ↔ reify");
    assert!(rxn.rate.is_some(), "the `rate` survived quote ↔ reify");
    assert_eq!(rxn.params.len(), 1, "the typed param `k` survived");
}

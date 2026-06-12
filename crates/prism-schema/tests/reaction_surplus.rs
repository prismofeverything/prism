//! Reaction firing — conservation through a *splitting* reactum (division). lang flagged
//! a drop via `mr_cell::mr_colony_grows_by_division` (4,4,4 → 2,2,2) and suspected the
//! `?rest` surplus re-emit (`absorb_surplus`). This tight prism-schema repro CORRECTS
//! that diagnosis:
//!
//! - [`surplus_rest_is_conserved_through_a_splitting_reactum`] (PASSES) — a SINGLE divide
//!   conserves the surplus, so the rest capture + re-emit is CORRECT (not the bug).
//! - [`repeated_divide_conserves_to_fixpoint`] (IGNORED — the REAL bug) — REPEATED divide
//!   drops atoms: the reactum's fixed daughter key `bud` collides across fires, merging
//!   and losing a cell. The surplus only matters because it lets the cell divide *again*.
//!   The fix: fresh-key new sorted ions (Milner — new ions get fresh names), unifying the
//!   List reactum path (already fresh via `gensym_node`) with the Map path. See
//!   `coord/core.next` + memory `reaction_keying_fresh_ions`.

use prism_schema::reaction::{apply_fire, find_matches, fire_rule_at, Pattern, ReactionRule};
use prism_schema::value::Value;

/// An atom node `{_type: t}`.
fn atom(t: &str) -> Value {
    Value::tree([("_type", Value::String(t.to_string()))])
}

/// A single-sort pattern with no fields — `{_type: t}` (matches an [`atom`]).
fn sort(t: &str) -> Pattern {
    Pattern::sort(t, Vec::<(&str, Pattern)>::new())
}

/// Count atoms tagged `_type: "A"` anywhere in the tree.
fn count_a(v: &Value) -> usize {
    fn walk(v: &Value, n: &mut usize) {
        if v.get_field("_type").and_then(|t| t.as_str()) == Some("A") {
            *n += 1;
        }
        if let Some(m) = v.as_map() {
            for (k, c) in m.iter() {
                if !k.starts_with('_') {
                    walk(c, n);
                }
            }
        }
    }
    let mut n = 0;
    walk(v, &mut n);
    n
}

#[test]
fn surplus_rest_is_conserved_through_a_splitting_reactum() {
    // A membrane with 4 atoms keyed x0..x3 (NONE named by the redex, so the redex's
    // reactant keys match COMBINATORIALLY as labels — the case after a first divide
    // re-keys the cell, exactly the chrysalis multi-divide scenario).
    let state = Value::tree([(
        "cell",
        Value::tree([
            ("_type", Value::String("Membrane".into())),
            ("x0", atom("A")),
            ("x1", atom("A")),
            ("x2", atom("A")),
            ("x3", atom("A")),
        ]),
    )]);
    assert_eq!(count_a(&state), 4, "seed: 4 atoms");

    // divide: target keeps the rest, bud is new. `p`/`q` are LABELS (not state keys) →
    // they bind two of the atoms combinatorially; `rest` captures the other two.
    //   (tgt: Membrane(p:A | q:A | rest:?rest))
    //   => (tgt: Membrane(n:A | rest:?rest) | bud: Membrane(m:A))
    let redex = Pattern::map([(
        "tgt",
        Pattern::sort(
            "Membrane",
            [("p", sort("A")), ("q", sort("A")), ("rest", Pattern::site())],
        ),
    )]);
    let reactum = Pattern::map([
        (
            "tgt",
            Pattern::sort("Membrane", [("n", sort("A")), ("rest", Pattern::site())]),
        ),
        ("bud", Pattern::sort("Membrane", [("m", sort("A"))])),
    ]);
    let rule = ReactionRule::new(redex, reactum);

    let matches = find_matches(&state, &rule.redex, None);
    assert!(!matches.is_empty(), "the divide redex matches the membrane");
    let update = fire_rule_at(&rule, &matches[0]).expect("fire produced an update");
    let result = apply_fire(&state, &update, None);

    assert_eq!(
        count_a(&result),
        4,
        "a SINGLE divide conserves the ?rest surplus (x2,x3) — the re-emit is correct",
    );
}

#[test]
#[ignore = "KNOWN BUG (reaction keying): repeated divide collides on the reactum's fixed \
            daughter key `bud`, merging + losing a cell (divide 0 conserves, divide 1 drops). \
            Fix = fresh-key new sorted ions (the List path's gensym_node), unified across \
            List/Map reactums. See coord/core.next + memory reaction_keying_fresh_ions."]
fn repeated_divide_conserves_to_fixpoint() {
    // The chrysalis GROW case divides REPEATEDLY (a big cell keeps splitting). Mimic the
    // BRS-to-fixpoint over the SAME divide rule and check conservation at every step —
    // isolating whether the drop is the surplus re-emit (single fire, above) or a
    // repeated-divide effect (the fixed `bud` key colliding across fires).
    let mut state = Value::tree([(
        "cell",
        Value::tree([
            ("_type", Value::String("Membrane".into())),
            ("x0", atom("A")),
            ("x1", atom("A")),
            ("x2", atom("A")),
            ("x3", atom("A")),
        ]),
    )]);
    let start = count_a(&state);
    assert_eq!(start, 4);

    let redex = Pattern::map([(
        "tgt",
        Pattern::sort(
            "Membrane",
            [("p", sort("A")), ("q", sort("A")), ("rest", Pattern::site())],
        ),
    )]);
    let reactum = Pattern::map([
        (
            "tgt",
            Pattern::sort("Membrane", [("n", sort("A")), ("rest", Pattern::site())]),
        ),
        ("bud", Pattern::sort("Membrane", [("m", sort("A"))])),
    ]);
    let rule = ReactionRule::new(redex, reactum);

    for step in 0..20 {
        let matches = find_matches(&state, &rule.redex, None);
        if matches.is_empty() {
            break;
        }
        let update = fire_rule_at(&rule, &matches[0]).expect("fire");
        state = apply_fire(&state, &update, None);
        let now = count_a(&state);
        eprintln!("after divide {step}: {now} atoms");
        assert_eq!(now, start, "conservation must hold at EVERY divide (step {step})");
    }
}

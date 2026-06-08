//! #40 / #43 — the cross-composite LINK-GRAPH redex on the surface.
//!
//! A cross-composite reaction couples two SEALED composites by their published
//! ports on a SHARED LINK — `?west :: Cell ~{edge: ~e} | ?east :: Cell ~{edge:
//! ~e}` — lifting MAPK's `~bond` to the composite scale (memory
//! `feedback_no_case_heuristics`). `?west`/`?east` are site binders (the `?`
//! head — restriction (1) removed: a redex item head may be `?x`, not only
//! `K[args]`); `~e` is the shared link var that COUPLES them. The match is on the
//! composites' port wires, NEVER by descent into inner state (encapsulation-clean).
//!
//! This probes the SURFACE path end-to-end: parse → `eval_pattern_top` lower →
//! `find_matches`. The matcher itself is the existing `Pattern::LinkVar` machinery
//! (the same that binds `~bond` between two molecules); this proves the surface
//! syntax lowers to it for composites.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_schema::{find_matches, MethodRegistry, Value};

use chrysalis::ast::Program;
use chrysalis::eval::Evaluator;
use chrysalis::runtime::rule::RuleBindings;

fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String((*s).into())).collect())
}

/// A sealed composite Cell node exposing an `edge` input port wired to `edge`.
fn cell(edge: &[&str]) -> Value {
    Value::tree([
        ("_type", Value::String("Cell".into())),
        ("inputs", Value::tree([("edge", wire(edge))])),
    ])
}

/// Parse a program, lower the named reaction's redex to a `prism_schema::Pattern`.
fn lower_redex(src: &str, name: &str) -> prism_schema::reaction::Pattern {
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let def = program
        .entity(name)
        .and_then(|v| v.reaction.cloned())
        .unwrap_or_else(|| panic!("no reaction {name}"));
    let ev = Evaluator::new(Arc::new(program), Arc::new(MethodRegistry::new()));
    let mut bindings = RuleBindings::new();
    ev.eval_pattern_top(&def.redex, &IndexMap::new(), &mut bindings)
        .expect("lower redex")
}

const DIFFUSE: &str = r#"
reaction Diffuse (
  ?west :: Cell ~{edge: ~e} | ?east :: Cell ~{edge: ~e} => ?west | ?east
)
"#;

#[test]
fn link_graph_redex_couples_two_composites_on_a_shared_edge() {
    let redex = lower_redex(DIFFUSE, "Diffuse");

    // Two cells whose `edge` ports wire to the SAME link → coupled → match.
    let coupled = Value::tree([
        ("a", cell(&["_links", "e"])),
        ("b", cell(&["_links", "e"])),
    ]);
    let matches = find_matches(&coupled, &redex, None);
    assert!(
        !matches.is_empty(),
        "two composites on the same edge link must couple (match): {redex:?}"
    );
    // Both composite keys are consumed by the redex.
    let consumed: std::collections::HashSet<String> = matches[0]
        .bindings
        .key_map
        .values()
        .map(|k| k.to_string())
        .collect();
    assert!(
        consumed.contains("a") && consumed.contains("b"),
        "both coupled composites are bound; got {consumed:?}"
    );
}

#[test]
fn link_graph_redex_does_not_couple_composites_on_different_edges() {
    let redex = lower_redex(DIFFUSE, "Diffuse");

    // Different edge links → the shared `~e` cannot unify → no coupling.
    let uncoupled = Value::tree([
        ("a", cell(&["_links", "e"])),
        ("b", cell(&["_links", "f"])),
    ]);
    let matches = find_matches(&uncoupled, &redex, None);
    assert!(
        matches.is_empty(),
        "composites on DIFFERENT edge links must NOT couple: {matches:?}"
    );
}

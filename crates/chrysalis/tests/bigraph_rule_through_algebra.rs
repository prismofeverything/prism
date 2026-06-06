//! End-to-end #42 / merge-protocol slice 5: a chrysalis `Rule`, converted
//! closure-free to a prism `ReactionRule`, rides a `:: bigraph` port as a
//! Foreign-tagged Value; the algebra's `apply_with(Custom{"bigraph"}, …)`
//! dispatches to `BigraphTypeMethods::apply` and fires the reaction.
//!
//! This is the chrysalis surface end of the substrate that
//! `prism-schema/tests/bigraph_type.rs` proves at the prism layer. With
//! the closure-free converter in place, a `.ys` `reaction R = …` can
//! travel across a composite boundary as data — the conceptual leap from
//! "reactions live in BRS processes" to "reactions are typed updates."

use std::sync::Arc;

use indexmap::IndexMap;
use prism_schema::{algebra, Key, Pattern, Schema, TypeRegistry, Value};

use chrysalis::runtime::rule::{to_bigraph_value, to_structural_rule, Reactum, Rule, RuleBindings};

fn val_str(s: &str) -> Value {
    Value::String(s.to_string())
}

/// Build a chrysalis `Rule` for ERK→pERK — the same shape the parser
/// produces for a STRUCTURAL `reaction Phos = ?cell.compartment::Compartment[…] ⟶ …`.
fn structural_chrysalis_rule() -> Rule {
    Rule {
        label: "phos".to_string(),
        redex: Pattern::map([(
            "compartment",
            Pattern::sort(
                "Compartment",
                [
                    ("substrate", Pattern::sort("ERK", Vec::<(&str, Pattern)>::new())),
                    ("rest", Pattern::site()),
                ],
            ),
        )]),
        reactum: Reactum::Structural {
            reactum: Pattern::map([(
                "compartment",
                Pattern::sort(
                    "Compartment",
                    [
                        ("substrate", Pattern::sort("pERK", Vec::<(&str, Pattern)>::new())),
                        ("rest", Pattern::site()),
                    ],
                ),
            )]),
            instantiation: IndexMap::new(),
        },
        guard: None,
        rate: None,
        bindings: RuleBindings::new(),
        closure: Arc::new(IndexMap::new()),
    }
}

fn state_with_two_erks() -> Value {
    Value::tree([(
        "cyto",
        Value::tree([
            ("_type", val_str("Compartment")),
            ("erk1", Value::tree([("_type", val_str("ERK"))])),
            ("erk2", Value::tree([("_type", val_str("ERK"))])),
        ]),
    )])
}

#[test]
fn structural_chrysalis_rule_converts_closure_free() {
    let rule = structural_chrysalis_rule();
    let prism_rule = to_structural_rule(&rule).expect("structural conversion");
    assert_eq!(prism_rule.label, "phos");
    // Round-trip identity: the redex/reactum patterns are carried through
    // unchanged — chrysalis owns the data, prism owns the firing.
    assert_eq!(prism_rule.redex, rule.redex);
}

#[test]
fn computed_chrysalis_rule_refuses_closure_free_conversion() {
    // A computed reactum can't be carried without an evaluator. The
    // converter returns None — callers must use the full `to_prism_rule`
    // with an evaluator, which is appropriate for in-engine BRS use
    // but NOT for sending the rule across a bridge as inert data.
    use chrysalis::ast::{Expr, StringLit};
    let mut rule = structural_chrysalis_rule();
    rule.reactum = Reactum::Computed(Expr::Str(StringLit::plain("ignored")));
    assert!(to_structural_rule(&rule).is_none());
    assert!(to_bigraph_value(&rule).is_none());
}

#[test]
fn chrysalis_rule_fires_through_bigraph_port() {
    // The end-to-end path: a chrysalis-built rule, wrapped as a
    // FOREIGN_REACTION-tagged Value, applied through
    // `algebra::apply_with(Custom{"bigraph"}, current, value)`. The
    // algebra dispatches to `BigraphTypeMethods::apply`, which fires
    // the reaction against the matchable state.
    let rule = structural_chrysalis_rule();
    let bigraph_value = to_bigraph_value(&rule).expect("structural rule wraps");

    let reg = TypeRegistry::new();
    let bigraph_schema = Schema::Custom {
        name: "bigraph".to_string(),
        parameters: IndexMap::new(),
    };
    let state = state_with_two_erks();

    let after = algebra::apply_with(Some(&reg), &bigraph_schema, &state, &bigraph_value);

    let cyto = after.get_path(&[Key::from("cyto")]).expect("cyto");
    let cm = cyto.as_map().expect("map");
    let p_count = ["erk1", "erk2"]
        .iter()
        .filter(|k| {
            cm.get(**k)
                .and_then(|v| v.get_field("_type"))
                .and_then(|t| t.as_str())
                == Some("pERK")
        })
        .count();
    assert_eq!(
        p_count, 1,
        "chrysalis rule should fire on the matchable bigraph slot, promoting one ERK to pERK"
    );
}

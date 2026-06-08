//! **#61b — a structural reaction is data.** `Pattern` and a closure-free
//! `ReactionRule` serialize to a plain `Value` (maps/strings/scalars, no
//! `Foreign`), so a reaction crosses a JSON/`rest:`/`stream:` boundary and is
//! reconstructed on the far side. The runnable `Foreign(FOREIGN_REACTION, …)`
//! carrier can't cross a wire; THIS data form is what does — the codec the
//! `reaction` type's `serialize`/`realize` wire onto so a schema-typed
//! `map[Reaction]` link (#61a) transports through the ordinary algebra.

use std::sync::Arc;

use prism_schema::reaction::{Pattern, ReactionRule};
use prism_schema::{Bindings, Key, Value};

/// A redex exercising every non-numeric `Pattern` variant.
fn rich_redex() -> Pattern {
    Pattern::map([
        ("_type", Pattern::Atom(Value::String("A".into()))),
        ("site", Pattern::Site),
        ("edge", Pattern::LinkVar(Key::from("e"))),
        ("gone", Pattern::Absent),
    ])
}

fn rich_reactum() -> Pattern {
    Pattern::map([
        ("_type", Pattern::Atom(Value::String("B".into()))),
        ("items", Pattern::List(vec![Pattern::Site])),
        (
            "bound",
            Pattern::Bind { name: Key::from("x"), inner: Box::new(Pattern::Site) },
        ),
    ])
}

#[test]
fn a_pattern_round_trips_through_its_data_form() {
    // Pure `to_value` ∘ `from_value` = id — covers numeric atoms too (no JSON
    // coercion in this direction, so `Int` stays `Int`).
    let cases = [
        rich_redex(),
        rich_reactum(),
        Pattern::Site,
        Pattern::Absent,
        Pattern::Atom(Value::Int(7)),
        Pattern::Atom(Value::Bool(true)),
        Pattern::List(vec![Pattern::Atom(Value::String("x".into())), Pattern::Site]),
    ];
    for p in cases {
        let back = Pattern::from_value(&p.to_value()).expect("data → pattern");
        assert_eq!(back, p, "a pattern survives to_value/from_value");
    }
}

#[test]
fn a_structural_rule_crosses_a_json_boundary() {
    let rule = ReactionRule::new(rich_redex(), rich_reactum())
        .with_label("Convert")
        .with_rate(2.5)
        .with_instantiation([("x", "site")]);

    // SERIALIZE → it is genuinely JSON-able (no `Foreign`).
    let data = rule.to_data_value().expect("a structural rule serializes");
    let json = serde_json::to_string(&data).expect("data → JSON");
    assert!(json.contains("\"Rule\""), "the rule is on the wire as data: {json}");

    // … crosses the wire … DESERIALIZE + reconstruct.
    let arrived: Value = serde_json::from_str(&json).expect("JSON → data");
    let back = ReactionRule::from_data_value(&arrived).expect("data → rule");

    assert_eq!(back.redex, rule.redex, "redex survives the wire");
    assert_eq!(back.reactum, rule.reactum, "reactum survives the wire");
    assert_eq!(back.label, "Convert");
    assert_eq!(back.rate, Some(2.5));
    assert_eq!(
        back.instantiation.get(&Key::from("x")),
        Some(&Key::from("site")),
        "the instantiation map survives"
    );
}

#[test]
fn a_computed_rule_has_no_wire_form() {
    // A closure can't cross a wire — `to_data_value` returns `None` so the
    // caller keeps the runnable form for in-process use (the honest boundary,
    // not a silent lossy serialize).
    let computed = ReactionRule::new(Pattern::Site, Pattern::Site)
        .with_reactum_fn(Arc::new(|_b: &Bindings| Value::None));
    assert!(computed.to_data_value().is_none(), "a computed reactum has no data form");

    let guarded = ReactionRule::new(Pattern::Site, Pattern::Site)
        .with_guard(Arc::new(|_b: &Bindings| true));
    assert!(guarded.to_data_value().is_none(), "a guard has no data form");

    let rated = ReactionRule::new(Pattern::Site, Pattern::Site)
        .with_rate_fn(Arc::new(|_b: &Bindings| 1.0));
    assert!(rated.to_data_value().is_none(), "a computed rate has no data form");
}

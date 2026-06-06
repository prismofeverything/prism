//! Tests for the `bigraph` type (#42) — schema kind whose `apply` is
//! **reaction-fire**. A `:: bigraph` input port accepts a reaction as a
//! typed update; the apply fires the reaction against the slot's current
//! state and returns the post-fire state. This is the schema-level
//! foundation for reaction-as-update-across-a-bridge (the conceptual
//! leap of merge-protocol slice 5 / BATWD §V).
//!
//! Together with #41 (symmetric input bridge → `apply_with_schema`), the
//! same `apply` runs whether the bigraph slot is local or piped across a
//! `stream:` / `rest:` boundary. The wire serializes only the reaction
//! Value and the schema (`bigraph`), and the receiver fires.

use prism_schema::reaction::{Pattern, ReactionRule};
use prism_schema::value::Foreign;
use prism_schema::{Key, Schema, TypeRegistry, Value, FOREIGN_REACTION};

fn val_str(s: &str) -> Value {
    Value::String(s.to_string())
}

/// Build a tiny ERK→pERK reaction (the same rule the reaction unit tests
/// use), wrapped as a `Foreign(FOREIGN_REACTION, …)` Value ready to ride
/// a `bigraph` port.
fn erk_to_perk_reaction() -> Value {
    let rule = ReactionRule::new(
        Pattern::map([(
            "compartment",
            Pattern::sort(
                "Compartment",
                [
                    ("substrate", Pattern::sort("ERK", Vec::<(&str, Pattern)>::new())),
                    ("rest", Pattern::site()),
                ],
            ),
        )]),
        Pattern::map([(
            "compartment",
            Pattern::sort(
                "Compartment",
                [
                    ("substrate", Pattern::sort("pERK", Vec::<(&str, Pattern)>::new())),
                    ("rest", Pattern::site()),
                ],
            ),
        )]),
    )
    .with_label("phos");
    Value::Foreign(Foreign::new(FOREIGN_REACTION, rule))
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
fn bigraph_apply_fires_a_reaction_update() {
    // The defining behavior: `type_apply("bigraph", state, foreign-wrapped-rule)`
    // finds matches of the redex in state and fires the reactum.
    let reg = TypeRegistry::new();
    let state = state_with_two_erks();
    let reaction = erk_to_perk_reaction();

    let after = reg.type_apply("bigraph", &state, &reaction);

    // Exactly one of the two ERKs has been phosphorylated.
    let cyto = after.get_path(&[Key::from("cyto")]).expect("cyto present");
    let cm = cyto.as_map().expect("cyto is a map");
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
        "exactly one ERK should have been promoted to pERK by the bigraph apply"
    );
}

#[test]
fn bigraph_apply_with_no_match_is_a_no_op() {
    // A reaction whose redex doesn't match the state should leave state
    // unchanged — an "update should not silently lose information." (The
    // contrasting Overwrite path would replace state with the foreign
    // value, which is wrong for a non-firing reaction.)
    let reg = TypeRegistry::new();
    let state = Value::tree([(
        "cyto",
        Value::tree([
            ("_type", val_str("Compartment")),
            // No ERK substrate present.
            ("nothing", Value::tree([("_type", val_str("Other"))])),
        ]),
    )]);
    let reaction = erk_to_perk_reaction();

    let after = reg.type_apply("bigraph", &state, &reaction);
    assert_eq!(after, state, "no-match should be a no-op");
}

#[test]
fn bigraph_apply_with_plain_update_is_overwrite() {
    // A plain (non-Foreign) update is interpreted as Overwrite — the
    // sub-bigraph is replaced wholesale. This is the same role
    // `overwrite[T]` plays for ordinary types: the schema's apply lets a
    // caller take the "replace it all" semantics where it wants.
    let reg = TypeRegistry::new();
    let state = state_with_two_erks();
    let replacement = Value::tree([(
        "cyto",
        Value::tree([
            ("_type", val_str("Compartment")),
            ("erk1", Value::tree([("_type", val_str("pERK"))])),
        ]),
    )]);

    let after = reg.type_apply("bigraph", &state, &replacement);
    assert_eq!(after, replacement, "plain update should overwrite");
}

#[test]
fn bigraph_apply_routes_through_algebra_via_custom_schema() {
    // The whole point of registering `bigraph` as a Custom type: ANY
    // schema-driven apply that encounters `Schema::Custom { name:
    // "bigraph" }` dispatches to BigraphTypeMethods. So a composite slot
    // typed `:: bigraph` automatically gets reaction-fire semantics when
    // the bridge runs `algebra::apply_with(Custom{bigraph}, …)` (#41).
    use prism_schema::algebra;
    let reg = TypeRegistry::new();
    let bigraph_schema = Schema::Custom {
        name: "bigraph".to_string(),
        parameters: Default::default(),
    };
    let state = state_with_two_erks();
    let reaction = erk_to_perk_reaction();

    let after = algebra::apply_with(Some(&reg), &bigraph_schema, &state, &reaction);
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
        "algebra::apply_with(Custom{{bigraph}}, …) should fire the reaction"
    );
}

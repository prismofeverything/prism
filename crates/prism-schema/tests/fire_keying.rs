//! Conformance: the firing KEYING convention is uniform across reactum shapes.
//!
//! `fire_rule_at` turns (match, reactum) into a localized `{_remove, _add}`
//! delta. A Map reactum keys its products by name; a LIST reactum (a multiset
//! reaction `a | b => c | d`) must key its products FRESHLY and consume the
//! matched reactants — the SAME convention the chrysalis computed path
//! (`reaction_delta`) uses, so structural and computed list reactions agree.
//! Before the fix a structural List reactum returned `None` (a silent no-op).

use prism_schema::reaction::{apply_fire, find_matches, fire_rule_at, Pattern, ReactionRule};
use prism_schema::{Key, Value};

fn sort(label: &str) -> Pattern {
    Pattern::sort(label, Vec::<(&str, Pattern)>::new())
}
fn node(t: &str) -> Value {
    Value::tree([("_type", Value::String(t.to_string()))])
}
fn types(v: &Value) -> Vec<String> {
    v.as_map()
        .map(|m| {
            m.values()
                .filter_map(|x| x.get_field("_type").and_then(|t| t.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn structural_list_reactum_fires_a_multiset_reaction() {
    // `F | B => G | H`: consume F and B, produce G and H (fresh keys). A
    // structural List reactum must FIRE, like its computed counterpart.
    let rule = ReactionRule::new(
        Pattern::list([sort("F"), sort("B")]),
        Pattern::list([sort("G"), sort("H")]),
    )
    .with_label("react");
    let state = Value::tree([("x", node("F")), ("y", node("B"))]);

    let matches = find_matches(&state, &rule.redex, None);
    assert!(!matches.is_empty(), "F|B matches the soup");
    let upd = fire_rule_at(&rule, &matches[0])
        .expect("a structural List reactum must fire (not silently no-op)");
    let after = apply_fire(&state, &upd, None);

    let ts = types(&after);
    assert!(
        ts.contains(&"G".to_string()) && ts.contains(&"H".to_string()),
        "products G and H were keyed in: {ts:?}"
    );
    assert!(
        !ts.contains(&"F".to_string()) && !ts.contains(&"B".to_string()),
        "reactants F and B were consumed: {ts:?}"
    );
}

#[test]
fn keyed_by_binder_reactum_modifies_coupled_nodes_in_place() {
    // `?west | ?east => { ?west: <west'>, ?east: <east'> }` — a reactum KEYED BY
    // THE BINDERS replaces each matched node AT ITS OWN KEY (an IN-PLACE modify of
    // the coupled pair), NOT under fresh keys. Enabled by the matcher recording
    // `key_map[?west] = <matched child key>`, so the structural `remap_keys`
    // renames the reactum's `?west` key to the matched entry. This is the firing
    // half of the cross-composite coupling (`?west … | ?east …`) — the composites
    // are modified in place, not consumed+reproduced under fresh keys.
    let redex = Pattern::list([
        Pattern::Bind { name: Key::from("?west"), inner: Box::new(sort("Cell")) },
        Pattern::Bind { name: Key::from("?east"), inner: Box::new(sort("Cell")) },
    ]);
    let bonded = Pattern::sort("Cell", [("bonded", Pattern::Atom(Value::Bool(true)))]);
    let reactum = Pattern::map([("?west", bonded.clone()), ("?east", bonded)]);
    let rule = ReactionRule::new(redex, reactum);
    let state = Value::tree([("a", node("Cell")), ("b", node("Cell"))]);

    let matches = find_matches(&state, &rule.redex, None);
    assert!(!matches.is_empty(), "?west|?east matches the two cells");
    let upd = fire_rule_at(&rule, &matches[0]).expect("fires");
    let after = apply_fire(&state, &upd, None);
    let m = after.as_map().expect("map");

    // SAME keys a, b (in place — not fresh g0/g1), both now bonded.
    let keys: Vec<&str> = m.keys().map(|k| k.as_str()).collect();
    assert!(
        m.contains_key("a") && m.contains_key("b"),
        "matched entries modified in place at their OWN keys: {keys:?}"
    );
    assert_eq!(
        m.get("a").and_then(|v| v.get_field("bonded")).and_then(|v| v.as_bool()),
        Some(true),
        "west modified in place"
    );
    assert_eq!(
        m.get("b").and_then(|v| v.get_field("bonded")).and_then(|v| v.as_bool()),
        Some(true),
        "east modified in place"
    );
}

#[test]
fn structural_single_term_list_reactum_fires() {
    // Degenerate multiset: `F | B => C` (two reactants fuse into one product).
    let rule = ReactionRule::new(
        Pattern::list([sort("F"), sort("B")]),
        Pattern::list([sort("C")]),
    );
    let state = Value::tree([("x", node("F")), ("y", node("B"))]);
    let matches = find_matches(&state, &rule.redex, None);
    let upd = fire_rule_at(&rule, &matches[0]).expect("must fire");
    let after = apply_fire(&state, &upd, None);
    let ts = types(&after);
    assert_eq!(ts, vec!["C".to_string()], "F,B fused into a single C: {ts:?}");
}

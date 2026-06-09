//! **Stage 4c — the BRS rule boundary EVALS TRANSPARENT REACTION-DATA.**
//!
//! `rules_as_state.rs` proved a reaction installed as the *runnable* carrier
//! `Foreign(FOREIGN_REACTION, ReactionRule)` becomes an active rule. This proves
//! the SYMMETRIC, homoiconic form: a reaction installed as **pure transparent
//! data** — the `{_pat: "Rule", redex, reactum, …}` map that
//! `ReactionRule::to_data_value` emits, with NO `Foreign` wrapper — ALSO becomes
//! active and fires. The BRS evals the data into a runnable rule exactly as
//! `Engine::discover_processes` evals a transparent process spec into a running
//! node (`docs/homoiconic-unification.md` Stage 4c: the rule boundary joins the
//! node boundary in reading DATA).
//!
//! Why it matters: a constructor (`build_reaction_value`) can now emit DATA
//! uniformly with every node constructor — dissolving the FLAT/RICH seam — and
//! the chrysalis `ReactionType::realize` re-wrap-for-the-BRS (`{_pat:"Rule"}` →
//! `Foreign`, present *only* because the BRS used to read Foreign alone) can
//! retire. The eval is the EXISTING schema-algebra codec `from_data_value`; this
//! test is its consumer at the BRS boundary.

use prism_bigraph::process::Process;
use prism_bigraph::{BigraphicalReactiveSystem, Update};
use prism_schema::reaction::{Pattern, ReactionRule};
use prism_schema::{Key, Schema, StateMap, Value, algebra};

/// A bare control ion `{_type: t}`.
fn ion(t: &str) -> Value {
    Value::tree([("_type", Value::String(t.into()))])
}

/// A STRUCTURAL rule (closure-free → it HAS a `to_data_value` form): an `ERK`
/// in a `Compartment` becomes `pERK`. The proven `phos` shape from brs.rs's own
/// `deterministic_one_firing_per_tick` test — a redex/reactum pattern pair, no
/// `reactum_fn`, so `to_data_value` yields a transparent wire form.
fn phos_rule() -> ReactionRule {
    let no_fields = Vec::<(&str, Pattern)>::new();
    ReactionRule::new(
        Pattern::map([(
            "comp",
            Pattern::sort(
                "Compartment",
                [
                    ("substrate", Pattern::sort("ERK", no_fields.clone())),
                    ("rest", Pattern::site()),
                ],
            ),
        )]),
        Pattern::map([(
            "comp",
            Pattern::sort(
                "Compartment",
                [
                    ("substrate", Pattern::sort("pERK", no_fields)),
                    ("rest", Pattern::site()),
                ],
            ),
        )]),
    )
    .with_label("phos")
}

/// The transparent DATA form of `phos_rule` — `{_pat: "Rule", …}`, plain
/// JSON-able state, NOT a `Foreign`. This is what a constructor emitting data,
/// or a reaction arriving over a `rest:`/`stream:` bridge as JSON, looks like.
fn phos_data() -> Value {
    let data = phos_rule().to_data_value().expect("a structural rule is data");
    // Guard the premise: it really is transparent (a plain map, no Foreign).
    let m = data.as_map().expect("the data form is a plain map");
    assert_eq!(
        m.get("_pat").and_then(|t| t.as_str()),
        Some("Rule"),
        "the transparent carrier is `_pat: \"Rule\"`, not a runnable Foreign"
    );
    data
}

/// A compartment holding two ERKs (and a MEK that never matches).
fn cell() -> Value {
    Value::tree([(
        "cyto",
        Value::tree([
            ("_type", Value::String("Compartment".into())),
            ("mek", ion("MEK")),
            ("erk1", ion("ERK")),
            ("erk2", ion("ERK")),
        ]),
    )])
}

fn count_perk(subtree: &Value) -> usize {
    let cyto = subtree.get_field("cyto").and_then(|v| v.as_map()).expect("cyto");
    ["erk1", "erk2"]
        .iter()
        .filter(|k| {
            cyto.get(**k)
                .and_then(|v| v.get_field("_type"))
                .and_then(|t| t.as_str())
                == Some("pERK")
        })
        .count()
}

/// Wrap a subtree (and optionally a `rules` pool) onto the BRS input ports, run
/// `ticks` updates, applying each emitted `state` delta back through the REAL
/// schema-algebra apply. Returns the final subtree. (Mirrors the engine's apply
/// step, as `rules_as_state.rs` does.)
fn drive(
    brs: &BigraphicalReactiveSystem,
    mut subtree: Value,
    rules_port: Option<Value>,
    ticks: usize,
) -> Value {
    for _ in 0..ticks {
        let mut input = StateMap::new();
        input.insert(Key::from("state"), subtree.clone());
        if let Some(r) = &rules_port {
            input.insert(Key::from("rules"), r.clone());
        }
        if let Update::Value(out) = brs.update(&Value::Map(input), 1.0) {
            if let Some(delta) = out.get_field("state") {
                subtree = algebra::apply_with(None, &Schema::Any, &subtree, delta);
            }
        }
    }
    subtree
}

#[test]
fn transparent_reaction_data_under_rules_fires() {
    // The rules-in-soup form: a transparent `{_pat:"Rule"}` value lives under
    // `_rules` of the very subtree it rewrites. NO seed rules, NO Foreign — the
    // BRS must EVAL the data to make it active.
    let brs = BigraphicalReactiveSystem::new(vec![]);
    let subtree = {
        let mut m = cell().as_map().expect("cell map").clone();
        m.insert(Key::from("_rules"), Value::tree([("phos", phos_data())]));
        Value::Map(m)
    };
    assert_eq!(count_perk(&subtree), 0, "no pERK before firing");

    let after = drive(&brs, subtree, None, 4);
    assert_eq!(
        count_perk(&after),
        2,
        "the BRS evaled the TRANSPARENT reaction-data into an active rule and \
         fired it (both ERK→pERK) — no Foreign wrapper needed: {after:?}"
    );
    // The data persists in state (re-evaled each tick, like a Foreign rule).
    assert!(
        after.get_field("_rules").is_some(),
        "the rule-data stays in state across ticks"
    );
}

#[test]
fn transparent_reaction_data_via_rules_pool_fires() {
    // The pool / link form: the transparent rule arrives on the `rules` input
    // port (a shared `map[Reaction]` link) — the shape a reaction has after
    // crossing a `rest:`/`stream:` bridge as JSON. The subtree itself carries no
    // rules, so a firing proves the POOL value was evaled.
    let brs = BigraphicalReactiveSystem::new(vec![]);
    let pool = Value::tree([("phos", phos_data())]);
    let after = drive(&brs, cell(), Some(pool), 4);
    assert_eq!(
        count_perk(&after),
        2,
        "a transparent reaction in the rules POOL is evaled + fires: {after:?}"
    );
}

#[test]
fn molecules_are_not_mistaken_for_rules() {
    // Negative control: the SAME cell, NO rule data anywhere. The molecular soup
    // (Compartment / ERK / MEK maps) must NOT be mis-evaled as rules
    // (`from_data_value` requires `_pat:"Rule"`; a `_type`-tagged molecule
    // returns `Err`), so nothing fires.
    let brs = BigraphicalReactiveSystem::new(vec![]);
    let after = drive(&brs, cell(), None, 4);
    assert_eq!(count_perk(&after), 0, "no rule data ⇒ no firing: {after:?}");
}

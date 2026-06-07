//! **The load-bearing consumer for threading the full Core into reactions.**
//!
//! A *reflective* reaction: its reactum introspects the ONE shared `Core` — the
//! same one the engine and every node hold (the Core-threading rule) — via the
//! `core_processes()` builtin, which lists the process CLASSES the runtime can
//! instantiate. The reactum eval reaches the Core through the evaluator's shared
//! late-bound handle (`compile` sets it on the same handle the engine uses), so a
//! reaction is informed by the live Core, not merely the program AST it compiled
//! from.
//!
//! The proof is that the manifest includes `Composite` and `Brs` — classes
//! registered into the Core by `compile` (the generic composite factory + the
//! BRS), which are NOT `def`s in the program. The reactum could only have learned
//! them from the live shared Core. Pair this with `prism-bigraph`'s
//! `reaction_creates_process` (a reactum that CREATES a process the engine then
//! instantiates) and reactions are end-to-end Core-aware: they can introspect the
//! Core and emit specs it builds.

use prism_bigraph::Engine;
use prism_schema::Value;

/// Collect every `kinds` string-list found anywhere under `v` (the record
/// reactum splices its fields into the match's container, so the manifest may
/// land at the `items` root or under a fresh key — either way we find it).
fn collect_kinds(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(m) = v.as_map() {
        if let Some(list) = m.get("kinds").and_then(|k| k.as_list()) {
            out.extend(list.iter().filter_map(|x| x.as_str().map(String::from)));
        }
        for vv in m.values() {
            out.extend(collect_kinds(vv));
        }
    }
    out
}

/// Compile + run an inline `.ys` program and return the final root state.
fn run(src: &str, time: f64) -> Value {
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let result = chrysalis::compile::compile(&program).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(time);
    engine.state().clone()
}

/// `Manifest` fires on a `Seed` ion and replaces it with a record of what the
/// shared Core can build — `core_processes()` evaluated *in the reactum*.
const REFLECT: &str = r#"
reaction Manifest (
  ?s :: Seed => { kinds: core_processes() }
)

composite World ->{items :: map[any] @ items} (
  items: { seed: Seed[] } |
  rxn: BRS[rules: [Manifest]] ~{state: items} ->{state: items}
)

World[]
"#;

#[test]
fn reactum_introspects_the_shared_core() {
    let s = run(REFLECT, 2.0);
    let items = s.get_field("items").expect("items");

    // The reaction fired: the Seed was consumed.
    assert!(
        items.get_field("seed").is_none(),
        "the reaction consumed the seed: {items:?}"
    );

    // The reactum recorded `core_processes()` — the live Core's class list.
    let kinds = collect_kinds(items);
    assert!(
        !kinds.is_empty(),
        "the reactum evaluated core_processes() and recorded the result: {items:?}"
    );

    // `Composite` and `Brs` are registered into the Core by `compile` (the generic
    // composite factory + the BRS) — they are NOT `def`s in this program. So the
    // reactum could only have read them from the LIVE shared Core, not the AST.
    assert!(
        kinds.contains(&"Composite".to_string()),
        "Core-registered (non-program) class `Composite` present — proves Core access: {kinds:?}"
    );
    assert!(
        kinds.contains(&"Brs".to_string()),
        "Core-registered (non-program) class `Brs` present — proves Core access: {kinds:?}"
    );
}

//! AlChemy with the rules pool as a SEPARATE wired slot (the BRS `rules` port) —
//! the step between rules-in-soup (alchemy.rs) and a shared `link` (milestone 5).
//! `Bootstrap` installs `Grow` into the `pool` slot via the BRS's `rules` output;
//! the BRS reads its active ruleset from `pool` (the `rules` input) AND its seed;
//! `Grow` then fires on the seed. Proves the `rules` port round-trips before we
//! make `pool` a link shared across reactors.

use prism_bigraph::Engine;
use prism_schema::Value;

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

fn collect_types(v: &Value, out: &mut Vec<String>) {
    if let Some(m) = v.as_map() {
        if let Some(t) = m.get("_type").and_then(|t| t.as_str()) {
            out.push(t.to_string());
        }
        for vv in m.values() {
            collect_types(vv, out);
        }
    }
}

const POOL: &str = r#"
reaction Grow ( ?s :: Seed => Sprout )

reaction Bootstrap (
  ?t :: Trigger
  =>
  { '_add': {}, '_rules': { 'grow': compile_reaction(Grow) } }
)

composite Lab ->{ soup :: any @ soup, pool :: map[Reaction] @ pool } (
  soup: { trigger: Trigger[], seed: Seed[] } |
  pool: {} |
  rxn: BRS[rules: [Bootstrap]] ~{state: soup, rules: pool} ->{state: soup, rules: pool}
)

Lab[]
"#;

#[test]
fn bootstrap_installs_grow_into_the_pool_and_it_fires() {
    let s = run(POOL, 5.0);
    let soup = s.get_field("soup").expect("soup slot");
    let mut kinds = Vec::new();
    collect_types(soup, &mut kinds);

    assert!(
        kinds.iter().any(|t| t == "Sprout"),
        "Grow (installed into the pool by Bootstrap) fired on the seed: {soup:?}"
    );
    assert!(
        !kinds.iter().any(|t| t == "Seed"),
        "the seed was consumed: {soup:?}"
    );

    // The pool holds the installed reaction (a transmittable reaction value).
    let pool = s.get_field("pool").and_then(|v| v.as_map());
    assert!(
        pool.is_some_and(|p| p.contains_key("grow")),
        "the pool holds the installed `grow` reaction: {:?}",
        s.get_field("pool")
    );
}

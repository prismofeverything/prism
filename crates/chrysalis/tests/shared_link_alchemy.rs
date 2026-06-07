//! **Shared-link AlChemy (#61, milestone 5).** The rules pool is a first-class
//! `link reactions :: map[Reaction]` — a value-bearing hyperedge. TWO reactor
//! regions share it: a `producer` (with a `Trigger`) and a `consumer` (with a
//! `Seed`), each driven by its own BRS attached to the SAME link via
//! `~{rules: ~reactions}`.
//!
//! The producer's `Bootstrap` fires, reifies `Grow`, and routes it onto the
//! shared link (the BRS `rules` output). The consumer's BRS — which has NO seed
//! rules of its own — reads `Grow` off the link on the next tick and fires it on
//! its `Seed`. **A reaction born in one region fires in another, carried by the
//! link graph.** This is the distribution fabric: the same primitive that shares
//! glucose shares reactions.

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

const SHARED: &str = r#"
reaction Grow ( ?s :: Seed => Sprout )

reaction Bootstrap (
  ?t :: Trigger
  =>
  { '_add': {}, '_rules': { 'grow': compile_reaction(Grow) } }
)

composite Lab ->{ producer :: any @ producer, consumer :: any @ consumer } (
  link reactions :: map[Reaction] = {} |
  producer: { trigger: Trigger[] } |
  consumer: { seed: Seed[] } |
  prod: BRS[rules: [Bootstrap]] ~{state: producer, rules: ~reactions} ->{state: producer, rules: ~reactions} |
  cons: BRS[] ~{state: consumer, rules: ~reactions} ->{state: consumer, rules: ~reactions}
)

Lab[]
"#;

#[test]
fn a_reaction_born_in_one_region_fires_in_another_via_the_link() {
    let s = run(SHARED, 6.0);

    // The consumer — which had NO seed rules — produced a Sprout, because `Grow`
    // (created by the producer's Bootstrap) reached it across the shared link.
    let consumer = s.get_field("consumer").expect("consumer region");
    let mut cons_kinds = Vec::new();
    collect_types(consumer, &mut cons_kinds);
    assert!(
        cons_kinds.iter().any(|t| t == "Sprout"),
        "the consumer fired a link-shared reaction it never declared: {consumer:?}"
    );
    assert!(
        !cons_kinds.iter().any(|t| t == "Seed"),
        "the consumer's seed was consumed by the shared Grow: {consumer:?}"
    );

    // The producer consumed its trigger (it installed Grow, didn't sprout).
    let producer = s.get_field("producer").expect("producer region");
    let mut prod_kinds = Vec::new();
    collect_types(producer, &mut prod_kinds);
    assert!(
        !prod_kinds.iter().any(|t| t == "Trigger"),
        "the producer consumed its trigger: {producer:?}"
    );
}

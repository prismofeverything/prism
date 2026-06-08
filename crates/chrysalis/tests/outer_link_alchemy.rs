//! **Outer-link AlChemy (#61, milestone 5b → 6).** The producer and consumer are
//! now SEPARATE COMPOSITES (sealed subengines), each referencing `~reactions` —
//! a link declared in their PARENT (`Lab`), not locally. That is an OUTER link:
//! the composite boundary auto-bridges it (a same-named port wired to the
//! parent's `~reactions`, plus a local mirror), so the sealed reactors share one
//! reaction pool through their bridges — encapsulation intact.
//!
//! `Producer.Bootstrap` reifies `Grow` and routes it onto the shared link;
//! `Consumer` — a different sealed composite with NO rules of its own — reads
//! `Grow` off the link and fires it on its `Seed`. A reaction born in one
//! composite fires in another, carried across the boundary by the link graph.
//! This is the distribution fabric (the next step is the link spanning a
//! stream/rest bridge — the same value, a different transport).

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

const OUTER: &str = r#"
reaction Grow ( ?s :: Seed => Sprout )

reaction Bootstrap (
  ?t :: Trigger
  =>
  { '_add': {}, '_rules': { 'grow': compile_reaction(Grow) } }
)

composite Producer ->{ soup :: any @ soup } (
  soup: { trigger: Trigger[] } |
  rxn: BRS[rules: [Bootstrap]] ~{state: soup, rules: ~reactions} ->{state: soup, rules: ~reactions}
)

composite Consumer ->{ soup :: any @ soup } (
  soup: { seed: Seed[] } |
  rxn: BRS[] ~{state: soup, rules: ~reactions} ->{state: soup, rules: ~reactions}
)

composite Lab ->{ producer_out :: any @ producer_out, consumer_out :: any @ consumer_out } (
  link reactions :: map[Reaction] = {} |
  producer_out: {} |
  consumer_out: {} |
  producer: Producer[] ~{} ->{soup: producer_out} |
  consumer: Consumer[] ~{} ->{soup: consumer_out}
)

Lab[]
"#;

#[test]
fn a_reaction_born_in_one_composite_fires_in_another_via_an_outer_link() {
    let s = run(OUTER, 15.0);

    // The consumer composite — a different sealed subengine with NO rules of its
    // own — produced a Sprout, because `Grow` (created in the producer composite)
    // crossed the boundary on the shared outer link. We observe its LIVE soup via
    // the `consumer_out` slot its `soup` output bridges to.
    let consumer_out = s.get_field("consumer_out").expect("consumer_out slot");
    let mut cons_kinds = Vec::new();
    collect_types(consumer_out, &mut cons_kinds);
    assert!(
        cons_kinds.iter().any(|t| t == "Sprout"),
        "the consumer fired a reaction created in another composite (outer link): {consumer_out:?}"
    );
}

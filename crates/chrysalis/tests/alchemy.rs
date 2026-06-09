//! **Local AlChemy (#61) — a `.ys` reaction installs a reaction that fires.**
//!
//! `Bootstrap` consumes a `Trigger` and installs `Grow` into the soup's `_rules`
//! (rules-as-state) via `compile_reaction(Grow)` — the reify that turns a reaction
//! reference into the runnable, transmittable form. `Grow` exists nowhere as a
//! seed rule; only after `Bootstrap` runs does it become active and convert a
//! `Seed` into a `Sprout`. A `Sprout` appearing is proof that a reaction created
//! a reaction that then fired — the reaction loop, at the chrysalis surface.

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

const ALCHEMY: &str = r#"
reaction Grow ( ?s :: Seed => Sprout )

reaction Bootstrap (
  ?t :: Trigger
  =>
  { '_add': {}, '_rules': { 'grow': compile_reaction(Grow) } }
)

composite Lab ->{ brew :: any @ brew } (
  brew: {
    trigger: Trigger[],
    seed: Seed[],
    _rules: {}
  } |
  rxn: BRS[rules: [Bootstrap]] ~{state: brew} ->{state: brew}
)

Lab[]
"#;

#[test]
fn a_ys_reaction_installs_a_reaction_that_fires() {
    let s = run(ALCHEMY, 5.0);
    let brew = s.get_field("brew").expect("brew slot");
    let mut kinds = Vec::new();
    collect_types(brew, &mut kinds);

    // Grow — a reaction CREATED by Bootstrap — fired and converted Seed→Sprout.
    assert!(
        kinds.iter().any(|t| t == "Sprout"),
        "Grow (installed by a reaction) fired: {brew:?}"
    );
    assert!(
        !kinds.iter().any(|t| t == "Trigger"),
        "Bootstrap consumed the trigger: {brew:?}"
    );
    assert!(
        !kinds.iter().any(|t| t == "Seed"),
        "the installed Grow consumed the seed: {brew:?}"
    );
}

const DEF_REACTION_SUGAR: &str = r#"
def Grow = Reaction[redex: ?s :: Seed, reactum: Sprout]

composite Lab ->{ brew :: any @ brew } (
  brew: { seed: Seed[] } |
  rxn: BRS[rules: [Grow]] ~{state: brew} ->{state: brew}
)

Lab[]
"#;

#[test]
fn def_bound_reaction_constructor_is_a_drop_in_for_the_definer() {
    // Stage 2b — the definer-as-sugar drop-in: `def Grow = Reaction[redex: …,
    // reactum: …]` is INTERCHANGEABLE with `reaction Grow ( … => … )`. The
    // capitalized constructor evaluates to a reaction value, and `brs_rules`
    // accepts it directly in a `BRS[rules: […]]` list (both the `FOREIGN_RULE` a
    // definer produces AND the `FOREIGN_REACTION` the constructor produces). No
    // `reaction` keyword, no `compile_reaction` — just `def NAME = Reaction[…]`,
    // and it fires Seed→Sprout. The "constructor IS the definer" made real.
    let s = run(DEF_REACTION_SUGAR, 3.0);
    let brew = s.get_field("brew").expect("brew slot");
    let mut kinds = Vec::new();
    collect_types(brew, &mut kinds);
    assert!(
        kinds.iter().any(|t| t == "Sprout"),
        "the def-bound Reaction[…] fired Seed→Sprout: {brew:?}"
    );
    assert!(
        !kinds.iter().any(|t| t == "Seed"),
        "the seed was consumed: {brew:?}"
    );
}

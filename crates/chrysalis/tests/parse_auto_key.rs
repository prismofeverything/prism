//! Bare subprocess auto-keying: an unkeyed process term in a composite body
//! (`Bump ~{…}`) is auto-keyed by its control name (`bump: Bump ~{…}`) so it's
//! registered and RUNS — instead of silently never instantiating.

use std::sync::Arc;

use chrysalis::ast::{Def, Expr};
use chrysalis::compile::compile;
use chrysalis::parse::parse_program;
use prism_bigraph::Engine;

const BARE_YS: &str = r#"
process Bump ~{v: float} ->{v: float} (
  { v: 1.0 }
)

composite Leaf[v: float = 0.0] ->{v} (
  v: v |
  Bump ~{v: v} ->{v: v}
)

Leaf
"#;

#[test]
fn bare_subprocess_is_auto_keyed_and_runs() {
    let program = parse_program(BARE_YS).expect("parse bare subprocess");

    // The bare `Bump` term was auto-keyed to `bump` in Leaf's body.
    let leaf = program
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Composite(c) if c.name == "Leaf" => Some(c),
            _ => None,
        })
        .expect("Leaf composite");
    let keys: Vec<String> = match &leaf.body {
        Expr::Parallel(items) => items
            .iter()
            .filter_map(|e| match e {
                Expr::KeyedEntry { key, .. } => key.as_plain(),
                _ => None,
            })
            .collect(),
        _ => vec![],
    };
    assert!(
        keys.contains(&"bump".to_string()),
        "bare `Bump` auto-keyed to `bump`, got {keys:?}"
    );
    assert!(
        !matches!(&leaf.body, Expr::Parallel(items) if items.iter().any(|e| matches!(e, Expr::Term { .. }))),
        "no bare (unkeyed) terms remain in the body"
    );

    // And it RUNS: the auto-keyed Bump grows v (a bare term would not have).
    let result = compile(&program).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(3.0);
    let v = engine
        .state()
        .get_field("v")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    assert!(v > 0.0, "the auto-keyed Bump ran and grew v (got {v})");
}

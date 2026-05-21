//! The `.ys` parser, M3 cross-file: `parse_file` resolves `import Dish from
//! "dish.ys"` — loading + merging another file's defs — then the composite is
//! compiled and run. This is the composite-process use case (#6) as TWO real
//! `.ys` files: `Dish` defined in `dish.ys`, imported and nested by
//! `culture.ys`, simulated.

use std::path::PathBuf;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{Engine, ProcessNode, ProcessRegistry};
use prism_schema::Value;

use chrysalis::ast::Def;
use chrysalis::compile::compile_with_registry;
use chrysalis::parse::parse_file;
use spatio_flux::processes::diffusion_advection::DiffusionAdvection;

fn culture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys/culture.ys")
}

fn diffusion_natives() -> ProcessRegistry {
    let mut reg = ProcessRegistry::new();
    reg.register("Diffusion", |_cfg| {
        ProcessNode::Process(Box::new(DiffusionAdvection::new(
            (3, 3),
            (3.0, 3.0),
            IndexMap::from([("glucose".into(), 1e-1), ("acetate".into(), 1e-1)]),
            1.0,
        )))
    });
    reg
}

fn slot_glucose(state: &Value, slot: &str) -> Vec<f64> {
    state
        .get_field(slot)
        .and_then(|f| f.get_field("glucose"))
        .and_then(|v| v.as_list())
        .map(|l| l.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default()
}

#[test]
fn import_resolves_and_runs_across_files() {
    // parse_file loads culture.ys AND resolves `import Dish from "dish.ys"`.
    let program = parse_file(culture_path()).expect("parse + resolve imports");

    // The merged program has Dish (imported), its Diffusion extern, and Culture.
    let has = |n: &str| program.defs.iter().any(|d| chrysalis::ast::def_name(d) == n);
    assert!(has("Dish"), "Dish imported from dish.ys");
    assert!(has("Diffusion"), "Dish's Diffusion extern came along");
    assert!(has("Culture"), "Culture is local");
    assert!(!program.defs.iter().any(|d| matches!(d, Def::Import { .. })), "imports resolved away");

    let result = compile_with_registry(&program, diffusion_natives()).expect("compile merged program");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(30.0);

    // Each imported+nested Dish ran its diffusion; the field changed and
    // conserved mass (sum 45) through the bridge — same as the AST-fixture
    // composite-process test, but from two real `.ys` files.
    let state = engine.state();
    let after_a = slot_glucose(state, "fields_a");
    let after_b = slot_glucose(state, "fields_b");
    let sum = |v: &[f64]| v.iter().sum::<f64>();
    assert_eq!(after_a.len(), 9, "well_a field observed via the bridge");
    assert_eq!(after_b.len(), 9, "well_b field observed via the bridge");
    assert!(after_a != vec![0.0, 0.0, 0.0, 5.0, 5.0, 5.0, 10.0, 10.0, 10.0], "well_a diffused");
    assert!((sum(&after_a) - 45.0).abs() < 1e-6, "well_a conserves glucose (got {})", sum(&after_a));
    assert!((sum(&after_b) - 45.0).abs() < 1e-6, "well_b conserves glucose (got {})", sum(&after_b));
}

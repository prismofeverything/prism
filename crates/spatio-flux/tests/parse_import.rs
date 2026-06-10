//! The `.ys` parser, M3 cross-file: `parse_file` resolves `from .dish import
//! Dish` — a RELATIVE-file import (leading dot) loading the sibling file and
//! merging the named def (+ its transitive deps + vocabulary) — then the
//! composite is compiled and run. This is the composite-process use case (#6) as
//! TWO real `.ys` files: `Dish` defined in `dish.ys`, imported and nested by
//! `culture.ys`, simulated. `Dish` wires the native `DiffusionAdvection`,
//! imported via a BARE `from diffusion import …` (a native/registry import — the
//! `extern` replacement; explicit-origin keeps it distinct from the `.dish` file
//! without any precedence rule).

use std::path::PathBuf;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{Core, Engine, ProcessNode, ProcessRegistry};
use prism_schema::Value;

use chrysalis::ast::Def;
use chrysalis::compile::compile_with_core;
use chrysalis::parse::parse_file;
use spatio_flux::prelude::sf_modules;
use spatio_flux::processes::diffusion_advection::DiffusionAdvection;

fn culture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys/culture.ys")
}

/// The native factory `dish.ys`'s `from diffusion import DiffusionAdvection`
/// resolves to — a 3×3 grid matching the field schema. (The module export is
/// declared in [`sf_modules`]; the factory implementation is supplied here, as
/// any host supplies its natives.)
fn diffusion_natives() -> ProcessRegistry {
    let mut reg = ProcessRegistry::new();
    reg.register("DiffusionAdvection", |_cfg| {
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
    // `parse_file` resolves `from .dish import Dish` (the leading dot ⇒ the
    // sibling file) by loading dish.ys and merging the named def. dish.ys's own
    // BARE `from diffusion import …` is a native/registry import — left for
    // compile, never confused with a `diffusion.ys` sibling, because the name's
    // shape (bare vs dotted) decides the origin. No `module_names`, no precedence.
    let program = parse_file(culture_path()).expect("parse + resolve imports");

    // The merged program has Dish (imported), its `from diffusion import …`
    // native import (came along with Dish), and Culture.
    let has = |n: &str| {
        program
            .defs
            .iter()
            .any(|d| chrysalis::ast::def_name(d) == n)
    };
    assert!(has("Dish"), "Dish imported from dish.ys");
    assert!(has("Culture"), "Culture is local");
    assert!(
        program.defs.iter().any(|d| matches!(d, Def::Use { .. })),
        "Dish's native import (`from diffusion import DiffusionAdvection`) came along"
    );
    assert!(
        !program
            .defs
            .iter()
            .any(|d| matches!(d, Def::Use { module, .. } if module == ".dish")),
        "the `from .dish import Dish` file-module is resolved away (only bare native `use`s remain)"
    );
    assert!(
        program.defs.iter().any(|d| matches!(d, Def::Use { module, .. } if module == "diffusion")),
        "the BARE `from diffusion import …` stays an unresolved native Use (explicit-origin: bare ⇒ registry)"
    );

    // Compile through the spatio-flux native modules (the `extern` replacement):
    // `from diffusion import DiffusionAdvection` resolves to the native factory.
    let result = compile_with_core(
        &program,
        Core::from(Arc::new(diffusion_natives())),
        sf_modules(),
    )
    .expect("compile merged program");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
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
    assert!(
        after_a != vec![0.0, 0.0, 0.0, 5.0, 5.0, 5.0, 10.0, 10.0, 10.0],
        "well_a diffused"
    );
    assert!(
        (sum(&after_a) - 45.0).abs() < 1e-6,
        "well_a conserves glucose (got {})",
        sum(&after_a)
    );
    assert!(
        (sum(&after_b) - 45.0).abs() < 1e-6,
        "well_b conserves glucose (got {})",
        sum(&after_b)
    );
}

//! Sanity-port of representative spatio-flux examples to chrysalis.
//!
//! These reproduce spatio-flux `examples.rs` documents as **chrysalis
//! programs** that *compose native processes*: an `extern process`
//! declaration gives chrysalis the interface, the implementation stays the
//! native Rust process (registered via `compile_with_registry`). The point
//! is to flush out gotchas before porting the whole suite — chrysalis is the
//! composition layer; the numerics stay in prism/spatio-flux.


use prism_bigraph::{Engine, ProcessNode, ProcessRegistry};

use chrysalis::ast::{
    CompositeDef, Def, Expr, Interface, Param, PortDecl, Program, SchemaExpr, StringLit,
};
use chrysalis::compile::{ModuleRegistry, compile_with_modules};
use prism_schema::MethodRegistry;

// ── Example 1: Monod kinetics (well-mixed) ───────────────────────────

/// `extern process Kinetics ~{biomass, substrates} ->{biomass, substrates}`
/// + a `Well` composite wiring it to a biomass scalar and a substrate map.
fn monod_program() -> Program {
    let float = || SchemaExpr::Float;
    let submap = || SchemaExpr::map_of(SchemaExpr::Float);

    let mut p = Program::new();

    // `Kinetics` is a native process imported wholesale (the `extern`
    // replacement); its interface comes from the call-site wiring + the factory.
    p.push(Def::Use {
        module: "natives".into(),
        names: vec!["Kinetics".into()],
    });

    p.push(Def::Composite(CompositeDef {
        name: "Well".into(),
        params: vec![
            Param::required("biomass", float()),
            Param::required("substrates", submap()),
        ],
        interface: Interface::new()
            .with_output("biomass", PortDecl::required(float()))
            .with_output("substrates", PortDecl::required(submap())),
        using: vec![],
        body: Expr::parallel(vec![
            Expr::entry("biomass", Expr::var("biomass")),
            Expr::entry("substrates", Expr::var("substrates")),
            Expr::entry(
                "kinetics",
                Expr::term("Kinetics")
                    .input("biomass", Expr::var("biomass"))
                    .input("substrates", Expr::var("substrates"))
                    .output("biomass", Expr::var("biomass"))
                    .output("substrates", Expr::var("substrates"))
                    .build(),
            ),
        ]),
    }));

    p.push(Def::Binding {
        name: "main".into(),
        schema: None,
        value: Expr::term("Well")
            .arg_named("biomass", Expr::float(0.1))
            .arg_named(
                "substrates",
                Expr::Map(vec![
                    (StringLit::plain("glucose"), Expr::float(10.0)),
                    (StringLit::plain("acetate"), Expr::float(0.0)),
                ]),
            )
            .build(),
    });

    p
}

#[test]
fn monod_kinetics_composed_from_chrysalis() {
    use spatio_flux::processes::monod_kinetics::{MonodKinetics, models};

    // The native factory backs `extern process Kinetics`.
    let mut natives = ProcessRegistry::new();
    natives.register("Kinetics", |_cfg| {
        ProcessNode::Process(Box::new(MonodKinetics::new(
            models::overflow_metabolism(),
            1.0,
        )))
    });

    let program = monod_program();
    let result = compile_with_modules(
        &program,
        natives,
        MethodRegistry::new(),
        ModuleRegistry::new().process("natives", "Kinetics"),
    )
    .expect("compile");

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(60.0);

    let state = engine.state();
    let biomass = state.get_field("biomass").and_then(|v| v.as_f64()).unwrap();
    let glucose = state
        .get_field("substrates")
        .and_then(|s| s.get_field("glucose"))
        .and_then(|v| v.as_f64())
        .unwrap();

    // Overflow metabolism on glucose: biomass grows from 0.1, glucose drops
    // from 10.0 — the native MonodKinetics ran, wired by chrysalis.
    assert!(biomass > 0.1, "biomass should grow from 0.1, got {biomass}");
    assert!(
        glucose < 10.0,
        "glucose should be consumed from 10.0, got {glucose}"
    );
}

// ── Example 2: Diffusion on a field (spatial) ────────────────────────

/// A flat field as a chrysalis `[Float]` list literal. GOTCHA: a real
/// 10×20 field is 200 entries — by hand this is impractical, so chrysalis
/// will want an array/field constructor. Here a 3×3 field keeps it legible.
fn field_list(vals: &[f64]) -> Expr {
    Expr::List(vals.iter().map(|&v| Expr::float(v)).collect())
}

fn diffusion_program() -> Program {
    // A field is an additively-applied numeric grid: `Array` (not a plain
    // `List`, which the engine REPLACES on apply — the gotcha that lost all
    // the glucose). DiffusionAdvection emits per-cell deltas.
    let fieldmap = || SchemaExpr::map_of(SchemaExpr::array(vec![3, 3], SchemaExpr::Float));

    let mut p = Program::new();

    // `Diffusion` is a native process imported wholesale (the `extern`
    // replacement); its interface comes from the call-site wiring + the factory.
    p.push(Def::Use {
        module: "natives".into(),
        names: vec!["Diffusion".into()],
    });

    p.push(Def::Composite(CompositeDef {
        name: "Dish".into(),
        params: vec![Param::required("fields", fieldmap())],
        interface: Interface::new().with_output("fields", PortDecl::required(fieldmap())),
        using: vec![],
        body: Expr::parallel(vec![
            Expr::entry("fields", Expr::var("fields")),
            Expr::entry(
                "diffusion",
                Expr::term("Diffusion")
                    .input("fields", Expr::var("fields"))
                    .output("fields", Expr::var("fields"))
                    .build(),
            ),
        ]),
    }));

    // 3×3 glucose gradient (y: 0 → 10), acetate flat zero.
    let glucose = field_list(&[0.0, 0.0, 0.0, 5.0, 5.0, 5.0, 10.0, 10.0, 10.0]);
    let acetate = field_list(&[0.0; 9]);
    p.push(Def::Binding {
        name: "main".into(),
        schema: None,
        value: Expr::term("Dish")
            .arg_named(
                "fields",
                Expr::Map(vec![
                    (StringLit::plain("glucose"), glucose),
                    (StringLit::plain("acetate"), acetate),
                ]),
            )
            .build(),
    });

    p
}

/// Sum a flat field at `fields.<name>`.
fn field_sum(state: &prism_schema::Value, name: &str) -> f64 {
    state
        .get_field("fields")
        .and_then(|f| f.get_field(name))
        .and_then(|v| v.as_list())
        .map(|l| l.iter().filter_map(|x| x.as_f64()).sum())
        .unwrap_or(f64::NAN)
}

fn field_vals(state: &prism_schema::Value, name: &str) -> Vec<f64> {
    state
        .get_field("fields")
        .and_then(|f| f.get_field(name))
        .and_then(|v| v.as_list())
        .map(|l| l.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default()
}

// GOTCHA #14 (FIXED): the field's additive `Array` slot schema is now honored.
// `apply_projections_to` PROMOTES the writer's output-port schema onto the
// slot's library schema (`algebra::promote`, law #5) instead of letting a
// native process's loose `Map`/`Any` output replace the field — so per-cell
// diffusion deltas apply element-wise (mass-conserving) rather than wholesale.
#[test]
fn diffusion_composed_from_chrysalis() {
    use indexmap::IndexMap;
    use spatio_flux::processes::diffusion_advection::DiffusionAdvection;

    let mut natives = ProcessRegistry::new();
    natives.register("Diffusion", |_cfg| {
        ProcessNode::Process(Box::new(DiffusionAdvection::new(
            (3, 3),
            (3.0, 3.0),
            IndexMap::from([("glucose".into(), 1e-1), ("acetate".into(), 1e-1)]),
            1.0,
        )))
    });

    let program = diffusion_program();
    let result = compile_with_modules(
        &program,
        natives,
        MethodRegistry::new(),
        ModuleRegistry::new().process("natives", "Diffusion"),
    )
    .expect("compile");

    let initial = field_vals(&result.initial_state, "glucose");
    let sum0 = field_sum(&result.initial_state, "glucose");

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(30.0);

    let state = engine.state();
    let after = field_vals(state, "glucose");
    let sum1 = field_sum(state, "glucose");

    // Diffusion conserves total mass (no-flux boundary) but smooths the
    // gradient — the field changed, the sum did not.
    assert!(
        (sum0 - sum1).abs() < 1e-6,
        "diffusion conserves total glucose: {sum0} vs {sum1}"
    );
    assert!(
        initial != after,
        "the gradient should have diffused (field changed)"
    );
}

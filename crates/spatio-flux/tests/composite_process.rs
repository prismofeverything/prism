//! The **composite-process use case** (#6): a spatio-flux composite defined
//! once (`Dish`, a diffusion field), **imported** into a larger composite
//! (`Culture`, which nests two `Dish`es), then **simulated as one step in a
//! workflow** that renders a spatio-flux report section.
//!
//! Pre-parser, a `.ys` "file" is a `Program` (a set of defs) and "import" is
//! merging one program's defs into another (`import_dish` below) — the
//! mechanism the parser's `import Dish from "dish.ys"` will drive. Everything
//! else is real: chrysalis composites, the native `DiffusionAdvection`
//! process, the engine running the nested composite, and (next) a step DAG.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{Engine, ProcessNode, ProcessRegistry};
use prism_schema::Value;

use chrysalis::ast::{
    CompositeDef, Def, Expr, ExternDef, Interface, Param, PortDecl, Program, SchemaExpr, StringLit,
};
use chrysalis::compile::compile_with_registry;
use spatio_flux::processes::diffusion_advection::DiffusionAdvection;

// ── helpers ─────────────────────────────────────────────────────────

fn field_list(vals: &[f64]) -> Expr {
    Expr::List(vals.iter().map(|&v| Expr::float(v)).collect())
}

fn fieldmap() -> SchemaExpr {
    SchemaExpr::map_of(SchemaExpr::array(vec![3, 3], SchemaExpr::Float))
}

/// A `{glucose, acetate}` field map literal (acetate flat zero).
fn gradient(glucose: &[f64]) -> Expr {
    Expr::Map(vec![
        (StringLit::plain("glucose"), field_list(glucose)),
        (StringLit::plain("acetate"), field_list(&[0.0; 9])),
    ])
}

// ── "dish.ys" — the reusable inner composite ────────────────────────

/// Merge the `Dish` composite (a diffusion field) + its `Diffusion` extern
/// into `p`. This is the **import**: pre-parser, bringing another file's defs
/// into scope = merging them (`import Dish from "dish.ys"` later).
fn import_dish(p: &mut Program) {
    p.push(Def::Extern(ExternDef {
        name: "Diffusion".into(),
        params: vec![],
        interface: Interface::new()
            .with_input("fields", PortDecl::required(fieldmap()))
            .with_output("fields", PortDecl::required(fieldmap())),
    }));
    // composite Dish[fields] ->{fields} ( fields: fields | Diffusion ~{fields}->{fields} )
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
}

// ── "culture.ys" — the larger composite that imports + nests Dish ───

/// `import Dish from "dish.ys"` + `composite Culture ( well_a = Dish[…] |
/// well_b = Dish[…] )` + `main = Culture()`.
fn culture_program() -> Program {
    let mut p = Program::new();
    import_dish(&mut p); // ← the import

    // Two wells with different glucose gradients (vertical vs horizontal).
    let grad_a = || gradient(&[0.0, 0.0, 0.0, 5.0, 5.0, 5.0, 10.0, 10.0, 10.0]);
    let grad_b = || gradient(&[0.0, 5.0, 10.0, 0.0, 5.0, 10.0, 0.0, 5.0, 10.0]);

    // Each nested Dish is a subengine; we observe its BRIDGED output port on
    // the parent — so wire each well's `fields` output to a DISTINCT Culture
    // slot (`fields_a` / `fields_b`), else both would bridge to one `fields`.
    p.push(Def::Composite(CompositeDef {
        name: "Culture".into(),
        params: vec![],
        interface: Interface::new(),
        using: vec![],
        body: Expr::parallel(vec![
            // Seed the observable slots with the initial gradient; each nested
            // Dish's bridge then lands its per-step diffusion *diffs* here, so
            // the slot tracks the live (mass-conserving) field.
            Expr::entry("fields_a", grad_a()),
            Expr::entry("fields_b", grad_b()),
            Expr::entry(
                "well_a",
                Expr::term("Dish")
                    .arg_named("fields", grad_a())
                    .output("fields", Expr::var("fields_a"))
                    .build(),
            ),
            Expr::entry(
                "well_b",
                Expr::term("Dish")
                    .arg_named("fields", grad_b())
                    .output("fields", Expr::var("fields_b"))
                    .build(),
            ),
        ]),
    }));

    p.push(Def::Binding { name: "main".into(), value: Expr::term("Culture").build() });
    p
}

/// The native factory backing `extern process Diffusion`.
fn diffusion_natives() -> ProcessRegistry {
    let mut natives = ProcessRegistry::new();
    natives.register("Diffusion", |_cfg| {
        ProcessNode::Process(Box::new(DiffusionAdvection::new(
            (3, 3),
            (3.0, 3.0),
            IndexMap::from([("glucose".into(), 1e-1), ("acetate".into(), 1e-1)]),
            1.0,
        )))
    });
    natives
}

/// The address of a nested process/composite spec at `state.<path…>`.
fn address_at(state: &Value, path: &[&str]) -> Option<String> {
    let mut node = state;
    for seg in path {
        node = node.get_field(seg)?;
    }
    node.get_field("address").and_then(|v| v.as_str()).map(str::to_string)
}

/// The composite-process use case, structurally: a `Dish` composite defined
/// once, **imported** into `Culture`, **nested** twice, and the whole thing
/// **instantiated and run** by the engine.
///
/// NOTE: observing each nested Dish's diffused *field* through its bridge does
/// NOT yet work — additive `Array` fields aren't reconstructed through a nested
/// composite's bridge (scalars are; see `nested_composite`). That's the nested
/// analogue of GOTCHA #14, tracked as a task; it blocks the live field-heatmap
/// report section. The structure below is sound regardless.
#[test]
fn culture_imports_and_nests_dish() {
    let program = culture_program();
    let result =
        compile_with_registry(&program, diffusion_natives()).expect("compile Culture (imports Dish)");

    // Import + nest: each well is a nested COMPOSITE spec carrying the imported
    // Dish's `Diffusion` sub-process — i.e. Dish's defs are in scope in Culture
    // (the import) and instantiated as subprocesses (the nesting).
    for well in ["well_a", "well_b"] {
        assert_eq!(
            address_at(&result.initial_state, &[well]).as_deref(),
            Some("local:Composite"),
            "{well} is a nested composite"
        );
        assert_eq!(
            address_at(&result.initial_state, &[well, "config", "state", "diffusion"]).as_deref(),
            Some("local:Diffusion"),
            "{well} nests the imported Dish's Diffusion process"
        );
    }

    // The composite-of-composites builds and runs without error (the engine
    // discovers and steps the nested subengines).
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(10.0);
}

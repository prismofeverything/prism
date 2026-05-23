//! Isolation test for composite-in-composite execution: a child
//! composite as a *direct slot* vs. inside a *map*. No units involved.
//!
//! `grow_divide` proves the map form runs (cells grow + divide inside the
//! Environment). This pins down whether a direct composite-valued slot
//! (`leaf: Leaf[...]`) runs the same way.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::Engine;

use chrysalis::ast::{
    CompositeDef, Def, Expr, Interface, Param, PortDecl, ProcessDef, Program, SchemaExpr, StringLit,
};

/// Push a `Bump` process (adds 1.0 to `v` each tick) and a `Leaf`
/// composite that wraps it.
fn push_base(p: &mut Program) {
    // process Bump ~{v: Float} ->{v: Float} ( {v: 1.0} )
    p.push(Def::Process(ProcessDef {
        name: "Bump".into(),
        params: vec![],
        interface: Interface::new()
            .with_input("v", PortDecl::required(SchemaExpr::Float))
            .with_output("v", PortDecl::required(SchemaExpr::Float)),
        body: Expr::Record(IndexMap::from_iter([("v".into(), Expr::float(1.0))])),
    }));
    // composite Leaf[v: Float = 0] ~{} ->{v} ( v: v | Bump ~{v: v} ->{v: v} )
    p.push(Def::Composite(CompositeDef {
        name: "Leaf".into(),
        params: vec![Param::with_default("v", SchemaExpr::Float, Expr::float(0.0))],
        interface: Interface::new().with_output("v", PortDecl::required(SchemaExpr::Float)),
        using: vec![],
        body: Expr::parallel(vec![
            Expr::entry("v", Expr::var("v")),
            Expr::entry(
                "bump",
                Expr::term("Bump")
                    .input("v", Expr::var("v"))
                    .output("v", Expr::var("v"))
                    .build(),
            ),
        ]),
    }));
}

fn run_and_read(p: &Program, path: &[&str]) -> Option<f64> {
    let result = chrysalis::compile::compile(p).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(3.0);
    let keys: Vec<prism_schema::Key> = path.iter().map(|s| prism_schema::Key::from(*s)).collect();
    let v = engine.state().get_path(&keys).and_then(|v| v.as_f64());
    eprintln!("path {path:?} => {v:?}\n  full state: {:?}", engine.state());
    v
}

#[test]
fn composite_in_map_runs() {
    // Mirror grow_divide exactly: the map of child composites is PASSED
    // IN via main as a param, so it sits in the initial root state.
    //   composite Holder[leaves: Any] ~{} ->{leaves} ( leaves: leaves )
    //   main = Holder[leaves: {'a': Leaf[]}]
    let mut p = Program::new();
    push_base(&mut p);
    p.push(Def::Composite(CompositeDef {
        name: "Holder".into(),
        params: vec![Param::required("leaves", SchemaExpr::Any)],
        interface: Interface::new().with_output("leaves", PortDecl::required(SchemaExpr::Any)),
        using: vec![],
        body: Expr::parallel(vec![Expr::entry("leaves", Expr::var("leaves"))]),
    }));
    p.push(Def::Binding {
        name: "main".into(),
        schema: None,
        value: Expr::term("Holder")
            .arg_named(
                "leaves",
                Expr::Map(vec![(StringLit::plain("a"), Expr::term("Leaf").build())]),
            )
            .build(),
    });

    // Observe the Leaf's BRIDGED output on its parent (`leaves.v`), not its
    // encapsulated inner slot (`leaves.a.v`). A composite is a subengine —
    // you see what it exposes through its bridge, not its internals.
    let v = run_and_read(&p, &["leaves", "v"]).unwrap_or(0.0);
    assert!(v > 0.0, "Leaf in a passed-in map should run and expose grown v (got {v})");
}

#[test]
fn composite_in_direct_slot_runs() {
    // composite Holder ~{} ->{leaf} ( leaf: Leaf[] ) — Holder is `main`,
    // inlined, so the `leaf` subengine sits at root.leaf and exposes its
    // output on the parent (root). (Previously #[ignore]d as an "engine bug";
    // it was the assertion reading into the subengine, not a real bug.)
    let mut p = Program::new();
    push_base(&mut p);
    p.push(Def::Composite(CompositeDef {
        name: "Holder".into(),
        params: vec![],
        interface: Interface::new()
            .with_output("leaf", PortDecl::required(SchemaExpr::custom("Leaf"))),
        using: vec![],
        body: Expr::parallel(vec![Expr::entry("leaf", Expr::term("Leaf").build())]),
    }));
    p.push(Def::Binding {
        name: "main".into(),
        schema: None,
        value: Expr::term("Holder").build(),
    });

    // Observe the Leaf's bridged output on its parent (root `v`).
    let v = run_and_read(&p, &["v"]).unwrap_or(0.0);
    assert!(v > 0.0, "Leaf in a direct slot should run and expose grown v (got {v})");
}

//! The `.ys` parser, end-to-end: parse `ys/graph.ys` (a REAL file, via the
//! lexer + recursive-descent parser) → `Program` → compile → run the
//! type's methods. The parser is a thin front-end: the resulting program is
//! the same one the hand-built fixture (`fixtures::graph`) produces, so it
//! compiles and runs identically.

use chrysalis::compile::compile;
use chrysalis::parse::parse_program;
use prism_schema::{Schema, Value, algebra};

/// The actual surface file (compiled in, so it's path/CWD-independent).
const GRAPH_YS: &str = include_str!("../ys/graph.ys");

fn s(x: &str) -> Value {
    Value::String(x.into())
}

fn graph_schema() -> Schema {
    Schema::Custom {
        name: "Graph".into(),
        parameters: Default::default(),
    }
}

#[test]
fn parses_graph_ys_and_runs() {
    let program = parse_program(GRAPH_YS).expect("parse ys/graph.ys");
    let result = compile(&program).expect("compile the parsed graph type");
    let m = &result.core.methods;
    let types = &*result.core.types;
    let graph_t = graph_schema();

    // A write method parsed from the file returns the delta.
    let delta = m.dispatch(&graph_empty(), "add_node", &[s("x")]).unwrap();
    assert_eq!(
        delta
            .get_path(&["nodes".into(), "_add".into()])
            .and_then(|v| v.as_list()),
        Some(&[s("x")][..]),
        "parsed add_node returns {{nodes: {{_add: [x]}}}}"
    );

    // Build a graph via the parsed methods + the algebra (Custom→repr).
    let mut g = graph_empty();
    for n in ["a", "b", "c"] {
        let d = m.dispatch(&g, "add_node", &[s(n)]).unwrap();
        g = algebra::apply_with(Some(types), &graph_t, &g, &d);
    }
    for (from, to) in [("a", "b"), ("a", "c")] {
        let d = m.dispatch(&g, "add_edge", &[s(from), s(to)]).unwrap();
        g = algebra::apply_with(Some(types), &graph_t, &g, &d);
    }
    assert_eq!(
        g.get_field("nodes")
            .and_then(|v| v.as_list())
            .map(<[_]>::len),
        Some(3)
    );
    assert_eq!(
        g.get_field("edges")
            .and_then(|v| v.as_list())
            .map(<[_]>::len),
        Some(2)
    );

    // A read method parsed from the file: the comprehension query.
    let nbrs = m.dispatch(&g, "neighbors", &[s("a")]).unwrap();
    assert_eq!(
        nbrs,
        Value::List(vec![s("b"), s("c")]),
        "parsed neighbors(a) = [b, c]"
    );

    // remove_node parsed from the file: removes c and its incident edges.
    let d = m.dispatch(&g, "remove_node", &[s("c")]).unwrap();
    let g = algebra::apply_with(Some(types), &graph_t, &g, &d);
    assert_eq!(
        g.get_field("nodes")
            .and_then(|v| v.as_list())
            .map(<[_]>::len),
        Some(2),
        "remove_node dropped c"
    );
    assert_eq!(
        g.get_field("edges")
            .and_then(|v| v.as_list())
            .map(<[_]>::len),
        Some(1),
        "remove_node dropped the incident a→c edge"
    );
}

fn graph_empty() -> Value {
    Value::tree([
        ("_type", Value::String("Graph".into())),
        ("nodes", Value::List(vec![])),
        ("edges", Value::List(vec![])),
    ])
}

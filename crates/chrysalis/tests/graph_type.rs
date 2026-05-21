//! Proof of first-class user types (#7): a directed-graph type defined
//! **entirely in chrysalis** (`fixtures::graph`) — no Rust method bodies
//! (contrast `prism_schema::registry::GraphTypeMethods`).
//!
//! The model (confirmed): there is no special "directive" layer. A method is a
//! **pure function returning the *data describing* a change** (a delta) or a
//! query value — it never mutates. `apply` on a `Custom(Graph)` slot
//! **delegates to the type's representation**, so a write-method's delta
//! (`add_node('x') ⇒ {nodes: {_add: ['x']}}`) lands and composes *exactly like
//! `_add`*. Demonstrates:
//!   * `type` compiles with no `main` (a file is a composite);
//!   * write methods return deltas; `apply` (delegating to the representation)
//!     installs them, and they **compose** (sequential apply + `reconcile`);
//!   * a read method / query (`neighbors`) via the list comprehension.

use chrysalis::compile::compile;
use chrysalis::fixtures::graph as g;
use prism_schema::{algebra, Schema, Value};

fn s(x: &str) -> Value {
    Value::String(x.into())
}

/// The `Custom(Graph)` slot schema.
fn graph_schema() -> Schema {
    Schema::Custom { name: "Graph".into(), parameters: Default::default() }
}

#[test]
fn graph_type_defined_in_chrysalis_is_first_class() {
    let result = compile(&g::program()).expect("compile a type-only program (no main)");
    let m = &result.methods;
    let types = &*result.type_registry;
    let graph_t = graph_schema();

    // A write method is PURE: it returns the *delta*, not a new graph.
    let delta = m.dispatch(&g::empty_graph(), "add_node", &[s("x")]).unwrap();
    assert_eq!(
        delta.get_path(&["nodes".into(), "_add".into()]).and_then(|v| v.as_list()),
        Some(&[s("x")][..]),
        "add_node returns the delta {{nodes: {{_add: [x]}}}}"
    );

    // Build a graph by APPLYING those deltas to a `Custom(Graph)` slot. apply
    // delegates to the representation, so the delta lands like any `_add`.
    let mut graph = g::empty_graph();
    for n in ["a", "b", "c"] {
        let d = m.dispatch(&graph, "add_node", &[s(n)]).unwrap();
        graph = algebra::apply_with(Some(types), &graph_t, &graph, &d);
    }
    for (from, to) in [("a", "b"), ("a", "c")] {
        let d = m.dispatch(&graph, "add_edge", &[s(from), s(to)]).unwrap();
        graph = algebra::apply_with(Some(types), &graph_t, &graph, &d);
    }

    // Structure grew, and the `_type` tag survived (apply only touched
    // nodes/edges via the representation).
    assert_eq!(graph.get_field("_type").and_then(|v| v.as_str()), Some("Graph"));
    assert_eq!(graph.get_field("nodes").and_then(|v| v.as_list()).map(<[_]>::len), Some(3));
    assert_eq!(graph.get_field("edges").and_then(|v| v.as_list()).map(<[_]>::len), Some(2));

    // Read method (query): the comprehension `[edge.to for edge in self.edges
    // if edge.from == "a"]`.
    let nbrs = m.dispatch(&graph, "neighbors", &[s("a")]).unwrap();
    assert_eq!(nbrs, Value::List(vec![s("b"), s("c")]), "a → b, a → c");
    let nbrs_b = m.dispatch(&graph, "neighbors", &[s("b")]).unwrap();
    assert_eq!(nbrs_b, Value::List(vec![]), "b has no out-edges");
}

/// Helper: apply a method's delta to a `Custom(Graph)` slot.
fn act(
    result: &chrysalis::compile::CompileResult,
    graph: &Value,
    method: &str,
    args: &[Value],
) -> Value {
    let delta = result.methods.dispatch(graph, method, args).unwrap();
    algebra::apply_with(Some(&result.type_registry), &graph_schema(), graph, &delta)
}

#[test]
fn arbitrary_operations_land_in_the_complete_delta_basis() {
    // `remove_node` / `remove_edge` / `union_with` are arbitrary operations
    // whose EFFECT is `_add`/`_remove` deltas — the per-sort basis is complete,
    // so no new update primitive is needed. (The user's question: "what about
    // remove / union?" — answered here.)
    let result = compile(&g::program()).unwrap();

    // Build a→b, a→c, b→c over {a,b,c}.
    let mut graph = g::empty_graph();
    for n in ["a", "b", "c"] {
        graph = act(&result, &graph, "add_node", &[s(n)]);
    }
    for (f, t) in [("a", "b"), ("a", "c"), ("b", "c")] {
        graph = act(&result, &graph, "add_edge", &[s(f), s(t)]);
    }

    // remove_edge(a,b): a `_remove` delta on the edge set (by value).
    graph = act(&result, &graph, "remove_edge", &[s("a"), s("b")]);
    let nbrs_a = result.methods.dispatch(&graph, "neighbors", &[s("a")]).unwrap();
    assert_eq!(nbrs_a, Value::List(vec![s("c")]), "a→b removed, a→c remains");

    // remove_node(c): removes c AND its incident edges (a→c, b→c) — a delta
    // computed by the method (a comprehension over self.edges).
    graph = act(&result, &graph, "remove_node", &[s("c")]);
    assert_eq!(
        graph.get_field("nodes").and_then(|v| v.as_list()),
        Some(&[s("a"), s("b")][..]),
        "c removed"
    );
    assert_eq!(
        graph.get_field("edges").and_then(|v| v.as_list()).map(<[_]>::len),
        Some(0),
        "incident edges a→c, b→c removed with c"
    );

    // union_with(other): an arbitrary binary op whose effect is `_add` deltas
    // (set-difference, via `in`). Adds only what's missing.
    let mut other = g::empty_graph();
    for n in ["b", "d"] {
        other = act(&result, &other, "add_node", &[s(n)]);
    }
    other = act(&result, &other, "add_edge", &[s("b"), s("d")]);

    graph = act(&result, &graph, "union_with", &[other]);
    let mut nodes: Vec<String> = graph
        .get_field("nodes")
        .and_then(|v| v.as_list())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    nodes.sort();
    assert_eq!(nodes, vec!["a", "b", "d"], "union adds d (b already present, not duplicated)");
    let nbrs_b = result.methods.dispatch(&graph, "neighbors", &[s("b")]).unwrap();
    assert_eq!(nbrs_b, Value::List(vec![s("d")]), "union brought edge b→d");
}

#[test]
fn add_node_deltas_compose_like_add() {
    // The whole point of returning deltas: concurrent `add_node`s **collate**
    // (just like `_add`) instead of clobbering. `reconcile` unions them at the
    // representation; applying the combined delta adds both nodes.
    let result = compile(&g::program()).unwrap();
    let m = &result.methods;
    let types = &*result.type_registry;
    let repr = types.get("Graph").expect("Graph registered").schema.clone();

    let d_a = m.dispatch(&g::empty_graph(), "add_node", &[s("a")]).unwrap();
    let d_b = m.dispatch(&g::empty_graph(), "add_node", &[s("b")]).unwrap();

    let combined = algebra::reconcile(&repr, &[d_a, d_b]).expect("two adds reconcile to one delta");
    let graph = algebra::apply_with(Some(types), &graph_schema(), &g::empty_graph(), &combined);

    let nodes = graph.get_field("nodes").and_then(|v| v.as_list()).unwrap();
    assert_eq!(nodes, &[s("a"), s("b")], "reconcile unioned the two _add deltas");
}

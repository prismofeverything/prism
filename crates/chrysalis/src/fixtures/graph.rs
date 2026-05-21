//! A directed-graph type **defined entirely in chrysalis** — the consumer that
//! proves first-class user types (#7).
//!
//! Surface form (once the parser lands, this is a literal `.ys` file):
//!
//! ```text
//! type Graph = { nodes: list[string], edges: list[{from: string, to: string}] }
//! with {
//!   add_node(id: string)        = { nodes: self.nodes ++ [id], edges: self.edges }
//!   add_edge(from: string, to: string) =
//!       { nodes: self.nodes, edges: self.edges ++ [{from: from, to: to}] }
//!   neighbors(id: string)       = [edge.to for edge in self.edges if edge.from == id]
//! }
//! ```
//!
//! `add_node`/`add_edge` are **write methods** (build a new graph; usable as
//! `apply` directives); `neighbors` is a **read method** (a query — note the
//! list comprehension, the iteration the language gained for this). The type is
//! first-class: its `nodes`/`edges` representation drives the structural
//! algebra ops, and the methods are dispatched on the `_type: Graph` tag. No
//! Rust — contrast `prism_schema::registry::GraphTypeMethods`.

use indexmap::IndexMap;

use prism_schema::Value;

use crate::ast::{
    BinOp, Def, Expr, MethodDef, Param, PlacePath, Program, SchemaExpr, StringLit, TypeDef,
};

/// `base.field` (field access on a bound variable, e.g. `self.nodes`).
fn path(base: &str, field: &str) -> Expr {
    Expr::Path(PlacePath::local(base).dot(field))
}

fn binop(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
}

fn record(fields: Vec<(&str, Expr)>) -> Expr {
    Expr::Record(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// A structural-add delta `{ field: {_add: [items…]} }` — the *data describing*
/// adding `items` to a collection field, in the representation's update
/// vocabulary. A write-method returns this (pure); `apply` installs it.
fn add_to(field: &str, items: Vec<Expr>) -> Expr {
    record(vec![(
        field,
        Expr::Map(vec![(StringLit::plain("_add"), Expr::List(items))]),
    )])
}

/// The program declaring the `Graph` type (no `main` — a file is a composite;
/// this one only exports a type).
pub fn program() -> Program {
    let edge_schema = SchemaExpr::Record(IndexMap::from_iter([
        ("from".to_string(), SchemaExpr::String),
        ("to".to_string(), SchemaExpr::String),
    ]));
    let representation = SchemaExpr::Record(IndexMap::from_iter([
        ("nodes".to_string(), SchemaExpr::list_of(SchemaExpr::String)),
        ("edges".to_string(), SchemaExpr::list_of(edge_schema)),
    ]));

    let methods = vec![
        // add_node(id) = { nodes: {_add: [id]} }  — a delta, not a new graph.
        MethodDef {
            name: "add_node".into(),
            params: vec![Param::required("id", SchemaExpr::String)],
            body: add_to("nodes", vec![Expr::var("id")]),
        },
        // add_edge(from, to) = { edges: {_add: [{from, to}]} }  — a delta.
        MethodDef {
            name: "add_edge".into(),
            params: vec![
                Param::required("from", SchemaExpr::String),
                Param::required("to", SchemaExpr::String),
            ],
            body: add_to(
                "edges",
                vec![record(vec![("from", Expr::var("from")), ("to", Expr::var("to"))])],
            ),
        },
        // neighbors(id) = [edge.to for edge in self.edges if edge.from == id]
        MethodDef {
            name: "neighbors".into(),
            params: vec![Param::required("id", SchemaExpr::String)],
            body: Expr::Comprehension {
                var: "edge".into(),
                source: Box::new(path("self", "edges")),
                filter: Some(Box::new(binop(BinOp::Eq, path("edge", "from"), Expr::var("id")))),
                body: Box::new(path("edge", "to")),
            },
        },
    ];

    let mut p = Program::new();
    p.push(Def::Type(TypeDef {
        name: "Graph".into(),
        params: vec![],
        representation,
        methods,
    }));
    p
}

/// An empty graph value, tagged `_type: Graph` for method dispatch.
pub fn empty_graph() -> Value {
    Value::tree([
        ("_type", Value::String("Graph".into())),
        ("nodes", Value::List(vec![])),
        ("edges", Value::List(vec![])),
    ])
}

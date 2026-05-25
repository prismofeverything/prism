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
    BinOp, Def, Expr, MethodDef, Param, PlacePath, Program, SchemaExpr, StringLit, TypeDef, UnaryOp,
};

/// `base.field` (field access on a bound variable, e.g. `self.nodes`).
fn path(base: &str, field: &str) -> Expr {
    Expr::Path(PlacePath::local(base).dot(field))
}

fn binop(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

fn not_(e: Expr) -> Expr {
    Expr::UnaryOp {
        op: UnaryOp::Not,
        operand: Box::new(e),
    }
}

fn record(fields: Vec<(&str, Expr)>) -> Expr {
    Expr::Record(
        fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

/// A structural collection delta `{ field: {op: items} }` — the *data
/// describing* a change to a collection field, in the representation's complete
/// delta basis (`_add` / `_remove`). A write-method returns this (pure);
/// `apply` installs it via the representation.
fn coll(field: &str, op: &str, items: Expr) -> Expr {
    record(vec![(
        field,
        Expr::Map(vec![(StringLit::plain(op), items)]),
    )])
}

/// `[ body for var in src if filter ]`.
fn comp(var: &str, src: Expr, filter: Expr, body: Expr) -> Expr {
    Expr::Comprehension {
        key_var: None,
        var: var.to_string(),
        source: Box::new(src),
        filter: Some(Box::new(filter)),
        body: Box::new(body),
        key: None,
    }
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

    let edge =
        |from: &str, to: &str| record(vec![("from", Expr::var(from)), ("to", Expr::var(to))]);

    let methods = vec![
        // ── write methods: pure delta-constructors over the complete basis ──
        // add_node(id) = { nodes: {_add: [id]} }
        MethodDef {
            name: "add_node".into(),
            params: vec![Param::required("id", SchemaExpr::String)],
            body: coll("nodes", "_add", Expr::List(vec![Expr::var("id")])),
        },
        // add_edge(from, to) = { edges: {_add: [{from, to}]} }
        MethodDef {
            name: "add_edge".into(),
            params: vec![
                Param::required("from", SchemaExpr::String),
                Param::required("to", SchemaExpr::String),
            ],
            body: coll("edges", "_add", Expr::List(vec![edge("from", "to")])),
        },
        // remove_node(id) = { nodes: {_remove: [id]},
        //                     edges: {_remove: [e for e in self.edges
        //                                       if e.from == id or e.to == id]} }
        // A `remove` is just as expressible as `add` — both land in the basis.
        MethodDef {
            name: "remove_node".into(),
            params: vec![Param::required("id", SchemaExpr::String)],
            body: record(vec![
                (
                    "nodes",
                    Expr::Map(vec![(
                        StringLit::plain("_remove"),
                        Expr::List(vec![Expr::var("id")]),
                    )]),
                ),
                (
                    "edges",
                    Expr::Map(vec![(
                        StringLit::plain("_remove"),
                        comp(
                            "e",
                            path("self", "edges"),
                            binop(
                                BinOp::Or,
                                binop(BinOp::Eq, path("e", "from"), Expr::var("id")),
                                binop(BinOp::Eq, path("e", "to"), Expr::var("id")),
                            ),
                            Expr::var("e"),
                        ),
                    )]),
                ),
            ]),
        },
        // remove_edge(from, to) = { edges: {_remove: [{from, to}]} }
        MethodDef {
            name: "remove_edge".into(),
            params: vec![
                Param::required("from", SchemaExpr::String),
                Param::required("to", SchemaExpr::String),
            ],
            body: coll("edges", "_remove", Expr::List(vec![edge("from", "to")])),
        },
        // union_with(other) = { nodes: {_add: [n for n in other.nodes if not (n in self.nodes)]},
        //                       edges: {_add: [e for e in other.edges if not (e in self.edges)]} }
        // An arbitrary (binary) operation whose EFFECT is `_add` deltas — no
        // new update primitive needed; the basis is complete.
        MethodDef {
            name: "union_with".into(),
            params: vec![Param::required("other", SchemaExpr::custom("Graph"))],
            body: record(vec![
                (
                    "nodes",
                    Expr::Map(vec![(
                        StringLit::plain("_add"),
                        comp(
                            "n",
                            path("other", "nodes"),
                            not_(binop(BinOp::In, Expr::var("n"), path("self", "nodes"))),
                            Expr::var("n"),
                        ),
                    )]),
                ),
                (
                    "edges",
                    Expr::Map(vec![(
                        StringLit::plain("_add"),
                        comp(
                            "e",
                            path("other", "edges"),
                            not_(binop(BinOp::In, Expr::var("e"), path("self", "edges"))),
                            Expr::var("e"),
                        ),
                    )]),
                ),
            ]),
        },
        // ── read method: a query (returns a value, not a delta) ──
        // neighbors(id) = [edge.to for edge in self.edges if edge.from == id]
        MethodDef {
            name: "neighbors".into(),
            params: vec![Param::required("id", SchemaExpr::String)],
            body: comp(
                "edge",
                path("self", "edges"),
                binop(BinOp::Eq, path("edge", "from"), Expr::var("id")),
                path("edge", "to"),
            ),
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

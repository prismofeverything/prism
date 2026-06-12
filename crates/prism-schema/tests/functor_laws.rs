//! Functoriality laws for [`prism_schema::functor::apply_functor`] — the executable
//! axioms `categorical-core.md` §8 demands of a functor (`F(id)=id`, `F(a⊗b)=F(a)⊗F(b)`,
//! `F(g∘f)=F(g)∘F(f)`), plus the two consumer proofs from `docs/functors.md`: render IS a
//! functor (the structural lift over a real bigraph) and a domain *view* = override one
//! generator + delegate the rest to the structural default.

use prism_schema::functor::{apply_functor, Functor, Identity};
use prism_schema::value::{StateMap, Value};

fn s(text: &str) -> Value {
    Value::String(text.to_string())
}

/// A render-shaped functor into a tiny markup prop: every node -> a `box` carrying its
/// generator label + its mapped children; a parallel composite -> a `group`; a leaf -> a
/// `text`. The shape of `View : Source -> Markup` (`functors.md` §2), at the core layer
/// (no `prism-viz` dep — viz owns the real SVG target table).
struct Markup;
impl Functor for Markup {
    fn construct(&self, control: Option<&str>, _node: &StateMap, mapped: StateMap) -> Value {
        Value::tree([
            ("tag", s("box")),
            ("label", s(control.unwrap_or("·"))),
            ("children", Value::Map(mapped)),
        ])
    }
    fn construct_list(&self, mapped: Vec<Value>) -> Value {
        Value::tree([("tag", s("group")), ("items", Value::List(mapped))])
    }
    fn construct_leaf(&self, leaf: &Value) -> Value {
        Value::tree([("tag", s("text")), ("value", leaf.clone())])
    }
}

// ── Law 1: F(id) = id ─────────────────────────────────────────────────────────────

#[test]
fn identity_functor_is_identity() {
    let leaf = Value::Int(7);
    let parallel = Value::List(vec![Value::Int(1), s("a")]);
    let nested = Value::tree([
        ("_type", s("P")),
        ("child", Value::tree([("_type", s("C")), ("x", Value::Int(1))])),
    ]);
    for v in [&leaf, &parallel, &nested] {
        assert_eq!(apply_functor(&Identity, v), *v, "F(id) must equal id");
    }
}

// ── Law 2: F(a ⊗ b) = F(a) ⊗ F(b)  (tensor / parallel) ──────────────────────────────

#[test]
fn functor_preserves_tensor_parallel() {
    let a = Value::tree([("_type", s("A"))]);
    let b = Value::tree([("_type", s("B"))]);
    let whole = Value::List(vec![a.clone(), b.clone()]);

    let got = apply_functor(&Markup, &whole);
    // F applied to the whole parallel == the target-tensor of F applied to each part.
    let expected = Markup.construct_list(vec![apply_functor(&Markup, &a), apply_functor(&Markup, &b)]);
    assert_eq!(got, expected, "F(a ⊗ b) = F(a) ⊗ F(b)");

    // concretely: the group's items are exactly [F(a), F(b)], in order.
    let items = got.as_map().unwrap().get("items").unwrap();
    assert_eq!(
        items,
        &Value::List(vec![apply_functor(&Markup, &a), apply_functor(&Markup, &b)])
    );
}

// ── Law 3: F(g ∘ f) = F(g) ∘ F(f)  (composition / nesting) ──────────────────────────

#[test]
fn functor_preserves_composition_nesting() {
    let inner = Value::tree([("_type", s("C")), ("x", Value::Int(1))]);
    let outer = Value::tree([("_type", s("P")), ("child", inner.clone())]);

    let got = apply_functor(&Markup, &outer);
    // The rendered parent's `children.child` is EXACTLY the independently-rendered child:
    // the postorder lift makes F(P ∘ C) = F(P) ∘ F(C).
    let children = got.as_map().unwrap().get("children").unwrap().as_map().unwrap();
    assert_eq!(
        children.get("child").unwrap(),
        &apply_functor(&Markup, &inner),
        "F(g ∘ f) = F(g) ∘ F(f): the nested image equals the independently-mapped child"
    );
    // and the parent box carries its own generator label (the control became its image).
    assert_eq!(got.as_map().unwrap().get("label").unwrap(), &s("P"));
}

// ── Consumer 1: render IS a functor (the structural lift over a real bigraph) ───────

#[test]
fn render_is_a_functor_over_a_real_bigraph() {
    // a small "engine state"-shaped bigraph: a colony with a cell + an environment leaf.
    let state = Value::tree([
        (
            "colony",
            Value::tree([
                ("cell0", Value::tree([("_type", s("Cell")), ("mass", Value::Int(3))])),
            ]),
        ),
        ("glucose", Value::Int(10)),
    ]);

    let view = apply_functor(&Markup, &state);

    // structure preserved: top is a box; nesting carried; the cell's control labels its box;
    // the scalar leaf became a `text`.
    let top = view.as_map().unwrap();
    assert_eq!(top.get("tag").unwrap(), &s("box"));
    let top_children = top.get("children").unwrap().as_map().unwrap();
    let cell_box = top_children
        .get("colony")
        .unwrap()
        .as_map()
        .unwrap()
        .get("children")
        .unwrap()
        .as_map()
        .unwrap()
        .get("cell0")
        .unwrap()
        .as_map()
        .unwrap();
    assert_eq!(cell_box.get("label").unwrap(), &s("Cell"));
    // the `glucose` leaf rendered as a text node carrying its value.
    let glucose = top_children.get("glucose").unwrap().as_map().unwrap();
    assert_eq!(glucose.get("tag").unwrap(), &s("text"));
    assert_eq!(glucose.get("value").unwrap(), &Value::Int(10));
}

// ── Consumer 2: a view = override one generator + delegate the rest (functors.md §2) ─

/// A domain VIEW: render a `Cell` generator as a circle (radius = its mass); delegate
/// every other generator to the structural default (`Identity::construct`). This is the
/// "compose + delegate" pattern — a view is a method-override-with-a-default.
struct CellView;
impl Functor for CellView {
    fn construct(&self, control: Option<&str>, node: &StateMap, mapped: StateMap) -> Value {
        if control == Some("Cell") {
            Value::tree([
                ("tag", s("circle")),
                ("r", node.get("mass").cloned().unwrap_or(Value::Int(1))),
            ])
        } else {
            // delegate to the generic structural functor for the generators we don't view.
            Identity.construct(control, node, mapped)
        }
    }
}

#[test]
fn a_view_overrides_one_generator_and_delegates_the_rest() {
    let state = Value::tree([
        ("cell0", Value::tree([("_type", s("Cell")), ("mass", Value::Int(3))])),
        ("env", Value::tree([("glucose", Value::Int(10))])),
    ]);

    let view = apply_functor(&CellView, &state);
    let top = view.as_map().unwrap();

    // the Cell generator got the custom view (a circle of radius = mass)…
    let cell = top.get("cell0").unwrap().as_map().unwrap();
    assert_eq!(cell.get("tag").unwrap(), &s("circle"));
    assert_eq!(cell.get("r").unwrap(), &Value::Int(3));

    // …and the un-viewed `env` subtree was delegated to the structural default (identity),
    // so it is carried through unchanged.
    assert_eq!(
        top.get("env").unwrap(),
        &Value::tree([("glucose", Value::Int(10))]),
        "a view delegates un-overridden generators to the structural functor"
    );
}

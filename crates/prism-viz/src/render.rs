//! The structural **render functor** — a bigraph's place graph as nested SVG boxes
//! (Milner's nested regions). The first consumer of
//! [`prism_schema::functor::apply_functor`] (`docs/functors.md` §2,
//! `categorical-core.md` §7b): **render = a functor** into the markup prop.
//!
//! `apply_functor` owns the place-graph recursion (functorial by construction); this
//! [`Functor`] table decides only each node's image (a labelled box) and each leaf's
//! (its value text). Domain **view** functors (`Waveform : Signal → Svg`,
//! `OrganismBoard : Colony → Svg`) override the generators they care about and
//! **delegate here** for the rest — that is the payoff of render-as-a-functor.
//!
//! Injected into the web boundary (`prism_bigraph::protocols::web`) by chrysalis (the
//! boundary cannot dep prism-viz — a cycle). The output is itself a place-graph
//! `Value` (`crate::svg`); [`to_svg`] serializes it.

use prism_schema::functor::{apply_functor, Functor};
use prism_schema::value::StateMap;
use prism_schema::Value;

use crate::svg::{el, group, svg, to_svg};

// ── layout (px) ──
const PAD: f64 = 7.0;
const LINE: f64 = 16.0;
const GAP: f64 = 3.0;
const CHAR: f64 = 7.2;
const INDENT: f64 = 12.0;
const HEADER: f64 = 26.0;
const MINW: f64 = 44.0;

// ── colours (the viewer's dark theme) ──
const NODE_FILL: &str = "#171a21";
const NODE_STROKE: &str = "#2b3342";
const KEY_COLOR: &str = "#7fb0ff";
const CTRL_COLOR: &str = "#56b6c2";
const CLOCK_COLOR: &str = "#7f8a9a";

/// The keys that name a node's generator (its control) — shown as the box's badge,
/// not as child rows. Mirrors `prism_schema::functor`'s private `CONTROL_KEYS`.
const CONTROL_KEYS: &[&str] = &["_type", "address", "control"];

/// The generic structural render: a bigraph place graph → nested SVG boxes.
pub struct StructuralRender;

impl Functor for StructuralRender {
    /// A leaf scalar → a sized fragment carrying its display text (the parent draws
    /// it inline as `key = text`).
    fn construct_leaf(&self, leaf: &Value) -> Value {
        let t = scalar_text(leaf);
        let w = (t.chars().count() as f64) * CHAR + 4.0;
        frag(w, LINE, true, Some(t), Value::None)
    }

    /// A node → a labelled box: a control badge (if any) over its children, each a
    /// `key = value` row (leaf) or a `key` label above the child's nested box.
    fn construct(&self, control: Option<&str>, _node: &StateMap, mapped: StateMap) -> Value {
        let mut elems: Vec<Value> = Vec::new();
        let mut y = PAD;
        let mut innerw = MINW;

        if let Some(c) = control {
            elems.push(text_at(PAD, y + LINE - 4.0, c, CTRL_COLOR, true));
            innerw = innerw.max((c.chars().count() as f64) * CHAR + 2.0 * PAD);
            y += LINE + GAP;
        }

        for (k, child) in &mapped {
            if CONTROL_KEYS.contains(&k.as_str()) {
                continue;
            }
            let (cw, ch, leaf, ctext, cel) = read_frag(child);
            if leaf {
                let row = format!("{k} = {}", ctext.unwrap_or_default());
                elems.push(text_at(PAD, y + LINE - 4.0, &row, KEY_COLOR, false));
                innerw = innerw.max((row.chars().count() as f64) * CHAR + 2.0 * PAD);
                y += LINE + GAP;
            } else {
                elems.push(text_at(PAD, y + LINE - 4.0, k.as_str(), KEY_COLOR, false));
                elems.push(translate(PAD + INDENT, y + LINE, cel));
                innerw = innerw.max(PAD + INDENT + cw + PAD);
                y += LINE + ch + GAP;
            }
        }

        let w = innerw;
        let h = (y - GAP).max(LINE) + PAD;
        let mut kids = vec![box_rect(w, h)];
        kids.extend(elems);
        frag(w, h, false, None, group(kids))
    }

    /// A parallel composite (`List`) → its items stacked in one box.
    fn construct_list(&self, mapped: Vec<Value>) -> Value {
        let mut elems: Vec<Value> = Vec::new();
        let mut y = PAD;
        let mut innerw = MINW;
        for child in &mapped {
            let (cw, ch, _leaf, _t, cel) = read_frag(child);
            elems.push(translate(PAD, y, cel));
            innerw = innerw.max(PAD + cw + PAD);
            y += ch + GAP;
        }
        let w = innerw;
        let h = (y - GAP).max(LINE) + PAD;
        let mut kids = vec![box_rect(w, h)];
        kids.extend(elems);
        frag(w, h, false, None, group(kids))
    }
}

/// Render a bigraph state (+ the clock) to a standalone SVG document string — the
/// functor injected into the web boundary.
pub fn render_bigraph(state: &Value, time: f64) -> String {
    let f = apply_functor(&StructuralRender, state);
    let (w, h, _leaf, _t, el) = read_frag(&f);
    let doc = svg(
        w + 2.0 * PAD,
        h + HEADER + PAD,
        vec![
            text_at(PAD, HEADER - 9.0, &format!("t = {time}"), CLOCK_COLOR, false),
            translate(PAD, HEADER, el),
        ],
    );
    to_svg(&doc)
}

// ── the sized-fragment intermediate (postorder layout carrier) ──

/// `{w, h, leaf, text?, el}` — a rendered subtree positioned at the origin plus its
/// bounding size, so a parent can stack/position it (postorder layout).
fn frag(w: f64, h: f64, leaf: bool, text: Option<String>, el: Value) -> Value {
    let mut m = StateMap::new();
    m.insert("w".into(), Value::float(w));
    m.insert("h".into(), Value::float(h));
    m.insert("leaf".into(), Value::Bool(leaf));
    if let Some(t) = text {
        m.insert("text".into(), Value::String(t));
    }
    m.insert("el".into(), el);
    Value::Map(m)
}

fn read_frag(v: &Value) -> (f64, f64, bool, Option<String>, Value) {
    let w = num(v.get_field("w"));
    let h = num(v.get_field("h"));
    let leaf = matches!(v.get_field("leaf"), Some(Value::Bool(true)));
    let text = v.get_field("text").and_then(|t| t.as_str()).map(str::to_string);
    let el = v.get_field("el").cloned().unwrap_or(Value::None);
    (w, h, leaf, text, el)
}

fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Float(f)) => f.0,
        Some(Value::Int(i)) => *i as f64,
        _ => 0.0,
    }
}

fn text_at(x: f64, y: f64, content: &str, fill: &str, bold: bool) -> Value {
    let mut attrs = vec![
        ("x", Value::float(x)),
        ("y", Value::float(y)),
        ("fill", Value::from(fill)),
        ("font-size", Value::from("12")),
        ("font-family", Value::from("ui-monospace, monospace")),
    ];
    if bold {
        attrs.push(("font-weight", Value::from("600")));
    }
    el("text", attrs, vec![Value::from(content)])
}

fn translate(x: f64, y: f64, child: Value) -> Value {
    el(
        "g",
        vec![("transform", Value::from(format!("translate({x},{y})").as_str()))],
        vec![child],
    )
}

fn box_rect(w: f64, h: f64) -> Value {
    el(
        "rect",
        vec![
            ("x", Value::float(0.0)),
            ("y", Value::float(0.0)),
            ("width", Value::float(w)),
            ("height", Value::float(h)),
            ("rx", Value::float(4.0)),
            ("fill", Value::from(NODE_FILL)),
            ("stroke", Value::from(NODE_STROKE)),
        ],
        vec![],
    )
}

fn scalar_text(v: &Value) -> String {
    match v {
        Value::None => "∅".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format!("{}", f.0),
        Value::String(s) => s.clone(),
        Value::Bytes(b) => format!("<{} bytes>", b.len()),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_schema::Key;

    fn counter() -> Value {
        let mut m = StateMap::new();
        m.insert(Key::from("count"), Value::float(2.0));
        Value::Map(m)
    }

    #[test]
    fn renders_a_bigraph_to_svg() {
        let out = render_bigraph(&counter(), 5.0);
        assert!(out.starts_with("<svg "), "is an svg doc: {out}");
        assert!(out.contains("t = 5"), "shows the clock");
        assert!(out.contains("count = 2"), "shows the leaf inline");
        assert!(out.contains("<rect"), "draws nested boxes");
        assert!(out.ends_with("</svg>"));
    }

    #[test]
    fn structural_render_is_a_functor_over_nesting() {
        // A nested node renders a box-in-box (the place graph as nested regions).
        let mut inner = StateMap::new();
        inner.insert(Key::from("v"), Value::float(1.0));
        let mut outer = StateMap::new();
        outer.insert(Key::from("cell"), Value::Map(inner));
        let out = render_bigraph(&Value::Map(outer), 0.0);
        // outer box + inner box → at least two rects nested.
        assert!(out.matches("<rect").count() >= 2, "nested boxes: {out}");
        assert!(out.contains("cell"), "labels the child node");
    }
}

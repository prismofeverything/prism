//! SVG as a **place-graph value** (#12).
//!
//! An SVG element is just a tree node — `{_type: "<tag>", <attr>: <value>…,
//! children: [...]}` — the same tree-of-maps that *is* a bigraph. So a
//! visualization is **data**: inspectable, composable, diff-able, serializable,
//! and manipulable in `.ys`, not an opaque plotters/graphviz string. [`to_svg`]
//! serializes the place-graph to SVG text; the constructors ([`el`], [`svg`],
//! [`rect`], [`line`], [`text`], [`group`]) build it.
//!
//! The direction (incremental, see #12): every renderer produces one of these
//! values, so `plot(schema, trace)` returns the viz AS a place-graph whose own
//! structure can be viewed / diffed / composed — "the structure of the viz is
//! data." The plotters/graphviz string renderers migrate onto this.

use prism_schema::Value;

/// An SVG node: `el("rect", vec![("x", v(0.0)), ("fill", s("blue"))], vec![])`.
/// `_type` is the tag; remaining fields are attributes; `children` are nested
/// nodes (a `Value::String` child is rendered as raw text content).
pub fn el(tag: &str, attrs: Vec<(&str, Value)>, children: Vec<Value>) -> Value {
    let mut pairs: Vec<(String, Value)> = vec![("_type".to_string(), Value::from(tag))];
    pairs.extend(attrs.into_iter().map(|(k, val)| (k.to_string(), val)));
    if !children.is_empty() {
        pairs.push(("children".to_string(), Value::List(children)));
    }
    Value::tree(pairs)
}

/// Root `<svg>` of the given pixel size (carries the xmlns).
pub fn svg(width: f64, height: f64, children: Vec<Value>) -> Value {
    el(
        "svg",
        vec![
            ("xmlns", s("http://www.w3.org/2000/svg")),
            ("width", v(width)),
            ("height", v(height)),
        ],
        children,
    )
}

/// A `<g>` group of children.
pub fn group(children: Vec<Value>) -> Value {
    el("g", vec![], children)
}

pub fn rect(x: f64, y: f64, w: f64, h: f64, fill: &str) -> Value {
    el("rect", vec![("x", v(x)), ("y", v(y)), ("width", v(w)), ("height", v(h)), ("fill", s(fill))], vec![])
}

pub fn line(x1: f64, y1: f64, x2: f64, y2: f64, stroke: &str) -> Value {
    el(
        "line",
        vec![("x1", v(x1)), ("y1", v(y1)), ("x2", v(x2)), ("y2", v(y2)), ("stroke", s(stroke))],
        vec![],
    )
}

pub fn text(x: f64, y: f64, content: &str) -> Value {
    el("text", vec![("x", v(x)), ("y", v(y))], vec![Value::from(content)])
}

fn v(n: f64) -> Value {
    Value::float(n)
}
fn s(t: &str) -> Value {
    Value::from(t)
}

/// Serialize an SVG place-graph value to an SVG string. A `Value::String` node
/// is raw text content; a `Value::Map` node renders `<tag attr="…">children</tag>`
/// (self-closing when childless). Non-SVG values render empty.
pub fn to_svg(node: &Value) -> String {
    match node {
        Value::String(content) => escape(content),
        Value::Map(m) => {
            let Some(tag) = m.get("_type").and_then(|t| t.as_str()) else {
                return String::new();
            };
            let mut out = format!("<{tag}");
            for (k, val) in m {
                if k == "_type" || k == "children" {
                    continue;
                }
                out.push_str(&format!(" {k}=\"{}\"", attr(val)));
            }
            match m.get("children").and_then(|c| c.as_list()) {
                Some(children) if !children.is_empty() => {
                    out.push('>');
                    for c in children {
                        out.push_str(&to_svg(c));
                    }
                    out.push_str(&format!("</{tag}>"));
                }
                _ => out.push_str("/>"),
            }
            out
        }
        _ => String::new(),
    }
}

/// Render an attribute value (numbers without a trailing `.0`).
fn attr(val: &Value) -> String {
    match val {
        Value::Float(f) => {
            let n = f.0;
            if n.fract() == 0.0 {
                format!("{n:.0}")
            } else {
                format!("{n}")
            }
        }
        Value::Int(i) => i.to_string(),
        Value::String(t) => escape(t),
        other => other.to_string(),
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_is_a_place_graph_value() {
        // The viz is data: a tree-of-maps, navigable like any bigraph state.
        let doc = svg(100.0, 50.0, vec![rect(0.0, 0.0, 100.0, 50.0, "#eee"), text(10.0, 30.0, "hi")]);
        assert_eq!(doc.get_field("_type").and_then(|t| t.as_str()), Some("svg"));
        let kids = doc.get_field("children").and_then(|c| c.as_list()).expect("children");
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0].get_field("_type").and_then(|t| t.as_str()), Some("rect"));
    }

    #[test]
    fn serializes_to_svg_text() {
        let doc = svg(100.0, 50.0, vec![rect(0.0, 0.0, 100.0, 50.0, "#eee"), text(10.0, 30.0, "hi & ok")]);
        let out = to_svg(&doc);
        assert!(out.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"50\">"), "got: {out}");
        assert!(out.contains("<rect x=\"0\" y=\"0\" width=\"100\" height=\"50\" fill=\"#eee\"/>"));
        assert!(out.contains("<text x=\"10\" y=\"30\">hi &amp; ok</text>"), "text content escaped; got: {out}");
        assert!(out.ends_with("</svg>"));
    }

    #[test]
    fn roundtrips_structure_then_render() {
        // Build, mutate as data (append a child), then render — viz-as-data.
        let mut kids = vec![line(0.0, 0.0, 10.0, 10.0, "black")];
        kids.push(rect(1.0, 1.0, 2.0, 2.0, "red"));
        let out = to_svg(&svg(20.0, 20.0, kids));
        assert!(out.contains("<line") && out.contains("<rect"));
    }
}

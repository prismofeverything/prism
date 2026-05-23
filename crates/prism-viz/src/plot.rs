//! `plot(schema, trace) -> Value` — **plotting as a schema-driven operation that
//! returns the viz AS DATA.**
//!
//! A `trace` is a single-item state extended through time (`list[state]`); the
//! **schema** chooses the characteristic view, and the result is an **SVG
//! place-graph value** ([`crate::svg`]) — a tree-of-maps, like any bigraph —
//! NOT an opaque string. So every plot is itself state: inspectable, composable,
//! diff-able, serializable (via [`crate::svg::to_svg`]).
//!
//! - scalars (and maps/trees of scalars) → a **line chart**;
//! - a **field** (`array`/`map[array]`) → a **heatmap** of the latest frame;
//! - otherwise → a small structural **note**.
//!
//! (The schema is the trace's KNOWN type parameter — carried from its producer,
//! never re-inferred here. See memory `feedback_carry_dont_infer_schema`.)

use indexmap::IndexMap;
use prism_schema::{Schema, Value};

use crate::svg::{el, line, polyline, rect, svg, text};

/// The characteristic view for a single-item `schema` extended through time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Lines,
    Field,
    Snapshot,
}

/// Classify how a single-item `schema` characteristically extends through time.
pub fn characteristic(schema: &Schema) -> View {
    match schema {
        Schema::Float { .. } | Schema::Integer { .. } | Schema::Delta { .. } => View::Lines,
        Schema::Array { .. } => View::Field,
        Schema::Map { value } => match value.as_ref() {
            Schema::Array { .. } | Schema::List { .. } => View::Field,
            v if is_scalar(v) => View::Lines,
            _ => View::Snapshot,
        },
        Schema::Tree { branches } => {
            if branches.values().any(has_field) {
                View::Field
            } else if branches.values().any(is_scalar_ish) {
                View::Lines
            } else {
                View::Snapshot
            }
        }
        _ => View::Snapshot,
    }
}

fn is_scalar(s: &Schema) -> bool {
    matches!(s, Schema::Float { .. } | Schema::Integer { .. } | Schema::Delta { .. })
}
fn is_scalar_ish(s: &Schema) -> bool {
    is_scalar(s) || matches!(s, Schema::Map { value } if is_scalar(value))
}
fn has_field(s: &Schema) -> bool {
    match s {
        Schema::Array { .. } => true,
        Schema::Map { value } | Schema::List { element: value } => {
            matches!(value.as_ref(), Schema::Array { .. })
        }
        _ => false,
    }
}

/// Render `trace` as the characteristic SVG **place-graph value** for its
/// `schema`. Serialize with [`crate::svg::to_svg`].
pub fn plot(schema: &Schema, trace: &[Value], title: &str) -> Value {
    match characteristic(schema) {
        View::Lines => line_chart(trace, title),
        View::Field => heatmap(trace, title),
        View::Snapshot => svg(
            420.0,
            60.0,
            vec![text(12.0, 34.0, &format!("{title} — {} frames", trace.len()))],
        ),
    }
}

const W: f64 = 800.0;
const H: f64 = 450.0;
const ML: f64 = 60.0;
const MR: f64 = 140.0; // room for the legend
const MT: f64 = 40.0;
const MB: f64 = 40.0;

const PALETTE: [&str; 6] = ["#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd", "#8c564b"];

/// A line chart of the scalar series in `trace`, as an SVG place-graph.
fn line_chart(trace: &[Value], title: &str) -> Value {
    let series = transpose(trace);
    let n = trace.len();
    let pw = W - ML - MR;
    let ph = H - MT - MB;

    let (mut ymin, mut ymax) = (f64::MAX, f64::MIN);
    for col in series.values() {
        for &v in col {
            ymin = ymin.min(v);
            ymax = ymax.max(v);
        }
    }
    if !ymin.is_finite() || !ymax.is_finite() {
        (ymin, ymax) = (0.0, 1.0);
    }
    let span = (ymax - ymin).abs().max(1e-9);
    let xpx = |i: usize| ML + if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 } * pw;
    let ypx = |v: f64| MT + (1.0 - (v - ymin) / span) * ph;

    let mut kids = vec![
        rect(0.0, 0.0, W, H, "#ffffff"),
        el("text", vec![("x", fl(W / 2.0)), ("y", fl(22.0)), ("text-anchor", st("middle")), ("font-family", st("sans-serif")), ("font-size", fl(14.0)), ("font-weight", st("600"))], vec![Value::from(title)]),
        line(ML, MT, ML, MT + ph, "#333"),         // y axis
        line(ML, MT + ph, ML + pw, MT + ph, "#333"), // x axis
        axis_label(ML - 6.0, ypx(ymax), &fmt(ymax), "end"),
        axis_label(ML - 6.0, ypx(ymin), &fmt(ymin), "end"),
    ];
    for (i, (name, col)) in series.iter().enumerate() {
        let color = PALETTE[i % PALETTE.len()];
        let points: Vec<(f64, f64)> = col.iter().enumerate().map(|(j, &v)| (xpx(j), ypx(v))).collect();
        kids.push(polyline(&points, color, 1.5));
        // legend
        kids.push(rect(ML + pw + 12.0, MT + 6.0 + i as f64 * 18.0, 10.0, 10.0, color));
        kids.push(legend_text(ML + pw + 26.0, MT + 15.0 + i as f64 * 18.0, name));
    }
    svg(W, H, kids)
}

/// An **animated** heatmap of a field trace, as an SVG place-graph: one cell per
/// grid point, each cell's `fill` animating through its value at every frame
/// (SMIL `<animate>`). The whole trace drives it; a single frame renders static.
fn heatmap(trace: &[Value], title: &str) -> Value {
    let cell = 40.0;
    let pad = 30.0;
    // One flattened grid per frame.
    let grids: Vec<Vec<f64>> = trace.iter().filter_map(first_field).collect();
    let Some(first) = grids.first() else {
        return svg(200.0, 60.0, vec![legend_text(12.0, 34.0, &format!("{title} (no field)"))]);
    };
    let n = first.len();
    let side = (n as f64).sqrt().round().max(1.0) as usize;
    // Global color scale across ALL frames, so the animation is comparable.
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for g in &grids {
        for &v in g {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    let span = (hi - lo).abs().max(1e-9);
    let shade = |v: f64| {
        let t = ((v - lo) / span).clamp(0.0, 1.0);
        let c = (255.0 * (1.0 - t)) as u8;
        format!("#{c:02x}{c:02x}ff") // white → blue ramp
    };
    let dur = (grids.len() as f64 * 0.4).max(0.4);

    let w = pad * 2.0 + side as f64 * cell;
    let h = 24.0 + pad + side as f64 * cell;
    let mut kids = vec![rect(0.0, 0.0, w, h, "#ffffff"), legend_text(pad, 18.0, title)];
    for idx in 0..n {
        let (r, c) = (idx / side, idx % side);
        let (x, y) = (pad + c as f64 * cell, 24.0 + r as f64 * cell);
        let frame0 = shade(first[idx]);
        if grids.len() > 1 {
            // The cell's fill animates through its value at each frame — the
            // trace itself, as a child node (animation is place-graph data).
            let values: Vec<String> = grids.iter().map(|g| shade(*g.get(idx).unwrap_or(&lo))).collect();
            kids.push(el(
                "rect",
                vec![("x", fl(x)), ("y", fl(y)), ("width", fl(cell - 1.0)), ("height", fl(cell - 1.0)), ("fill", st(&frame0))],
                vec![crate::svg::animate("fill", &values, dur)],
            ));
        } else {
            kids.push(rect(x, y, cell - 1.0, cell - 1.0, &frame0));
        }
    }
    svg(w, h, kids)
}

/// Transpose a scalar trace into named series (flattening nested maps to dotted
/// keys; non-scalar leaves skipped). Frames index time 0..n.
fn transpose(trace: &[Value]) -> IndexMap<String, Vec<f64>> {
    let mut series: IndexMap<String, Vec<f64>> = IndexMap::new();
    for (i, frame) in trace.iter().enumerate() {
        collect_scalars(frame, "", &mut |key, v| {
            series.entry(key.to_string()).or_insert_with(|| vec![0.0; trace.len()])[i] = v;
        });
    }
    series
}

fn collect_scalars(v: &Value, prefix: &str, emit: &mut impl FnMut(&str, f64)) {
    match v {
        Value::Float(_) | Value::Int(_) => {
            if let Some(f) = v.as_f64() {
                emit(prefix, f);
            }
        }
        Value::Map(m) => {
            for (k, val) in m {
                if k.as_str() == "_type" {
                    continue;
                }
                let key = if prefix.is_empty() { k.to_string() } else { format!("{prefix}.{k}") };
                collect_scalars(val, &key, emit);
            }
        }
        _ => {}
    }
}

/// The first list-of-floats field in a frame (a flattened spatial grid).
fn first_field(frame: &Value) -> Option<Vec<f64>> {
    let m = frame.as_map()?;
    for (_k, v) in m {
        if let Some(list) = v.as_list() {
            let floats: Vec<f64> = list.iter().filter_map(|x| x.as_f64()).collect();
            if floats.len() == list.len() && !floats.is_empty() {
                return Some(floats);
            }
        }
    }
    None
}

fn axis_label(x: f64, y: f64, s: &str, anchor: &str) -> Value {
    el(
        "text",
        vec![("x", fl(x)), ("y", fl(y)), ("text-anchor", st(anchor)), ("font-family", st("sans-serif")), ("font-size", fl(10.0)), ("fill", st("#666"))],
        vec![Value::from(s)],
    )
}
fn legend_text(x: f64, y: f64, s: &str) -> Value {
    el(
        "text",
        vec![("x", fl(x)), ("y", fl(y)), ("font-family", st("sans-serif")), ("font-size", fl(11.0))],
        vec![Value::from(s)],
    )
}
fn fl(n: f64) -> Value {
    Value::float(n)
}
fn st(t: &str) -> Value {
    Value::from(t)
}
fn fmt(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{n:.0}")
    } else {
        format!("{n:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::to_svg;
    use indexmap::IndexMap as IM;

    fn float_map(pairs: &[(&str, f64)]) -> Value {
        Value::Map(pairs.iter().map(|(k, v)| ((*k).into(), Value::float(*v))).collect())
    }

    #[test]
    fn schema_chooses_the_view() {
        assert_eq!(characteristic(&Schema::float()), View::Lines);
        assert_eq!(characteristic(&Schema::map(Schema::float())), View::Lines);
        let field = Schema::Map { value: Box::new(Schema::Array { shape: vec![3, 3], element: Box::new(Schema::float()) }) };
        assert_eq!(characteristic(&field), View::Field);
    }

    #[test]
    fn line_plot_is_a_place_graph_value() {
        let trace = vec![float_map(&[("a", 0.1)]), float_map(&[("a", 1.0)]), float_map(&[("a", 2.5)])];
        let doc = plot(&Schema::map(Schema::float()), &trace, "growth");
        // The plot is DATA: a navigable svg place-graph.
        assert_eq!(doc.get_field("_type").and_then(|t| t.as_str()), Some("svg"));
        let kids = doc.get_field("children").and_then(|c| c.as_list()).expect("children");
        assert!(kids.iter().any(|k| k.get_field("_type").and_then(|t| t.as_str()) == Some("polyline")), "has a data line");
        // …and serializes to an SVG string.
        let s = to_svg(&doc);
        assert!(s.starts_with("<svg") && s.contains("<polyline") && s.contains("growth"));
    }

    #[test]
    fn field_view_is_an_animated_heatmap_place_graph() {
        let frame = |base: f64| {
            Value::Map(IM::from([(
                "glucose".into(),
                Value::List((0..9).map(|i| Value::float(base + i as f64)).collect()),
            )]))
        };
        let field = Schema::Map { value: Box::new(Schema::Array { shape: vec![3, 3], element: Box::new(Schema::float()) }) };
        // Two frames ⇒ each cell animates.
        let doc = plot(&field, &[frame(0.0), frame(5.0)], "diffusion");
        assert_eq!(doc.get_field("_type").and_then(|t| t.as_str()), Some("svg"));
        let kids = doc.get_field("children").and_then(|c| c.as_list()).unwrap();
        let cells = kids.iter().filter(|k| k.get_field("_type").and_then(|t| t.as_str()) == Some("rect")).count();
        assert!(cells >= 9, "3x3 grid (+bg); got {cells}");
        // The animation is itself place-graph data: an <animate> child of a cell.
        let s = to_svg(&doc);
        assert!(s.contains("<animate attributeName=\"fill\""), "cells animate over the trace; got:\n{s}");
    }
}

//! `plot(schema, trace)` — **plotting as a schema-driven operation.**
//!
//! A `trace` is a single-item state *extended through time*: a `list[state]`,
//! one frame per timestep. The **schema** of that single item chooses the
//! characteristic visualization — exactly as the schema algebra dispatches
//! `serialize`/`apply`/`divide` on schema:
//!
//! - scalars (and maps/trees of scalars) → a **line plot** over time
//!   (`map[float]`-over-time, the integrator trace, is just this common case);
//! - a **field** (`array` / `map[array]`) → a heatmap/animation of the frames;
//! - anything else → a structural **snapshot** of the latest frame.
//!
//! The per-type renderers already live in this crate; `plot` is the dispatcher
//! over them. ("Derive the canonical operation from the type.")

use indexmap::IndexMap;
use prism_schema::{Schema, Value};

use crate::mol_svg::render_cell_snapshot_svg;
use crate::timeseries::render_timeseries_svg;

/// The characteristic view for a single-item `schema` extended through time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Scalars over time → a line plot.
    Lines,
    /// A spatial field over time → a heatmap / animation.
    Field,
    /// Otherwise → a structural snapshot of the latest frame.
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

/// Render `trace` (a single-item state sampled over time) as the characteristic
/// SVG for its `schema`, titled `title`.
pub fn plot(schema: &Schema, trace: &[Value], title: &str) -> String {
    match characteristic(schema) {
        View::Lines => lines(trace, title),
        // TODO: a true field heatmap/animation renderer (port report.rs's
        // plotters heatmap into prism-viz); snapshot the latest frame for now.
        View::Field | View::Snapshot => {
            render_cell_snapshot_svg(trace.last().unwrap_or(&Value::None), title)
        }
    }
}

/// Transpose a scalar `trace` into named series (flattening nested maps to dotted
/// keys, e.g. `substrates.glucose`; non-scalar leaves are skipped) and render a
/// line plot. Frames index time 0..n.
fn lines(trace: &[Value], title: &str) -> String {
    let times: Vec<f64> = (0..trace.len()).map(|i| i as f64).collect();
    let mut series: IndexMap<String, Vec<f64>> = IndexMap::new();
    for (i, frame) in trace.iter().enumerate() {
        collect_scalars(frame, "", &mut |key, v| {
            series.entry(key.to_string()).or_insert_with(|| vec![0.0; trace.len()])[i] = v;
        });
    }
    render_timeseries_svg(title, &times, &series, false)
}

/// Visit each scalar leaf of `v`, calling `emit(dotted_key, value)`.
fn collect_scalars(v: &Value, prefix: &str, emit: &mut impl FnMut(&str, f64)) {
    match v {
        Value::Float(_) | Value::Int(_) => {
            if let Some(f) = v.as_f64() {
                emit(prefix, f);
            }
        }
        Value::Map(m) => {
            for (k, val) in m {
                let key =
                    if prefix.is_empty() { k.to_string() } else { format!("{prefix}.{k}") };
                collect_scalars(val, &key, emit);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // A composite tree of scalars (+ a non-scalar process branch) → Lines.
        let tree = Schema::Tree {
            branches: IM::from([
                ("biomass".into(), Schema::float()),
                ("substrates".into(), Schema::map(Schema::float())),
                ("proc".into(), Schema::Any),
            ]),
        };
        assert_eq!(characteristic(&tree), View::Lines);
    }

    #[test]
    fn scalar_trace_renders_a_line_plot_svg() {
        // biomass grows, glucose falls over 4 frames.
        let trace = vec![
            float_map(&[("biomass", 0.1), ("glucose", 10.0)]),
            float_map(&[("biomass", 1.0), ("glucose", 7.0)]),
            float_map(&[("biomass", 2.5), ("glucose", 3.0)]),
            float_map(&[("biomass", 4.0), ("glucose", 0.0)]),
        ];
        let svg = plot(&Schema::map(Schema::float()), &trace, "growth");
        assert!(svg.contains("<svg"), "produced an SVG");
        assert!(svg.contains("growth"), "title rendered");
    }

    #[test]
    fn nested_scalars_flatten_to_dotted_series() {
        // {biomass, substrates:{glucose}} over time → biomass + substrates.glucose.
        let frame = |b: f64, g: f64| {
            Value::Map(IM::from([
                ("biomass".into(), Value::float(b)),
                ("substrates".into(), float_map(&[("glucose", g)])),
            ]))
        };
        let trace = vec![frame(0.1, 10.0), frame(2.0, 5.0), frame(4.0, 0.0)];
        let mut series: IndexMap<String, Vec<f64>> = IndexMap::new();
        for (i, f) in trace.iter().enumerate() {
            collect_scalars(f, "", &mut |k, v| {
                series.entry(k.to_string()).or_insert_with(|| vec![0.0; trace.len()])[i] = v;
            });
        }
        assert!(series.contains_key("biomass"));
        assert!(series.contains_key("substrates.glucose"), "nested key flattened; got {:?}", series.keys().collect::<Vec<_>>());
        assert_eq!(series["substrates.glucose"], vec![10.0, 5.0, 0.0]);
    }
}

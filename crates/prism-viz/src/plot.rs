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
    /// Spatial particles (a map of records with a `position`) → an animated scatter.
    Particles,
    /// Both a field AND particles (a comet) → field heatmap with particles overlaid.
    Spatial,
    Snapshot,
}

/// Classify how a single-item `schema` characteristically extends through time.
pub fn characteristic(schema: &Schema) -> View {
    match schema {
        Schema::Float { .. } | Schema::Integer { .. } | Schema::Delta { .. } => View::Lines,
        Schema::Array { .. } => View::Field,
        Schema::Map { value } => match value.as_ref() {
            Schema::Array { .. } | Schema::List { .. } => View::Field,
            // A map of records carrying a `position` → spatial particles.
            Schema::Tree { branches } if branches.contains_key("position") => View::Particles,
            v if is_scalar(v) => View::Lines,
            _ => View::Snapshot,
        },
        Schema::Tree { branches } => {
            let field = branches.values().any(has_field);
            let particles = branches.values().any(has_particles);
            if field && particles {
                View::Spatial
            } else if field {
                View::Field
            } else if particles {
                View::Particles
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
/// A schema branch that is a map of `position`-bearing records (particles).
fn has_particles(s: &Schema) -> bool {
    matches!(s, Schema::Map { value }
        if matches!(value.as_ref(), Schema::Tree { branches } if branches.contains_key("position")))
}

/// Render `trace` as the characteristic SVG **place-graph value** for its
/// `schema`. Serialize with [`crate::svg::to_svg`].
pub fn plot(schema: &Schema, trace: &[Value], title: &str) -> Value {
    match characteristic(schema) {
        View::Lines => line_chart(trace, title),
        View::Field => heatmap(trace, title),
        View::Particles => scatter(trace, title),
        View::Spatial => spatial(trace, title),
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
    let times: Vec<f64> = (0..n).map(|i| i as f64).collect();
    time_series_chart(&times, &series, title, false)
}

/// A line chart with explicit time stamps + named scalar series, as an SVG
/// place-graph value. Supports auto- and forced-log y-scale (used by the
/// spatio-flux report) and field-name color hinting (glucose blue, biomass
/// green, etc. — matching the spatio-flux palette). Public so callers that
/// have pre-aggregated `(times, series)` data can plot directly without
/// re-routing through a synthetic trace.
pub fn time_series_chart(
    times: &[f64],
    series: &IndexMap<String, Vec<f64>>,
    title: &str,
    force_log: bool,
) -> Value {
    let pw = W - ML - MR;
    let ph = H - MT - MB;

    let t_min = times.first().copied().unwrap_or(0.0);
    let t_max = times.last().copied().unwrap_or(1.0).max(t_min + 1e-9);

    // Pick log y-axis when multi-scale (per-series maxima span ≥1000x median).
    let mut series_maxes: Vec<f64> = series
        .values()
        .map(|s| s.iter().copied().fold(0.0_f64, f64::max))
        .filter(|&m| m > 0.0)
        .collect();
    series_maxes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let auto_log = series_maxes.len() >= 2 && {
        let median = series_maxes[series_maxes.len() / 2];
        *series_maxes.last().unwrap() / median > 1000.0
    };
    let use_log = force_log || auto_log;

    let (raw_min, raw_max) = series.values().flat_map(|s| s.iter()).copied().fold(
        (f64::MAX, f64::MIN),
        |(lo, hi), v| (lo.min(v), hi.max(v)),
    );
    let pos_min = series
        .values()
        .flat_map(|s| s.iter())
        .copied()
        .filter(|v| *v > 0.0)
        .fold(f64::MAX, f64::min);
    let (ymin, ymax) = if !raw_min.is_finite() || !raw_max.is_finite() {
        (0.0, 1.0)
    } else if use_log {
        let lo = if pos_min.is_finite() { pos_min.log10().floor() } else { 0.0 };
        let hi = (raw_max.max(0.01) * 1.2).log10().ceil();
        (lo, hi.max(lo + 1e-9))
    } else {
        (0.0, raw_max.max(0.01) * 1.15)
    };
    let span = (ymax - ymin).abs().max(1e-9);
    let ymap = |v: f64| {
        if use_log {
            if v > 0.0 { v.log10() } else { ymin }
        } else {
            v
        }
    };
    let xpx = |t: f64| ML + (t - t_min) / (t_max - t_min) * pw;
    let ypx = |v: f64| MT + (1.0 - (ymap(v) - ymin) / span) * ph;

    let header = if use_log { format!("{title} (log scale)") } else { title.to_string() };
    let y_axis_desc = if use_log { "log10(value)" } else { "value" };

    let mut kids = vec![
        rect(0.0, 0.0, W, H, "#ffffff"),
        el(
            "text",
            vec![
                ("x", fl(W / 2.0)),
                ("y", fl(22.0)),
                ("text-anchor", st("middle")),
                ("font-family", st("sans-serif")),
                ("font-size", fl(14.0)),
                ("font-weight", st("600")),
            ],
            vec![Value::from(header.as_str())],
        ),
        line(ML, MT, ML, MT + ph, "#333"),
        line(ML, MT + ph, ML + pw, MT + ph, "#333"),
        axis_label(ML - 6.0, ypx(raw_max.max(0.01)), &fmt(ymax), "end"),
        axis_label(ML - 6.0, MT + ph, &fmt(ymin), "end"),
        axis_label(ML + pw / 2.0, H - 8.0, "time", "middle"),
        el(
            "text",
            vec![
                ("x", fl(16.0)),
                ("y", fl(MT + ph / 2.0)),
                ("text-anchor", st("middle")),
                ("font-family", st("sans-serif")),
                ("font-size", fl(10.0)),
                ("fill", st("#666")),
                ("transform", st(&format!("rotate(-90 16 {})", MT + ph / 2.0))),
            ],
            vec![Value::from(y_axis_desc)],
        ),
    ];
    for (i, (name, col)) in series.iter().enumerate() {
        let color = series_color(name, i);
        let points: Vec<(f64, f64)> = times
            .iter()
            .zip(col.iter())
            .map(|(&t, &v)| (xpx(t), ypx(v)))
            .collect();
        kids.push(polyline(&points, color, 1.5));
        kids.push(rect(ML + pw + 12.0, MT + 6.0 + i as f64 * 18.0, 10.0, 10.0, color));
        kids.push(legend_text(ML + pw + 26.0, MT + 15.0 + i as f64 * 18.0, name));
    }
    svg(W, H, kids)
}

/// Field-name → color hint, matching the spatio-flux palette so reports stay
/// visually consistent across the two plotting paths. `prefix` matching keeps
/// it lenient (e.g. `"glucose (probe 0,0)"` still maps to glucose-blue).
fn series_color(name: &str, idx: usize) -> &'static str {
    if name.starts_with("glucose") {
        "#1f77b4"
    } else if name.starts_with("acetate") {
        "#ff7f0e"
    } else if name.starts_with("dissolved biomass") {
        "#17becf"
    } else if name.starts_with("biomass")
        || name.starts_with("dfba_biomass")
        || name.starts_with("ecoli core biomass")
        || name.starts_with("ecoli_core")
        || name.starts_with("monod_biomass")
        || name.starts_with("mass")
    {
        "#2ca02c"
    } else if name.starts_with("formate") {
        "#9467bd"
    } else if name.starts_with("ammonium") {
        "#bcbd22"
    } else if name.starts_with("kinetic_biomass") {
        "#1b9e77"
    } else {
        PALETTE[idx % PALETTE.len()]
    }
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

/// A scatter of particle positions over time, as an SVG place-graph: one circle
/// per particle, its cx/cy animating through the trajectory (SMIL `<animate>`),
/// radius scaled by mass. The whole trace drives it; a single frame is static.
fn scatter(trace: &[Value], title: &str) -> Value {
    let frames: Vec<IndexMap<String, (f64, f64, f64)>> = trace.iter().map(particle_dots).collect();
    let (mut xlo, mut xhi, mut ylo, mut yhi, mut mmax) =
        (f64::MAX, f64::MIN, f64::MAX, f64::MIN, 1e-9_f64);
    for f in &frames {
        for &(x, y, m) in f.values() {
            xlo = xlo.min(x);
            xhi = xhi.max(x);
            ylo = ylo.min(y);
            yhi = yhi.max(y);
            mmax = mmax.max(m);
        }
    }
    if !xlo.is_finite() {
        return svg(220.0, 60.0, vec![legend_text(12.0, 34.0, &format!("{title} (no particles)"))]);
    }
    let (w, h, pad) = (360.0, 360.0, 30.0);
    let sx = |x: f64| pad + (if xhi > xlo { (x - xlo) / (xhi - xlo) } else { 0.5 }) * (w - 2.0 * pad);
    let sy = |y: f64| h - pad - (if yhi > ylo { (y - ylo) / (yhi - ylo) } else { 0.5 }) * (h - 2.0 * pad);
    let dur = (frames.len() as f64 * 0.4).max(0.4);

    // Union of particle ids (first-appearance order) → one animated circle each.
    let mut pids: Vec<String> = Vec::new();
    for f in &frames {
        for pid in f.keys() {
            if !pids.iter().any(|p| p == pid) {
                pids.push(pid.clone());
            }
        }
    }

    let mut kids = vec![rect(0.0, 0.0, w, h, "#ffffff"), legend_text(pad, 18.0, title)];
    for pid in &pids {
        // The particle's trajectory (carry the last-seen position forward across
        // frames where it is absent), and a radius from its mass.
        let mut traj: Vec<(f64, f64)> = Vec::with_capacity(frames.len());
        let (mut cx, mut cy, mut r) = (xlo, ylo, 3.0);
        for f in &frames {
            if let Some(&(x, y, m)) = f.get(pid) {
                cx = x;
                cy = y;
                r = (3.0 + 6.0 * (m / mmax)).max(2.0);
            }
            traj.push((sx(cx), sy(cy)));
        }
        let (x0, y0) = traj[0];
        let attrs = vec![
            ("cx", fl(x0)),
            ("cy", fl(y0)),
            ("r", fl(r)),
            ("fill", st("#1f77b4")),
            ("fill-opacity", st("0.7")),
        ];
        let children = if frames.len() > 1 {
            let xs: Vec<String> = traj.iter().map(|(x, _)| fmt(*x)).collect();
            let ys: Vec<String> = traj.iter().map(|(_, y)| fmt(*y)).collect();
            vec![crate::svg::animate("cx", &xs, dur), crate::svg::animate("cy", &ys, dur)]
        } else {
            vec![]
        };
        kids.push(el("circle", attrs, children));
    }
    svg(w, h, kids)
}

/// A COMET view: the field heatmap with the particle scatter overlaid (agents in
/// a diffusing field). Field cells animate (blue ramp); particles are red dots
/// scaled to the grid area, animating their cx/cy. The two spatial layers in one
/// place-graph. Falls back to a plain scatter if there is no field.
fn spatial(trace: &[Value], title: &str) -> Value {
    let grids: Vec<Vec<f64>> = trace.iter().filter_map(first_field).collect();
    let Some(first) = grids.first() else {
        return scatter(trace, title);
    };
    let side = (first.len() as f64).sqrt().round().max(1.0) as usize;
    let (cell, pad) = (48.0, 30.0);
    let gw = side as f64 * cell;
    let (w, h) = (pad * 2.0 + gw, 24.0 + pad + gw);

    // Field color scale across all frames.
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
        format!("#{c:02x}{c:02x}ff")
    };
    let dur = (grids.len() as f64 * 0.4).max(0.4);

    let mut kids = vec![rect(0.0, 0.0, w, h, "#ffffff"), legend_text(pad, 18.0, title)];
    // Field heatmap (animated cells).
    for idx in 0..first.len() {
        let (r, c) = (idx / side, idx % side);
        let (x, y) = (pad + c as f64 * cell, 24.0 + r as f64 * cell);
        let f0 = shade(first[idx]);
        if grids.len() > 1 {
            let vals: Vec<String> = grids.iter().map(|g| shade(*g.get(idx).unwrap_or(&lo))).collect();
            kids.push(el(
                "rect",
                vec![("x", fl(x)), ("y", fl(y)), ("width", fl(cell)), ("height", fl(cell)), ("fill", st(&f0))],
                vec![crate::svg::animate("fill", &vals, dur)],
            ));
        } else {
            kids.push(rect(x, y, cell, cell, &f0));
        }
    }

    // Particle overlay: positions scaled to the grid area (red dots over the field).
    let pframes: Vec<IndexMap<String, (f64, f64, f64)>> = trace.iter().map(particle_dots).collect();
    let (mut xlo, mut xhi, mut ylo, mut yhi, mut mmax) =
        (f64::MAX, f64::MIN, f64::MAX, f64::MIN, 1e-9_f64);
    for f in &pframes {
        for &(x, y, m) in f.values() {
            xlo = xlo.min(x);
            xhi = xhi.max(x);
            ylo = ylo.min(y);
            yhi = yhi.max(y);
            mmax = mmax.max(m);
        }
    }
    if xlo.is_finite() {
        let sx = |x: f64| pad + (if xhi > xlo { (x - xlo) / (xhi - xlo) } else { 0.5 }) * gw;
        let sy = |y: f64| 24.0 + (if yhi > ylo { 1.0 - (y - ylo) / (yhi - ylo) } else { 0.5 }) * gw;
        let mut pids: Vec<String> = Vec::new();
        for f in &pframes {
            for pid in f.keys() {
                if !pids.iter().any(|p| p == pid) {
                    pids.push(pid.clone());
                }
            }
        }
        for pid in &pids {
            let mut traj: Vec<(f64, f64)> = Vec::with_capacity(pframes.len());
            let (mut cx, mut cy, mut r) = (xlo, ylo, 4.0);
            for f in &pframes {
                if let Some(&(x, y, m)) = f.get(pid) {
                    cx = x;
                    cy = y;
                    r = (4.0 + 6.0 * (m / mmax)).max(2.0);
                }
                traj.push((sx(cx), sy(cy)));
            }
            let (x0, y0) = traj[0];
            let attrs = vec![
                ("cx", fl(x0)),
                ("cy", fl(y0)),
                ("r", fl(r)),
                ("fill", st("#d62728")),
                ("stroke", st("#ffffff")),
                ("stroke-width", fl(1.0)),
            ];
            let children = if pframes.len() > 1 {
                let xs: Vec<String> = traj.iter().map(|(x, _)| fmt(*x)).collect();
                let ys: Vec<String> = traj.iter().map(|(_, y)| fmt(*y)).collect();
                vec![crate::svg::animate("cx", &xs, dur), crate::svg::animate("cy", &ys, dur)]
            } else {
                vec![]
            };
            kids.push(el("circle", attrs, children));
        }
    }
    svg(w, h, kids)
}

/// Per-frame particle dots: `pid -> (x, y, mass)`, digging to the particles map.
fn particle_dots(frame: &Value) -> IndexMap<String, (f64, f64, f64)> {
    let mut out = IndexMap::new();
    collect_particles(frame, &mut out);
    out
}

/// Find the particles map (values carry a `position`), digging through named
/// ports, and fill `out` with each particle's `(x, y, mass)`. Returns whether a
/// particles map was found at this node.
fn collect_particles(frame: &Value, out: &mut IndexMap<String, (f64, f64, f64)>) -> bool {
    let Some(m) = frame.as_map() else { return false };
    if m.iter().any(|(_k, v)| v.get_field("position").is_some()) {
        for (pid, p) in m {
            if let Some((x, y)) = position_xy(p) {
                let mass = p.get_field("mass").and_then(|v| v.as_f64()).unwrap_or(1.0);
                out.insert(pid.to_string(), (x, y, mass));
            }
        }
        return true;
    }
    for (_k, v) in m {
        if collect_particles(v, out) {
            return true;
        }
    }
    false
}

/// A particle's `[x, y]` position, if present.
fn position_xy(p: &Value) -> Option<(f64, f64)> {
    let pos = p.get_field("position")?.as_list()?;
    Some((pos.first()?.as_f64()?, pos.get(1)?.as_f64()?))
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
    // A grid: flat floats, or rows-of-floats (row-major, e.g. [[a,b,c],[d,e,f],…]).
    if let Some(items) = frame.as_list() {
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            if let Some(f) = item.as_f64() {
                out.push(f);
            } else if let Some(row) = item.as_list() {
                out.extend(row.iter().filter_map(|c| c.as_f64()));
            } else {
                return None; // not a numeric grid
            }
        }
        return (!out.is_empty()).then_some(out);
    }
    // Dig through named ports — the spatial state's field lives under one, e.g.
    // `{fields: {glucose: [...]}}`. Return the first grid found.
    if let Some(m) = frame.as_map() {
        for (k, v) in m {
            if k.as_str() == "_type" {
                continue;
            }
            if let Some(grid) = first_field(v) {
                return Some(grid);
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

    #[test]
    fn nested_port_field_plots_as_heatmap() {
        // Regression (diffusion-section): a spatial field nested under a named port
        // — `{fields: {glucose: [...]}}`, the real engine-state shape — must be
        // found by first_field and rendered as a heatmap, not the "(no field)"
        // placeholder. The element is a Tree carrying the array type.
        let frame = |base: f64| {
            Value::Map(IM::from([(
                "fields".into(),
                Value::Map(IM::from([(
                    "glucose".into(),
                    Value::List((0..9).map(|i| Value::float(base + i as f64)).collect()),
                )])),
            )]))
        };
        let elem = Schema::Tree {
            branches: IM::from([(
                "fields".into(),
                Schema::Map {
                    value: Box::new(Schema::Array { shape: vec![3, 3], element: Box::new(Schema::float()) }),
                },
            )]),
        };
        assert_eq!(characteristic(&elem), View::Field, "a tree with an array branch is a field");
        let doc = plot(&elem, &[frame(0.0), frame(5.0)], "glucose");
        let s = to_svg(&doc);
        assert!(!s.contains("no field"), "the nested field is found, not missed:\n{s}");
        assert!(s.contains("<animate attributeName=\"fill\""), "cells animate over the trace");
        let cells = doc
            .get_field("children")
            .and_then(|c| c.as_list())
            .unwrap()
            .iter()
            .filter(|k| k.get_field("_type").and_then(|t| t.as_str()) == Some("rect"))
            .count();
        assert!(cells >= 9, "3x3 grid (+bg); got {cells}");
    }

    #[test]
    fn particles_plot_as_an_animated_scatter() {
        // A map of `position`-bearing records → View::Particles → a scatter of
        // circles, each animating cx/cy over the trace (the delta-motion section).
        let elem = Schema::Tree {
            branches: IM::from([(
                "particles".into(),
                Schema::Map {
                    value: Box::new(Schema::Tree {
                        branches: IM::from([(
                            "position".into(),
                            Schema::Array { shape: vec![2], element: Box::new(Schema::float()) },
                        )]),
                    }),
                },
            )]),
        };
        assert_eq!(characteristic(&elem), View::Particles, "a map of position-records is particles");
        let frame = |x: f64, y: f64| {
            Value::Map(IM::from([(
                "particles".into(),
                Value::Map(IM::from([(
                    "p1".into(),
                    Value::tree([
                        ("position", Value::List(vec![Value::float(x), Value::float(y)])),
                        ("mass", Value::float(0.5)),
                    ]),
                )])),
            )]))
        };
        let doc = plot(&elem, &[frame(10.0, 10.0), frame(20.0, 30.0)], "particles");
        let s = to_svg(&doc);
        assert!(!s.contains("no particles"), "the particle is found:\n{s}");
        assert!(s.contains("<circle"), "a circle per particle");
        assert!(
            s.contains("<animate attributeName=\"cx\"") && s.contains("attributeName=\"cy\""),
            "cx/cy animate over the trace:\n{s}"
        );
    }

    #[test]
    fn comet_plots_field_and_particles_overlaid() {
        // A {fields, particles} element → View::Spatial → field heatmap (rects) WITH
        // the particle scatter (red circles) overlaid in one place-graph.
        let vec2 = || Schema::Array { shape: vec![2], element: Box::new(Schema::float()) };
        let elem = Schema::Tree {
            branches: IM::from([
                (
                    "fields".into(),
                    Schema::Map {
                        value: Box::new(Schema::Array { shape: vec![2, 2], element: Box::new(Schema::float()) }),
                    },
                ),
                (
                    "particles".into(),
                    Schema::Map {
                        value: Box::new(Schema::Tree { branches: IM::from([("position".into(), vec2())]) }),
                    },
                ),
            ]),
        };
        assert_eq!(characteristic(&elem), View::Spatial, "field + particles is a comet");
        let frame = |base: f64| {
            Value::Map(IM::from([
                (
                    "fields".into(),
                    Value::Map(IM::from([(
                        "glucose".into(),
                        Value::List((0..4).map(|i| Value::float(base + i as f64)).collect()),
                    )])),
                ),
                (
                    "particles".into(),
                    Value::Map(IM::from([(
                        "p1".into(),
                        Value::tree([("position", Value::List(vec![Value::float(base), Value::float(1.0)]))]),
                    )])),
                ),
            ]))
        };
        let doc = plot(&elem, &[frame(0.0), frame(2.0)], "comet");
        let s = to_svg(&doc);
        assert!(s.contains("<rect"), "field heatmap cells");
        assert!(s.contains("<circle"), "particles overlaid");
        assert!(s.contains("#d62728"), "particles are red dots over the field");
    }
}

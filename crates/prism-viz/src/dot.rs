//! DOT format generation from bigraph structures.
//!
//! Produces Graphviz digraph source that can be rendered with `dot`:
//! - State nodes as circles
//! - Process/step nodes as boxes
//! - Input wiring as dashed forward arrows
//! - Output wiring as dashed back arrows
//! - Bidirectional wiring as dashed both arrows
//! - Hierarchical containment as solid edges

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use prism_bigraph::document::Document;
use prism_bigraph::topology::Topology;
use prism_schema::{Key, Value};

/// Options for DOT rendering.
#[derive(Clone, Debug)]
pub struct DotOptions {
    /// Graph layout direction: "TB" (top-bottom), "LR" (left-right).
    pub rankdir: String,
    /// Canvas size in inches (width, height).
    pub size: (f64, f64),
    /// DPI for raster output.
    pub dpi: u32,
    /// Node label font size in points.
    pub node_label_size: u32,
    /// Process label font size in points.
    pub process_label_size: u32,
    /// Whether to show state values in node labels.
    pub show_values: bool,
    /// Whether to show type annotations in node labels.
    pub show_types: bool,
    /// Whether to show port names on wiring edges.
    pub port_labels: bool,
    /// Port label font size in points.
    pub port_label_size: u32,
    /// Node border pen width.
    pub pen_width: f64,
    /// Significant digits for float display.
    pub significant_digits: usize,
}

impl Default for DotOptions {
    fn default() -> Self {
        Self {
            rankdir: "TB".into(),
            size: (16.0, 10.0),
            dpi: 70,
            node_label_size: 12,
            process_label_size: 12,
            show_values: false,
            show_types: false,
            port_labels: true,
            port_label_size: 10,
            pen_width: 2.0,
            significant_digits: 2,
        }
    }
}

/// Render a Document as a Graphviz DOT string.
pub fn render_dot(doc: &Document, options: &DotOptions) -> String {
    let topology = doc.to_topology();
    render_topology_dot(&topology, &doc.state, options)
}

/// A link-graph hyperedge to draw on top of the place graph.
///
/// Each [`LinkEdge`] is modelled as one invisible "bond anchor" node
/// plus a colored dashed spoke to every endpoint. With `rankdir=TB`
/// Graphviz drops the anchor below the connected ports and routes
/// the spokes through it — naturally producing a U-shape for 2-ended
/// bonds, a 3-spoke star for arity-3 hyperedges, and so on.
///
/// This is also how the link half of a Milner bigraph is most
/// faithfully drawn: a hyperedge is a single edge identity that many
/// points map to, not a 2-ended connection.
#[derive(Clone, Debug)]
pub struct LinkEdge {
    /// Endpoints — paths into the state tree. Length-1 is allowed
    /// (a dangling port) but produces just a labeled spoke with no
    /// connection partner; length-2 is the most common (a bond);
    /// length ≥ 3 are hyperedges.
    pub endpoints: Vec<Vec<Key>>,
    /// Label printed on the bond anchor (typically the link variable
    /// name).
    pub label: String,
    /// Stroke color for the spokes (a hex string).
    pub color: String,
}

/// [`render_state_dot`] with optional link-graph hyperedges layered
/// over the place graph. Useful for pattern-rule diagrams where the
/// link graph adds bonds between place-graph nodes.
pub fn render_state_dot_with_links(
    state: &Value,
    options: &DotOptions,
    links: &[LinkEdge],
) -> String {
    let mut dot = render_state_dot_inner(state, options);
    if !links.is_empty() && dot.ends_with("}\n") {
        dot.truncate(dot.len() - 2);

        // For 2-ended bonds the prettiest shape is a single curve
        // between the two ports — splines=curved + a one-step edge
        // gives Graphviz license to draw a real U rather than two
        // straight spokes meeting at a point. For arity ≥ 3 we still
        // need an anchor (a star center). The two cases coexist; the
        // anchor approach also documents the bond identity, which is
        // the right Milner reading of a hyperedge.
        let _ = writeln!(dot, "    // Link graph");
        for (i, e) in links.iter().enumerate() {
            match e.endpoints.len() {
                0 | 1 => continue,
                2 => emit_bond_2(&mut dot, i, e),
                _ => emit_bond_hyper(&mut dot, i, e),
            }
        }
        dot.push_str("}\n");
    }
    dot
}

/// 2-ended bond: a single curved edge between the two ports, labelled
/// with the link variable name. `constraint=false` keeps the bond out
/// of the place-graph rank flow so it just decorates without pushing
/// nodes around.
fn emit_bond_2(dot: &mut String, _i: usize, e: &LinkEdge) {
    let _ = writeln!(
        dot,
        "    {} -> {} [color=\"{}\", penwidth=2, dir=none, style=dashed, \
         constraint=false, label=<<font color=\"{}\"><b>{}</b></font>>, \
         fontcolor=\"{}\", fontsize=10];",
        path_to_id(&e.endpoints[0]),
        path_to_id(&e.endpoints[1]),
        e.color,
        e.color,
        escape_dot_string(&e.label),
        e.color,
    );
}

/// Hyperedge (arity ≥ 3): one bond-anchor node plus a spoke to each
/// endpoint. The anchor is laid out below the ports; spokes form a
/// star or fan. `constraint=true` lets the edges influence rank so
/// the anchor drops naturally.
fn emit_bond_hyper(dot: &mut String, i: usize, e: &LinkEdge) {
    let anchor = format!("bond_anchor_{i}");
    let _ = writeln!(
        dot,
        "    {anchor} [shape=circle, style=filled, fillcolor=\"{}\", \
         color=\"{}\", penwidth=0, width=0.12, height=0.12, fixedsize=true, \
         label=\"\", xlabel=<<font color=\"{}\" point-size=\"10\"><b>{}</b></font>>];",
        e.color, e.color, e.color, escape_dot_string(&e.label),
    );
    for endpoint in &e.endpoints {
        let _ = writeln!(
            dot,
            "    {} -> {anchor} [color=\"{}\", penwidth=2, dir=none, \
             style=dashed, arrowhead=none];",
            path_to_id(endpoint),
            e.color,
        );
    }
}

/// Render a [`Value`] tree as a Graphviz DOT string in the same
/// bigraph-viz style as [`render_dot`], but without requiring any
/// processes. Every nested map becomes a filled circle node; parent →
/// child containment becomes a solid arrowhead-less edge.
///
/// Node labels prefer the value's `_type` (or `_control`) tag — that's
/// what carries the sort label in pattern / reaction-rule diagrams —
/// falling back to the dict key. Used for rendering rule patterns
/// (which are state-only — no processes), one-shot state
/// visualisations, and debugging snapshots.
pub fn render_state_dot(state: &Value, options: &DotOptions) -> String {
    render_state_dot_inner(state, options)
}

fn render_state_dot_inner(state: &Value, options: &DotOptions) -> String {
    let mut dot = String::new();
    let _ = writeln!(dot, "digraph {{");
    let _ = writeln!(dot, "    rankdir={};", options.rankdir);
    let _ = writeln!(
        dot,
        "    size=\"{},{}\";",
        options.size.0, options.size.1
    );
    let _ = writeln!(dot, "    dpi={};", options.dpi);
    // `splines=curved` forces edges to be drawn as Bézier curves —
    // important for the link-graph layer: two-ended bonds need a real
    // U-shape rather than the straight line Graphviz defaults to for
    // unobstructed paths.
    let _ = writeln!(dot, "    splines=curved;");
    let _ = writeln!(dot);

    // Walk the state, collecting one path per nested map.
    let mut paths: Vec<Vec<Key>> = Vec::new();
    walk_state_paths(state, &mut Vec::new(), &mut paths);

    let _ = writeln!(dot, "    // State nodes");
    for path in &paths {
        let node_id = path_to_id(path);
        let (label, sort_label) = state_label(path, state);
        let (fill, border) = sort_node_color(sort_label.as_deref(), path);
        let _ = writeln!(
            dot,
            "    {node_id} [shape=circle, style=filled, fillcolor=\"{fill}\", \
             color=\"{border}\", penwidth=1.4, label=\"{label}\", fontsize={fs}];",
            fs = options.node_label_size,
        );
    }
    let _ = writeln!(dot);

    let _ = writeln!(dot, "    // Hierarchy");
    let empty_procs: HashSet<&str> = HashSet::new();
    for (parent, child) in compute_hierarchy(&paths, &empty_procs) {
        // The edge label is the child key — tells the reader which
        // slot of the parent this child fills.
        let edge_label = child.last().map(|s| s.as_str()).unwrap_or("");
        let _ = writeln!(
            dot,
            "    {} -> {} [arrowhead=none, penwidth={pw}, label=\"{}\", \
             fontcolor=\"#777777\", fontsize=9];",
            path_to_id(&parent),
            path_to_id(&child),
            escape_dot_string(edge_label),
            pw = options.pen_width,
        );
    }

    let _ = writeln!(dot, "}}");
    dot
}

/// Walk a [`Value`] tree adding a path for each map node (including
/// the path-to-leaf entries that name-only carry a `_type` tag).
fn walk_state_paths(value: &Value, path: &mut Vec<Key>, out: &mut Vec<Vec<Key>>) {
    if let Some(iter) = value.iter_fields() {
        for (k, child) in iter {
            if k.starts_with('_') {
                continue;
            }
            path.push(k.clone());
            out.push(path.clone());
            walk_state_paths(child, path, out);
            path.pop();
        }
    }
}

/// Pick a label for a node at `path`. Prefers the value's `_type`
/// (or `_control`) tag; falls back to the dict key.
///
/// Returns `(label, sort)` so the caller can use the sort for
/// colouring.
fn state_label(path: &[Key], state: &Value) -> (String, Option<String>) {
    let key = path.last().map(|s| s.as_str()).unwrap_or("?");
    let sort = state
        .get_path(path)
        .and_then(|v| {
            v.get_field("_type")
                .or_else(|| v.get_field("_control"))
        })
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let label = sort.clone().unwrap_or_else(|| key.to_string());
    (label, sort)
}

/// Look up a sort-tinted (fill, stroke) colour pair. Falls back to
/// the path-based heuristic from [`state_node_color`] when no sort
/// tag is present.
fn sort_node_color(sort: Option<&str>, path: &[Key]) -> (String, String) {
    if let Some(s) = sort {
        let (f, b) = sort_palette(s);
        return (f.to_string(), b.to_string());
    }
    let (f, b) = state_node_color(path);
    (f.to_string(), b)
}

/// MAPK-friendly sort palette — matches `pattern.rs`. Generic sorts
/// fall back to the spatio-flux teal.
fn sort_palette(label: &str) -> (&'static str, &'static str) {
    match label {
        "MEK" => ("#9ecae1", "#3b6fb0"),
        "ERK" => ("#7fbf7b", "#3a7a3a"),
        "pERK" => ("#ef8a62", "#b85a1a"),
        "Cell" => ("#f5f5f5", "#666666"),
        "Compartment" => ("#e8e4d8", "#777067"),
        "Cytoplasm" => ("#cfe3cf", "#5a7a5a"),
        "Nucleus" => ("#f0d8b4", "#7d5b3a"),
        "ERLumen" => ("#f5c6a4", "#a96a3a"),
        // Pattern markers — neutral, slightly muted.
        s if s.starts_with("◦") => ("#ffffff", "#888888"),
        s if s.starts_with("●") => ("#ffe9d1", "#d95f02"),
        s if s.starts_with("✗") => ("#fff5f5", "#c0392b"),
        _ => ("#C3E1D6", "#98afa6"),
    }
}

fn escape_dot_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Render a Topology as a Graphviz DOT string.
/// Deduplicate topology for visualization: for maps with multiple complex children
/// (like particles with many IDs), keep only one representative in both the
/// topology (processes) and state.
/// Maps process name → display label (with count for collapsed groups).
type CollapseMap = HashMap<String, String>;

fn deduplicate_for_viz(topology: &Topology, state: &Value) -> (Topology, Value, CollapseMap) {
    // Find maps in state that have multiple complex children
    let mut representatives: HashMap<Vec<Key>, Key> = HashMap::new();

    fn find_representatives(val: &Value, path: &[Key], reps: &mut HashMap<Vec<Key>, Key>) {
        if let Value::Map(map) = val {
            let complex_count = map.values()
                .filter(|v| matches!(v, Value::Map(m) if m.len() > 2))
                .count();
            if complex_count > 1 && complex_count == map.len() {
                if let Some(first_key) = map.keys().next() {
                    reps.insert(path.to_vec(), first_key.clone());
                }
            }
            for (k, v) in map {
                let mut child_path = path.to_vec();
                child_path.push(k.clone());
                find_representatives(v, &child_path, reps);
            }
        }
    }

    find_representatives(state, &[], &mut representatives);

    // Check if a dot-separated name goes through a non-representative child
    let is_rep = |name: &str| -> bool {
        let parts: Vec<&str> = name.split('.').collect();
        for i in 1..parts.len() {
            let parent: Vec<Key> = parts[..i].iter().map(|s| Key::from(*s)).collect();
            if let Some(rep) = representatives.get(&parent) {
                if parts[i] != rep.as_str() {
                    return false;
                }
            }
        }
        true
    };

    let mut filtered = topology.clone();
    let mut collapse_map = CollapseMap::new();

    // 1. Filter nested duplicate processes (particles/spatial grids).
    // Count how many were removed per representative so we can label them.
    let _pre_count = filtered.processes.len();
    let mut nested_rep_counts: HashMap<String, usize> = HashMap::new();
    for name in filtered.processes.keys() {
        if !is_rep(name) {
            // Find which representative this maps to
            let parts: Vec<&str> = name.split('.').collect();
            for i in 1..parts.len() {
                let parent: Vec<Key> = parts[..i].iter().map(|s| Key::from(*s)).collect();
                if let Some(rep) = representatives.get(&parent) {
                    let mut rep_name = parts[..i].to_vec();
                    rep_name.push(rep);
                    rep_name.extend_from_slice(&parts[i+1..]);
                    let rep_full = rep_name.join(".");
                    *nested_rep_counts.entry(rep_full).or_insert(1) += 1;
                    break;
                }
            }
        }
    }
    filtered.processes.retain(|name, _| is_rep(name));

    // Add collapse labels for nested deduplication
    for (rep_name, count) in &nested_rep_counts {
        if *count > 1 {
            if let Some(last) = rep_name.rsplit('.').next() {
                let base = last.split('[').next().unwrap_or(last);
                collapse_map.insert(
                    rep_name.clone(),
                    format!("{base}[*] (x{count})"),
                );
            }
        }
    }

    // 2. Collapse top-level duplicate process types.
    // Group by name pattern (with [indices] replaced by [*]) AND process_type.
    // This prevents collapsing processes like "ecoli_1 dFBA" and "ecoli_2 dFBA"
    // which are different processes with different wiring, even though they
    // share the same process_type.
    let name_pattern = |name: &str| -> String {
        // Replace bracket-enclosed indices with [*]
        let re_like: String = name.chars().fold((String::new(), false), |(mut s, in_bracket), c| {
            match c {
                '[' => { s.push('['); (s, true) }
                ']' => { s.push_str("*]"); (s, false) }
                _ if in_bracket => (s, true), // skip index chars
                _ => { s.push(c); (s, false) }
            }
        }).0;
        re_like
    };

    let mut pattern_groups: HashMap<(String, String), Vec<String>> = HashMap::new();
    for (name, spec) in &filtered.processes {
        let pattern = name_pattern(name);
        pattern_groups
            .entry((spec.process_type.clone(), pattern))
            .or_default()
            .push(name.clone());
    }

    for ((_, _), names) in &pattern_groups {
        if names.len() > 1 {
            let representative = &names[0];
            let count = names.len();
            let base_name = representative
                .split('[').next()
                .unwrap_or(representative);
            // Use leaf name for the label (strip parent path)
            let leaf = base_name.rsplit('.').next().unwrap_or(base_name);
            collapse_map.insert(
                representative.clone(),
                format!("{leaf}[*] (x{count})"),
            );
            for name in &names[1..] {
                filtered.processes.swap_remove(name);
            }
        }
    }

    // 3. Filter state: for each representative parent, keep only the representative child
    let mut filtered_state = state.clone();
    for (parent_path, rep_key) in &representatives {
        if let Some(Value::Map(map)) = filtered_state.get_path(parent_path).cloned().as_ref() {
            let mut kept = indexmap::IndexMap::new();
            if let Some(rep_val) = map.get(rep_key.as_str()) {
                kept.insert(rep_key.clone(), rep_val.clone());
            }
            filtered_state.set_path(parent_path, Value::Map(kept));
        }
    }

    (filtered, filtered_state, collapse_map)
}

pub fn render_topology_dot(
    topology: &Topology,
    state: &Value,
    options: &DotOptions,
) -> String {
    // Filter topology to show only one representative for duplicate structures
    // (e.g., multiple particles with identical process specs → show one)
    let (topology, state, collapse_map) = deduplicate_for_viz(topology, state);
    let topology = &topology;
    let state = &state;

    let mut dot = String::new();
    let _ = writeln!(dot, "digraph {{");
    let _ = writeln!(dot, "    rankdir={};", options.rankdir);
    let _ = writeln!(
        dot,
        "    size=\"{},{}\";",
        options.size.0, options.size.1
    );
    let _ = writeln!(dot, "    dpi={};", options.dpi);
    let _ = writeln!(dot, "    splines=true;");
    let _ = writeln!(dot);

    let (mut state_paths, _) = collect_state_paths(topology, state);
    expand_state_tree(&mut state_paths, state, 3);

    // Render state nodes (circles) — filled, colored by role
    let _ = writeln!(dot, "    // State nodes");
    let mut emitted_state_ids: HashSet<String> = HashSet::new();
    for path in &state_paths {
        let node_id = path_to_id(path);
        emitted_state_ids.insert(node_id.clone());
        let label = make_state_label(path, state, options);
        let (fill, border) = state_node_color(path);
        let _ = writeln!(
            dot,
            "    {node_id} [shape=circle, style=filled, fillcolor=\"{fill}\", \
             color=\"{border}\", penwidth=1, label={label}, fontsize={fs}];",
            fs = options.node_label_size,
        );
    }
    let _ = writeln!(dot);

    // Render process/step nodes (boxes) — colored by process family, no outline
    // For nested processes (e.g., "particles.pid.monod_kinetics"), show only the
    // last segment as the label.
    let _ = writeln!(dot, "    // Process nodes");
    for (name, spec) in &topology.processes {
        let node_id = format!("proc_{}", sanitize(name));
        let (fill, _border) = process_node_color(name, &spec.process_type);
        let label = collapse_map
            .get(name.as_str())
            .map(|s| s.as_str())
            .unwrap_or_else(|| name.rsplit('.').next().unwrap_or(name));
        let _ = writeln!(
            dot,
            "    {node_id} [shape=box, style=filled, fillcolor=\"{fill}\", \
             color=\"{fill}\", penwidth=0, label=\"{label}\", fontsize={fs}];",
            fs = options.process_label_size,
        );
    }
    let _ = writeln!(dot);

    // Build set of process names for wiring resolution
    let process_names: HashSet<&str> = topology
        .processes
        .keys()
        .map(|s| s.as_str())
        .collect();

    // Resolve a wire path to a node ID — if path starts with a process name,
    // wire to that process box instead of a (non-existent) state circle
    let resolve_node_id = |path: &[Key]| -> String {
        // Only redirect single-element paths that match a process name
        if path.len() == 1 {
            if let Some(root) = path.first() {
                if process_names.contains(root.as_str()) {
                    return format!("proc_{}", sanitize(root.as_str()));
                }
            }
        }
        path_to_id(path)
    };

    // Render wiring edges
    let _ = writeln!(dot, "    // Wiring");
    for (name, spec) in &topology.processes {
        let proc_id = format!("proc_{}", sanitize(name));

        // Find bidirectional ports (same port name in both inputs and outputs
        // wired to the same path)
        let mut bidi_ports: HashSet<String> = HashSet::new();
        for (port, in_path) in &spec.inputs {
            if let Some(out_path) = spec.outputs.get(port) {
                if in_path == out_path {
                    bidi_ports.insert(port.clone());
                }
            }
        }

        // Input edges: state → process (forward arrow, dashed)
        for (port, path) in &spec.inputs {
            if bidi_ports.contains(port) {
                continue; // handled as bidirectional
            }
            let state_id = resolve_node_id(path);
            let label = if options.port_labels {
                format!(
                    ", label=\"{port}\", fontsize={}",
                    options.port_label_size
                )
            } else {
                String::new()
            };
            let _ = writeln!(
                dot,
                "    {state_id} -> {proc_id} [style=dashed, penwidth=1, arrowhead=normal{label}];",
            );
        }

        // Output edges: process → state (forward arrow, dashed)
        for (port, path) in &spec.outputs {
            if bidi_ports.contains(port) {
                continue;
            }
            let state_id = resolve_node_id(path);
            let label = if options.port_labels {
                format!(
                    ", label=\"{port}\", fontsize={}",
                    options.port_label_size
                )
            } else {
                String::new()
            };
            let _ = writeln!(
                dot,
                "    {proc_id} -> {state_id} [style=dashed, penwidth=1, arrowhead=normal{label}];",
            );
        }

        // Bidirectional edges: both arrows
        for port in &bidi_ports {
            let path = &spec.inputs[port];
            let state_id = resolve_node_id(path);
            let label = if options.port_labels {
                format!(
                    ", label=\"{port}\", fontsize={}",
                    options.port_label_size
                )
            } else {
                String::new()
            };
            let _ = writeln!(
                dot,
                "    {state_id} -> {proc_id} [style=dashed, penwidth=1, dir=both{label}];",
            );
        }
    }

    // Render hierarchy edges (state paths that are prefixes of other paths)
    let _ = writeln!(dot);
    let _ = writeln!(dot, "    // Hierarchy");
    let hierarchy = compute_hierarchy(&state_paths, &process_names);
    for (parent, child) in &hierarchy {
        let parent_id = if parent.len() == 1
            && process_names.contains(parent[0].as_str())
        {
            format!("proc_{}", sanitize(&parent[0]))
        } else {
            path_to_id(parent)
        };
        let child_id = path_to_id(child);
        let _ = writeln!(
            dot,
            "    {parent_id} -> {child_id} [arrowhead=none, penwidth={pw}];",
            pw = options.pen_width,
        );
    }

    // Hierarchy edges for nested processes: if a process name contains dots
    // (e.g., "particles.pid.monod_kinetics"), connect its parent state path
    // to the process box. Ensure the parent state node exists.
    let _ = writeln!(dot, "    // Nested process hierarchy");
    for name in topology.processes.keys() {
        if let Some(dot_pos) = name.rfind('.') {
            let parent_path_str = &name[..dot_pos];
            let parent_path: Vec<Key> =
                parent_path_str.split('.').map(Key::from).collect();
            let parent_id = path_to_id(&parent_path);
            let proc_id = format!("proc_{}", sanitize(name));

            // Ensure parent state node is defined (it may not be in the
            // wiring-derived paths if it only contains process specs)
            if !emitted_state_ids.contains(&parent_id) {
                emitted_state_ids.insert(parent_id.clone());
                let label = parent_path.last().map(|s| s.as_str()).unwrap_or("");
                let (fill, border) = state_node_color(&parent_path);
                let _ = writeln!(
                    dot,
                    "    {parent_id} [shape=circle, style=filled, fillcolor=\"{fill}\", \
                     color=\"{border}\", penwidth=1, label=\"{label}\", fontsize=12];",
                );
            }

            let _ = writeln!(
                dot,
                "    {parent_id} -> {proc_id} [arrowhead=none, penwidth={pw}];",
                pw = options.pen_width,
            );
        }
    }

    let _ = writeln!(dot, "}}");
    dot
}

/// Collect all unique state paths from process wiring.
/// Filters out paths that collide with process names (those are rendered as boxes).
/// Returns (state_paths, representatives) where representatives maps parent paths
/// to the chosen child key for deduplication.
fn collect_state_paths(topology: &Topology, state: &Value) -> (Vec<Vec<Key>>, HashMap<Vec<Key>, Key>) {
    let process_names: HashSet<&str> = topology
        .processes
        .keys()
        .map(|s| s.as_str())
        .collect();

    let mut paths: Vec<Vec<Key>> = Vec::new();
    let mut seen: HashSet<Vec<Key>> = HashSet::new();

    for spec in topology.processes.values() {
        for path in spec.inputs.values().chain(spec.outputs.values()) {
            if seen.insert(path.clone()) {
                paths.push(path.clone());
            }
        }
    }

    // Also add intermediate path nodes for hierarchy
    let leaf_paths = paths.clone();
    for path in &leaf_paths {
        for i in 1..path.len() {
            let prefix = path[..i].to_vec();
            if seen.insert(prefix.clone()) {
                paths.push(prefix);
            }
        }
    }

    // Filter out:
    // 1. Paths that are EXACTLY a process name (single element)
    // 2. Paths whose value in the state tree is a process spec (has "address")
    paths.retain(|path| {
        if path.len() == 1 {
            if let Some(name) = path.first() {
                if process_names.contains(name.as_str()) {
                    return false;
                }
            }
        }
        // Check if the value at this path is a process spec
        if let Some(Value::Map(m)) = state.get_path(path) {
            if m.contains_key("address") {
                return false;
            }
        }
        true
    });

    // Deduplicate: for maps with multiple complex children (like particles),
    // keep only one representative. Find which parents have multiple complex
    // children, pick the first child as representative, filter the rest.
    let mut representatives: HashMap<Vec<Key>, Key> = HashMap::new();

    // First pass: determine the representative child for each complex-valued parent
    for path in &paths {
        for i in 1..path.len() {
            let parent = path[..i].to_vec();
            if representatives.contains_key(&parent) {
                continue;
            }
            if let Some(Value::Map(parent_map)) = state.get_path(&parent) {
                let has_complex = parent_map.values()
                    .any(|v| matches!(v, Value::Map(m) if m.len() > 1));
                if has_complex && parent_map.len() > 1 {
                    representatives.insert(parent, path[i].clone());
                }
            }
        }
    }

    // Second pass: filter paths that go through non-representative children
    if !representatives.is_empty() {
        paths.retain(|path| {
            for i in 1..path.len() {
                let parent = path[..i].to_vec();
                if let Some(rep) = representatives.get(&parent) {
                    if path[i] != *rep {
                        return false;
                    }
                }
            }
            true
        });
    }

    paths.sort();
    (paths, representatives)
}

/// Expand state paths by walking the actual state tree.
/// For each existing state path that points to a map, add its children
/// up to `max_depth` additional levels. This reveals internal structure
/// like particle fields (id, position, mass, local, exchange).
/// Only expands the first entry of map-type containers to avoid explosion.
fn expand_state_tree(paths: &mut Vec<Vec<Key>>, state: &Value, max_depth: usize) {
    let mut seen: HashSet<Vec<Key>> = paths.iter().cloned().collect();
    let mut to_expand: Vec<Vec<Key>> = paths.clone();

    for _depth in 0..max_depth {
        let mut new_paths = Vec::new();
        for path in &to_expand {
            if let Some(Value::Map(map)) = state.get_path(path) {
                // Show all children. The representative-sibling dedup in
                // collect_state_paths already limits multi-entry maps to 1 entry.
                let limit = map.len();
                for (key, child_val) in map.iter().take(limit) {
                    if matches!(child_val, Value::None) {
                        continue;
                    }
                    // Skip process specs (maps with "address" key) — they're
                    // rendered as process boxes, not state circles
                    if let Value::Map(m) = child_val {
                        if m.contains_key("address") {
                            continue;
                        }
                    }
                    let mut child = path.clone();
                    child.push(key.clone());
                    if seen.insert(child.clone()) {
                        new_paths.push(child);
                    }
                }
            }
        }
        paths.extend(new_paths.clone());
        to_expand = new_paths;
    }

    paths.sort();
}

/// Compute parent→child hierarchy edges from paths.
fn compute_hierarchy(
    paths: &[Vec<Key>],
    process_names: &HashSet<&str>,
) -> Vec<(Vec<Key>, Vec<Key>)> {
    let path_set: HashSet<Vec<Key>> = paths.iter().cloned().collect();
    let mut edges = Vec::new();

    for path in paths {
        if path.len() > 1 {
            let parent = path[..path.len() - 1].to_vec();
            // Parent is valid if it's in the state paths OR is a process name
            let parent_exists = path_set.contains(&parent)
                || (parent.len() == 1
                    && process_names.contains(parent[0].as_str()));
            if parent_exists {
                edges.push((parent, path.clone()));
            }
        }
    }

    edges
}

/// Create an HTML-like label for a state node.
fn make_state_label(
    path: &[Key],
    state: &Value,
    options: &DotOptions,
) -> String {
    let name = path.last().map(|s| s.as_str()).unwrap_or("?");

    if !options.show_values && !options.show_types {
        return format!("\"{name}\"");
    }

    let mut rows = vec![format!(
        "<TR><TD><FONT POINT-SIZE=\"{}\">{name}</FONT></TD></TR>",
        options.node_label_size
    )];

    if options.show_values {
        if let Some(val) = state.get_path(path) {
            let val_str = format_value(val, options.significant_digits);
            if val_str.len() <= 20 {
                rows.push(format!(
                    "<TR><TD><FONT COLOR=\"gray40\" POINT-SIZE=\"10\">{val_str}</FONT></TD></TR>"
                ));
            }
        }
    }

    format!(
        "<<TABLE BORDER=\"0\" CELLBORDER=\"0\" CELLSPACING=\"0\">{}</TABLE>>",
        rows.join("")
    )
}

/// Format a value for display.
fn format_value(val: &Value, digits: usize) -> String {
    match val {
        Value::Float(f) => format!("{:.prec$}", f.0, prec = digits),
        Value::Int(i) => format!("{i}"),
        Value::Bool(b) => format!("{b}"),
        Value::String(s) => {
            if s.len() > 20 {
                format!("\"{}...\"", &s[..17])
            } else {
                format!("\"{s}\"")
            }
        }
        Value::None => "none".into(),
        Value::List(l) => format!("[{} items]", l.len()),
        Value::Map(m) => format!("{{{} keys}}", m.len()),
        Value::Struct { layout, .. } => format!("{{{} fields}}", layout.fields.len()),
        Value::Foreign(f) => format!("foreign:{}", f.type_name),
        Value::Bytes(b) => format!("<{} bytes>", b.len()),
    }
}

/// Convert a state path to a valid DOT node ID.
fn path_to_id(path: &[Key]) -> String {
    if path.is_empty() {
        "root".into()
    } else {
        format!("state_{}", path.iter().map(|s| sanitize(s)).collect::<Vec<_>>().join("__"))
    }
}

/// Sanitize a string for use as a DOT identifier.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Save DOT to a file.
pub fn save_dot(dot: &str, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
    std::fs::write(path, dot)
}

/// Render DOT to an image file using the `dot` command.
/// Requires graphviz to be installed.
pub fn render_to_file(
    dot: &str,
    output_path: impl AsRef<std::path::Path>,
    format: &str,
) -> std::io::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("dot")
        .arg(format!("-T{format}"))
        .arg("-o")
        .arg(output_path.as_ref())
        .stdin(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(dot.as_bytes())?;
    }

    child.wait()?;
    Ok(())
}

// ── Color palette (matching spatio-flux Python colors.py) ──

/// Darken a hex color by a factor.
fn darken(hex: &str, factor: f64) -> String {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(200);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(200);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(200);
    format!(
        "#{:02x}{:02x}{:02x}",
        (r as f64 * factor) as u8,
        (g as f64 * factor) as u8,
        (b as f64 * factor) as u8,
    )
}

/// Color for a state node based on its path.
fn state_node_color(path: &[Key]) -> (&'static str, String) {
    let name = path.last().map(|s| s.as_str()).unwrap_or("");
    let first = path.first().map(|s| s.as_str()).unwrap_or("");

    // Check leaf name first (semantically meaningful regardless of nesting)
    let fill = match name {
        "local" => "#F4E6B8",                             // yellow (field values)
        "exchange" => "#EEC06F",                          // amber (exchange deltas)
        "mass" | "biomass" | "dissolved biomass" => "#E4CF77",  // gold
        "position" | "sub_masses" | "id" => "#C3E1D6",   // sage (particle internals)
        "global_time" | "interval" => "#C9CED6",          // neutral
        "particles" => "#C3E1D6",                         // sage
        "fields" => "#F4E6B8",                            // yellow
        _ => {
            // Fall back to context from root
            match first {
                "particles" => "#C3E1D6",
                "fields" => "#F4E6B8",
                _ => "#E8E8E8",
            }
        }
    };
    (fill, darken(fill, 0.78))
}

/// Color for a process node based on its name and type.
fn process_node_color(name: &str, process_type: &str) -> (&'static str, String) {
    let fill = match process_type {
        // Metabolism (coral/mauve)
        "DynamicFBA" | "SpatialDFBA" => "#A24E56",
        "MonodKinetics" => "#D9A4A6",
        // Transport (olive/mustard)
        "DiffusionAdvection" => "#D1BE56",
        // Movement (sage/teal)
        "BrownianMovement" => "#C3E1D6",
        "PymunkParticleMovement" => "#7FBFA8",
        // Coupling (amber)
        "ParticleExchange" => "#C6D780",
        // Rewrite (teal)
        "ParticleDivision" | "ManageBoundaries" => "#9FD8DF",
        "ParticleTotalMass" => "#C6D780",
        _ => {
            // Fallback: guess from name
            if name.contains("dFBA") || name.contains("dfba") || name.contains("DynamicFBA") {
                "#A24E56"
            } else if name.contains("kinetics") || name.contains("Kinetics") || name.contains("monod") {
                "#D9A4A6"
            } else if name.contains("diffusion") || name.contains("Diffusion") {
                "#D1BE56"
            } else if name.contains("brownian") || name.contains("movement") {
                "#C3E1D6"
            } else if name.contains("newtonian") || name.contains("pymunk") || name.contains("Pymunk") {
                "#7FBFA8"
            } else if name.contains("exchange") || name.contains("Exchange") {
                "#C6D780"
            } else if name.contains("division") || name.contains("boundar") {
                "#9FD8DF"
            } else {
                "#C9CED6"  // neutral
            }
        }
    };
    (fill, darken(fill, 0.78))
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use prism_bigraph::Document;

    #[test]
    fn test_basic_dot_output() {
        let mut doc = Document::new();
        doc.state = Value::tree([
            ("biomass", Value::float(1.0)),
            ("glucose", Value::float(10.0)),
        ]);
        doc.processes.insert(
            "growth".into(),
            prism_bigraph::document::ProcessDocument {
                process_type: "monod_kinetics".into(),
                config: Value::None,
                inputs: IndexMap::from([
                    ("biomass".into(), vec!["biomass".into()]),
                    ("glucose".into(), vec!["glucose".into()]),
                ]),
                outputs: IndexMap::from([
                    ("biomass".into(), vec!["biomass".into()]),
                    ("glucose".into(), vec!["glucose".into()]),
                ]),
                interval: Some(1.0),
                priority: 0.0,
            },
        );

        let dot = render_dot(&doc, &DotOptions::default());
        assert!(dot.contains("digraph"));
        assert!(dot.contains("state_biomass"));
        assert!(dot.contains("state_glucose"));
        assert!(dot.contains("proc_growth"));
        assert!(dot.contains("dir=both")); // bidirectional wiring
    }

    #[test]
    fn test_hierarchical_paths() {
        let mut doc = Document::new();
        doc.state = Value::tree([
            ("cell", Value::tree([
                ("glucose", Value::float(5.0)),
                ("atp", Value::float(100.0)),
            ])),
        ]);
        doc.processes.insert(
            "metabolism".into(),
            prism_bigraph::document::ProcessDocument {
                process_type: "fba".into(),
                config: Value::None,
                inputs: IndexMap::from([
                    ("glc".into(), vec!["cell".into(), "glucose".into()]),
                    ("energy".into(), vec!["cell".into(), "atp".into()]),
                ]),
                outputs: IndexMap::from([
                    ("glc".into(), vec!["cell".into(), "glucose".into()]),
                    ("energy".into(), vec!["cell".into(), "atp".into()]),
                ]),
                interval: Some(1.0),
                priority: 0.0,
            },
        );

        let dot = render_dot(&doc, &DotOptions::default());
        // Should have hierarchy: cell → glucose, cell → atp
        assert!(dot.contains("state_cell"));
        assert!(dot.contains("state_cell__glucose"));
        assert!(dot.contains("state_cell__atp"));
        assert!(dot.contains("arrowhead=none")); // hierarchy edges
    }
}

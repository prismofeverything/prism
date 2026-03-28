//! DOT format generation from bigraph structures.
//!
//! Produces Graphviz digraph source that can be rendered with `dot`:
//! - State nodes as circles
//! - Process/step nodes as boxes
//! - Input wiring as dashed forward arrows
//! - Output wiring as dashed back arrows
//! - Bidirectional wiring as dashed both arrows
//! - Hierarchical containment as solid edges

use std::collections::HashSet;
use std::fmt::Write;

use prism_bigraph::document::Document;
use prism_bigraph::topology::Topology;
use prism_schema::Value;

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

/// Render a Topology as a Graphviz DOT string.
pub fn render_topology_dot(
    topology: &Topology,
    state: &Value,
    options: &DotOptions,
) -> String {
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

    // Collect all state paths referenced by processes
    let mut state_paths = collect_state_paths(topology);

    // Expand state tree: show internals of map-type state nodes (e.g., particles)
    // by adding child paths from the actual state data
    expand_state_tree(&mut state_paths, state, 3);

    // Render state nodes (circles) — filled, colored by role
    let _ = writeln!(dot, "    // State nodes");
    for path in &state_paths {
        let node_id = path_to_id(path);
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
        let label = name.rsplit('.').next().unwrap_or(name);
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
    let resolve_node_id = |path: &[String]| -> String {
        // Only redirect single-element paths that match a process name
        if path.len() == 1 {
            if let Some(root) = path.first() {
                if process_names.contains(root.as_str()) {
                    return format!("proc_{}", sanitize(root));
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
    // to the process box
    let _ = writeln!(dot, "    // Nested process hierarchy");
    for name in topology.processes.keys() {
        if let Some(dot_pos) = name.rfind('.') {
            let parent_path_str = &name[..dot_pos];
            let parent_path: Vec<String> =
                parent_path_str.split('.').map(|s| s.to_string()).collect();
            let parent_id = path_to_id(&parent_path);
            let proc_id = format!("proc_{}", sanitize(name));
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
fn collect_state_paths(topology: &Topology) -> Vec<Vec<String>> {
    let process_names: HashSet<&str> = topology
        .processes
        .keys()
        .map(|s| s.as_str())
        .collect();

    let mut paths: Vec<Vec<String>> = Vec::new();
    let mut seen: HashSet<Vec<String>> = HashSet::new();

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

    // Filter out paths that are EXACTLY a process name (single element).
    // Sub-paths like ["brownian_movement", "interval"] stay — they represent
    // process config state that other processes can read.
    paths.retain(|path| {
        if path.len() == 1 {
            if let Some(name) = path.first() {
                return !process_names.contains(name.as_str());
            }
        }
        true
    });

    paths.sort();
    paths
}

/// Expand state paths by walking the actual state tree.
/// For each existing state path that points to a map, add its children
/// up to `max_depth` additional levels. This reveals internal structure
/// like particle fields (id, position, mass, local, exchange).
/// Only expands the first entry of map-type containers to avoid explosion.
fn expand_state_tree(paths: &mut Vec<Vec<String>>, state: &Value, max_depth: usize) {
    let mut seen: HashSet<Vec<String>> = paths.iter().cloned().collect();
    let mut to_expand: Vec<Vec<String>> = paths.clone();

    for _depth in 0..max_depth {
        let mut new_paths = Vec::new();
        for path in &to_expand {
            if let Some(Value::Map(map)) = state.get_path(path) {
                // For very large maps (>20 entries like many dFBA processes),
                // only show first entry. Otherwise show all.
                let limit = if map.len() > 20 { 1 } else { map.len() };
                for (key, child_val) in map.iter().take(limit) {
                    if matches!(child_val, Value::None) {
                        continue;
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
    paths: &[Vec<String>],
    process_names: &HashSet<&str>,
) -> Vec<(Vec<String>, Vec<String>)> {
    let path_set: HashSet<Vec<String>> = paths.iter().cloned().collect();
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
    path: &[String],
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
        Value::Bytes(b) => format!("<{} bytes>", b.len()),
    }
}

/// Convert a state path to a valid DOT node ID.
fn path_to_id(path: &[String]) -> String {
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
fn state_node_color(path: &[String]) -> (&'static str, String) {
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

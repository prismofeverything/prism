//! Adapter from reaction-rule
//! [`Pattern`](prism_schema::reaction::Pattern)s to the existing
//! `render_state_dot` pipeline.
//!
//! Rather than write a second DOT-emitter for patterns, we convert a
//! pattern to a [`Value`] — a state tree with marker nodes for the
//! three pattern-only constructors — and reuse
//! [`crate::render_state_dot_with_links`]. That keeps the bigraph-viz
//! visual language consistent across state and rule diagrams.
//!
//! ## Marker encoding
//!
//! - [`Pattern::Map`] preserves keys; nested patterns recurse.
//! - [`Pattern::Site`] becomes `{_type: "◦ site"}` — a small circle
//!   labelled `◦ site`.
//! - [`Pattern::LinkVar(name)`] becomes `{_type: "● <name>"}` — a
//!   circle labelled with the variable name. Two link variables with
//!   the same name additionally get connected by a colored dashed
//!   edge in the link graph (drawn after the place-graph hierarchy).
//! - [`Pattern::Absent`] becomes `{_type: "✗ <key>"}`.
//! - [`Pattern::Atom`] becomes the underlying [`Value`] unchanged.

use prism_schema::reaction::Pattern;
use prism_schema::{Key, StateMap, Value};

use crate::dot::LinkEdge;
use crate::pattern::linkvar_color;

/// Render a single pattern as a DOT digraph string. Shared `LinkVar`s
/// are drawn as dashed colored edges between their port circles.
pub fn render_pattern_dot(pattern: &Pattern, title: &str) -> String {
    let (state, links) = pattern_to_state_with_links(pattern);
    let mut dot =
        crate::render_state_dot_with_links(&state, &crate::DotOptions::default(), &links);
    if !title.is_empty() {
        dot = dot.replacen(
            "digraph {\n",
            &format!(
                "digraph {{\n    label=<<b>{}</b>>;\n    labelloc=\"t\";\n",
                escape_html(title),
            ),
            1,
        );
    }
    dot
}

/// Render a redex → reactum pair as two DOT strings, one each.
/// The caller is expected to place them side-by-side in the report.
pub fn render_rule_pair_dot(
    _label: &str,
    redex: &Pattern,
    reactum: &Pattern,
    _rule_color: &str,
) -> (String, String) {
    (
        render_pattern_dot(redex, "redex"),
        render_pattern_dot(reactum, "reactum"),
    )
}

/// Convert a pattern to (state, link_edges).
///
/// The state is what `render_state_dot` will walk to draw the place
/// graph. The link edges are the bonds — one per pair of same-named
/// [`Pattern::LinkVar`] occurrences (chained when more than two share
/// a name). A single occurrence becomes a port with no edge (a
/// dangling endpoint, valid for reactum-introduced edges).
pub fn pattern_to_state_with_links(pattern: &Pattern) -> (Value, Vec<LinkEdge>) {
    let mut anchors: Vec<(String, Vec<Key>)> = Vec::new();
    let state = walk_into_state(pattern, None, &mut Vec::new(), &mut anchors);

    // Bigraph state convention: top-level keys are *regions*. If the
    // top is a single sort'd map, wrap it in a synthetic root key so
    // the renderer has something to walk into; rewrite anchor paths
    // accordingly.
    let (state, anchors) = wrap_as_state(state, anchors);

    let mut by_name: std::collections::BTreeMap<String, Vec<Vec<Key>>> =
        std::collections::BTreeMap::new();
    for (name, path) in anchors {
        by_name.entry(name).or_default().push(path);
    }

    // One hyperedge per link variable name. The renderer adds a
    // single bond anchor for each, with spokes to every endpoint —
    // so arity-3+ link variables naturally render as star-shaped
    // hyperedges, not chained pairs.
    let mut links = Vec::new();
    for (name, paths) in by_name {
        if paths.len() < 2 {
            continue;
        }
        links.push(LinkEdge {
            endpoints: paths,
            label: name.clone(),
            color: linkvar_color(&name).to_string(),
        });
    }

    (state, links)
}

/// Walk a pattern, building the marker-encoded state Value AND a
/// list of `(link_var_name, path_to_port)` tuples that the caller
/// turns into link-graph edges.
fn walk_into_state(
    pat: &Pattern,
    key_hint: Option<&str>,
    path: &mut Vec<Key>,
    anchors: &mut Vec<(String, Vec<Key>)>,
) -> Value {
    match pat {
        Pattern::Site => marker_node("◦ site"),
        Pattern::LinkVar(name) => {
            anchors.push((name.to_string(), path.clone()));
            marker_node(&format!("● {name}"))
        }
        Pattern::Absent => {
            let label = match key_hint {
                Some(k) if !k.is_empty() => format!("✗ {k}"),
                _ => "✗ absent".to_string(),
            };
            marker_node(&label)
        }
        Pattern::Atom(v) => v.clone(),
        Pattern::Map(m) => {
            let mut out = StateMap::new();
            for (k, v) in m {
                let push = !k.starts_with('_');
                if push {
                    path.push(k.clone());
                }
                let child = walk_into_state(v, Some(k.as_str()), path, anchors);
                if push {
                    path.pop();
                }
                out.insert(k.clone(), child);
            }
            Value::Map(out)
        }
        Pattern::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, p) in items.iter().enumerate() {
                let key = Key::from(i.to_string());
                path.push(key.clone());
                out.push(walk_into_state(p, Some(&i.to_string()), path, anchors));
                path.pop();
            }
            Value::List(out)
        }
    }
}

/// If the top-level value is a single-rooted map (has `_type` /
/// `_control`), wrap it in a synthetic `root` key so the place graph
/// has an outermost region for `render_state_dot` to walk into.
/// Rewrites all collected anchor paths to match.
fn wrap_as_state(
    state: Value,
    anchors: Vec<(String, Vec<Key>)>,
) -> (Value, Vec<(String, Vec<Key>)>) {
    let single_rooted = matches!(&state, Value::Map(m)
        if m.contains_key("_type") || m.contains_key("_control"));
    if !single_rooted {
        return (state, anchors);
    }
    let mut m = StateMap::new();
    m.insert(Key::from("root"), state);
    let anchors = anchors
        .into_iter()
        .map(|(n, mut p)| {
            p.insert(0, Key::from("root"));
            (n, p)
        })
        .collect();
    (Value::Map(m), anchors)
}

fn marker_node(label: &str) -> Value {
    let mut m = StateMap::new();
    m.insert(Key::from("_type"), Value::String(label.to_string()));
    Value::Map(m)
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_schema::reaction::Pattern;

    #[test]
    fn pattern_renders_with_link_edge_between_shared_bonds() {
        let p = Pattern::map([
            (
                "a",
                Pattern::sort(
                    "MEK",
                    [(
                        "outputs",
                        Pattern::map([("p", Pattern::link_var("bond"))]),
                    )],
                ),
            ),
            (
                "b",
                Pattern::sort(
                    "pERK",
                    [(
                        "outputs",
                        Pattern::map([("p", Pattern::link_var("bond"))]),
                    )],
                ),
            ),
        ]);
        let dot = render_pattern_dot(&p, "");
        assert!(dot.contains("// Link graph"));
        assert!(dot.contains("bond"));
        assert!(dot.contains("dir=none") && dot.contains("style=dashed"));
    }

    #[test]
    fn single_linkvar_has_no_edge() {
        // A "fresh" link variable on the reactum that doesn't pair
        // with anything in the redex — just one anchor, no line.
        let p = Pattern::sort(
            "X",
            [("outputs", Pattern::map([("p", Pattern::link_var("fresh"))]))],
        );
        let dot = render_pattern_dot(&p, "");
        // There IS a node for the port, but NO link-graph edge.
        assert!(dot.contains("fresh"));
        assert!(!dot.contains("// Link graph"));
    }

    #[test]
    fn absent_marker_renders() {
        let p = Pattern::map([(
            "x",
            Pattern::sort("MEK", [("outputs", Pattern::absent())]),
        )]);
        let dot = render_pattern_dot(&p, "");
        assert!(dot.contains("✗") || dot.contains("absent"));
    }
}

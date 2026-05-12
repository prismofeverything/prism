//! Top-down "Milner-style" SVG rendering of reaction-rule
//! [`Pattern`](prism_schema::reaction::Pattern)s.
//!
//! Each compartment is an **ellipse** containing its children
//! (recursively). The label sits at the top of the ellipse; children
//! are arranged in a horizontal strip just below. Sites are small
//! dashed ellipses, link variables are filled circles, and Absent
//! markers are small struck-through tags.
//!
//! The aesthetic is the one you see in Milner's *Space and Motion of
//! Communicating Agents* — a top-down view of nested compartments
//! (think: floor plan of a building) rather than a tree projection.
//! That complements the side-view tree produced by the Graphviz
//! pipeline ([`crate::render_state_dot`] / [`crate::render_pattern_dot`]):
//! the two readings of the same structure.
//!
//! Pure-Rust, no external dependencies — useful as a fallback when
//! `dot` isn't installed, or as a stylistic counterpoint in reports.

use prism_schema::reaction::Pattern;
use prism_schema::Value;

// ── Tunables ────────────────────────────────────────────────────────

/// Padding inside a compartment (between the ellipse border and the
/// children's bounding box). Generous so the inner ovals don't crowd
/// the outer curve.
const PAD: f64 = 26.0;
/// Horizontal gap between sibling children inside a compartment.
const GAP: f64 = 16.0;
/// Vertical space reserved for the compartment's label.
const LABEL_H: f64 = 22.0;
/// Multiplier converting the children's bounding rectangle to the
/// enclosing ellipse's horizontal axis.
const ELLIPSE_FIT_W: f64 = 1.18;
/// Vertical inflation factor — larger than the horizontal one so the
/// ovals come out rounder (taller relative to width) rather than the
/// squashed disks you get with isotropic inflation.
const ELLIPSE_FIT_H: f64 = 1.50;
/// Lower bound on the height-to-width ratio of any compartment
/// ellipse. Keeps long shallow trees from rendering as razor-thin
/// slivers; clamps to a minimum "round enough" aspect.
const MIN_ASPECT: f64 = 0.55;
/// Minimum width/height of any node.
const MIN_W: f64 = 72.0;
const MIN_H: f64 = 60.0;
/// Site marker (dashed ellipse).
const SITE_W: f64 = 62.0;
const SITE_H: f64 = 36.0;
/// LinkVar port (small filled circle).
const PORT_R: f64 = 7.0;
const FONT: &str = "-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif";

// ── Layout pass ─────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Lay {
    w: f64,
    h: f64,
    kind: LayKind,
}

#[derive(Clone, Debug)]
enum LayKind {
    /// A sort'd compartment with children laid out inside.
    Sort {
        label: String,
        children: Vec<(String, Lay)>,
    },
    /// Forest of regions (top-level multi-rooted bigraph).
    Forest {
        regions: Vec<(String, Lay)>,
    },
    Site,
    LinkVar(String),
    Absent(String),
    Atom(String),
}

fn layout(pat: &Pattern) -> Lay {
    match pat {
        Pattern::Site => Lay { w: SITE_W, h: SITE_H, kind: LayKind::Site },
        Pattern::Absent => {
            let label = "✗".to_string();
            let w = text_width(&label) + 16.0;
            Lay { w, h: 26.0, kind: LayKind::Absent(label) }
        }
        Pattern::LinkVar(name) => {
            // Width includes the xlabel hanging off the port.
            let w = 2.0 * PORT_R + 6.0 + text_width(name.as_str());
            Lay { w: w.max(2.0 * PORT_R + 4.0), h: 2.0 * PORT_R + 14.0, kind: LayKind::LinkVar(name.to_string()) }
        }
        Pattern::Atom(v) => {
            let s = atom_str(v);
            Lay { w: text_width(&s).max(40.0), h: 18.0, kind: LayKind::Atom(s) }
        }
        Pattern::List(items) => {
            let children: Vec<(String, Lay)> = items
                .iter()
                .enumerate()
                .map(|(i, p)| (format!("{i}"), layout(p)))
                .collect();
            forest_layout(children)
        }
        Pattern::Map(map) => {
            let sort = map
                .get("_type")
                .or_else(|| map.get("_control"))
                .and_then(|p| match p {
                    Pattern::Atom(v) => v.as_str().map(|s| s.to_string()),
                    _ => None,
                });
            let children: Vec<(String, Lay)> = map
                .iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .map(|(k, v)| {
                    let mut child = layout(v);
                    if let Pattern::Absent = v {
                        // Embed the redex key into the absent marker.
                        if let LayKind::Absent(_) = &child.kind {
                            let new_label = format!("✗ {k}");
                            child.w = text_width(&new_label) + 16.0;
                            child.kind = LayKind::Absent(new_label);
                        }
                    }
                    (k.to_string(), child)
                })
                .collect();
            match sort {
                Some(label) => compartment_layout(label, children),
                None => forest_layout(children),
            }
        }
    }
}

fn compartment_layout(label: String, children: Vec<(String, Lay)>) -> Lay {
    let (inner_w, inner_h) = pack_children(&children);
    let label_w = text_width(&label) + 24.0;
    let content_w = inner_w.max(label_w);
    let content_h = inner_h + LABEL_H;
    // Inflate independently so the resulting ellipse is rounder than
    // a naive isotropic scaling would produce.
    let w = (content_w + 2.0 * PAD).max(MIN_W) * ELLIPSE_FIT_W;
    let mut h = (content_h + 2.0 * PAD).max(MIN_H) * ELLIPSE_FIT_H;
    // Enforce a minimum height-to-width aspect — keeps shallow trees
    // from rendering as squashed disks.
    if h / w < MIN_ASPECT {
        h = w * MIN_ASPECT;
    }
    Lay { w, h, kind: LayKind::Sort { label, children } }
}

fn forest_layout(regions: Vec<(String, Lay)>) -> Lay {
    let (w, h) = pack_children(&regions);
    Lay {
        w: w + 2.0 * PAD,
        h: h + 2.0 * PAD,
        kind: LayKind::Forest { regions },
    }
}

/// Pack children horizontally and return (bounding_width, bounding_height).
fn pack_children(children: &[(String, Lay)]) -> (f64, f64) {
    if children.is_empty() {
        return (0.0, 0.0);
    }
    let total_w: f64 = children.iter().map(|(_, c)| c.w).sum::<f64>()
        + GAP * (children.len() as f64 - 1.0).max(0.0);
    let max_h: f64 = children.iter().map(|(_, c)| c.h).fold(0.0, f64::max);
    // Children get a small caption above them — account for it.
    (total_w, max_h + KEY_LABEL_H)
}

const KEY_LABEL_H: f64 = 12.0;

// ── Public renderers ────────────────────────────────────────────────

/// Render a pattern as a self-contained `<svg>...</svg>` string.
/// Top-down Milner-style nested ovals. Same-named link variables get
/// connected by a U-shaped curve in the link graph layer.
pub fn render_pattern_svg(pattern: &Pattern, title: &str) -> String {
    let lay = layout(pattern);
    let title_h = if title.is_empty() { 0.0 } else { 26.0 };
    let width = (lay.w + 2.0 * PAD).max(180.0);
    let base_height = lay.h + 2.0 * PAD + title_h;

    // Draw the place graph into a buffer, collecting ports.
    let mut body = String::new();
    let cx = width / 2.0;
    let cy = title_h + PAD + lay.h / 2.0;
    let mut ports: Vec<PortPos> = Vec::new();
    draw(&lay, cx, cy, &mut body, &mut ports);

    // Then the link graph, also into a buffer (after which we know
    // how far the bonds drop and can extend the SVG height).
    let mut links_body = String::new();
    let bond_max_y = draw_link_edges(&ports, &mut links_body);
    let height = base_height.max(bond_max_y + PAD);

    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         viewBox=\"0 0 {width:.0} {height:.0}\" width=\"100%\" \
         font-family=\"{FONT}\" font-size=\"11\">\n"
    ));
    if !title.is_empty() {
        out.push_str(&format!(
            "  <text x=\"{:.1}\" y=\"18\" font-size=\"14\" \
             font-weight=\"700\" fill=\"#333\">{}</text>\n",
            PAD,
            escape_xml(title),
        ));
    }
    out.push_str(&body);
    out.push_str(&links_body);
    out.push_str("</svg>");
    out
}

/// Where a [`Pattern::LinkVar`] port ended up on the canvas, plus
/// its name so the link-graph pass can connect same-named ports.
#[derive(Clone, Debug)]
struct PortPos {
    name: String,
    x: f64,
    y: f64,
}

/// Emit U-shaped curves for every pair of same-named ports. Returns
/// the maximum y any curve reaches, so the caller can extend the
/// SVG canvas if needed.
fn draw_link_edges(ports: &[PortPos], out: &mut String) -> f64 {
    let mut by_name: std::collections::BTreeMap<&str, Vec<&PortPos>> =
        std::collections::BTreeMap::new();
    for p in ports {
        by_name.entry(p.name.as_str()).or_default().push(p);
    }
    let mut max_y: f64 = 0.0;
    for (name, anchors) in by_name {
        if anchors.len() < 2 {
            continue;
        }
        let color = linkvar_color(name);
        for pair in anchors.windows(2) {
            let a = pair[0];
            let b = pair[1];
            let trough = draw_u_edge(a, b, name, color, out);
            if trough > max_y {
                max_y = trough;
            }
        }
    }
    max_y
}

/// Draw a single "U"-shaped two-ended edge between port `a` and
/// port `b`, labelled with `name` and stroked in `color`. The curve
/// drops to a horizontal trough below both endpoints — the shape used
/// in kaleidoscope and Milner's textbook diagrams for chemical bonds
/// and process-port wiring.
///
/// Uses a cubic Bézier with both control points placed at the same
/// y, well below both endpoints. As `b.y` moves relative to `a.y`
/// the trough tilts gracefully — the curve still reads as "drops
/// down and back up" for arbitrary two-port pairs, which is what we
/// want from a general 2-ended edge primitive.
/// Returns the y-coordinate of the trough (deepest point of the
/// curve plus the label's descender), so the caller can ensure the
/// SVG canvas extends far enough.
fn draw_u_edge(a: &PortPos, b: &PortPos, name: &str, color: &str, out: &mut String) -> f64 {
    let dx = (b.x - a.x).abs();
    let dy = (b.y - a.y).abs();
    // Drop depth scales with the horizontal span but is gently
    // bounded so very wide bonds don't dive halfway across the
    // canvas. The asymmetry term keeps the trough below both ports
    // even when one port is much lower than the other.
    let drop = (dx * 0.30).clamp(28.0, 70.0) + dy * 0.15;
    let trough_y = a.y.max(b.y) + drop;

    // Cubic Bézier: P0 → C1, C2 → P3.
    let c1x = a.x;
    let c1y = trough_y;
    let c2x = b.x;
    let c2y = trough_y;

    out.push_str(&format!(
        "  <path d=\"M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}\" \
         fill=\"none\" stroke=\"{color}\" stroke-width=\"2\" \
         stroke-dasharray=\"5 3\" stroke-linecap=\"round\" />\n",
        a.x, a.y, c1x, c1y, c2x, c2y, b.x, b.y,
    ));

    // Label at the trough's apex. For a cubic with both control
    // points at `trough_y`, the curve's actual minimum y is at
    // t = 0.5, sitting at the average of the endpoints and controls:
    //   y(0.5) = (a.y + 3*c1y + 3*c2y + b.y) / 8
    let label_x = (a.x + b.x) / 2.0;
    let label_y = (a.y + 3.0 * c1y + 3.0 * c2y + b.y) / 8.0 + 4.0;
    out.push_str(&format!(
        "  <text x=\"{label_x:.1}\" y=\"{label_y:.1}\" text-anchor=\"middle\" \
         fill=\"{color}\" font-size=\"10\" font-weight=\"600\">{}</text>\n",
        escape_xml(name),
    ));
    label_y + 4.0
}

/// Render a redex → reactum pair as a single side-by-side SVG. Each
/// half tracks its own link-variable ports — bonds inside the redex
/// don't cross over to the reactum side.
pub fn render_rule_pair_svg(
    label: &str,
    redex: &Pattern,
    reactum: &Pattern,
    rule_color: &str,
) -> String {
    let l = layout(redex);
    let r = layout(reactum);
    let arrow_w = 44.0;
    let gap = 16.0;
    let title_h = 30.0;
    let inner_w = l.w + arrow_w + 2.0 * gap + r.w + 2.0 * PAD;
    let base_h = l.h.max(r.h) + 2.0 * PAD + title_h;

    let y0 = title_h + PAD;
    let l_cx = PAD + l.w / 2.0;
    let l_cy = y0 + l.h / 2.0;
    let mut l_body = String::new();
    let mut l_ports: Vec<PortPos> = Vec::new();
    draw(&l, l_cx, l_cy, &mut l_body, &mut l_ports);
    let mut l_links = String::new();
    let l_bond_y = draw_link_edges(&l_ports, &mut l_links);

    let arrow_x = PAD + l.w + gap;
    let arrow_cy = y0 + l.h.max(r.h) / 2.0;
    let mut arrow = String::new();
    arrow.push_str(&format!(
        "  <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" \
         stroke=\"#777\" stroke-width=\"1.6\" />\n",
        arrow_x + 4.0, arrow_cy, arrow_x + arrow_w - 4.0, arrow_cy,
    ));
    let head_x = arrow_x + arrow_w - 4.0;
    arrow.push_str(&format!(
        "  <polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" \
         fill=\"#777\" />\n",
        head_x, arrow_cy,
        head_x - 8.0, arrow_cy - 5.0,
        head_x - 8.0, arrow_cy + 5.0,
    ));

    let r_cx = arrow_x + arrow_w + gap + r.w / 2.0;
    let r_cy = y0 + r.h / 2.0;
    let mut r_body = String::new();
    let mut r_ports: Vec<PortPos> = Vec::new();
    draw(&r, r_cx, r_cy, &mut r_body, &mut r_ports);
    let mut r_links = String::new();
    let r_bond_y = draw_link_edges(&r_ports, &mut r_links);

    let inner_h = base_h.max(l_bond_y + PAD).max(r_bond_y + PAD);

    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         viewBox=\"0 0 {inner_w:.0} {inner_h:.0}\" width=\"100%\" \
         font-family=\"{FONT}\" font-size=\"11\">\n"
    ));
    out.push_str(&format!(
        "  <text x=\"{:.1}\" y=\"20\" font-size=\"14\" \
         font-weight=\"700\" fill=\"{}\">{}</text>\n",
        PAD, rule_color, escape_xml(label),
    ));
    out.push_str(&l_body);
    out.push_str(&l_links);
    out.push_str(&arrow);
    out.push_str(&r_body);
    out.push_str(&r_links);
    out.push_str("</svg>");
    out
}

// ── Render pass ─────────────────────────────────────────────────────

/// Render a layout centred at `(cx, cy)`. `ports` accumulates
/// link-variable anchor positions so the caller can draw the link
/// graph after the place graph.
fn draw(lay: &Lay, cx: f64, cy: f64, out: &mut String, ports: &mut Vec<PortPos>) {
    match &lay.kind {
        LayKind::Sort { label, children } => {
            let (fill, stroke) = sort_colors(label);
            let rx = lay.w / 2.0;
            let ry = lay.h / 2.0;
            out.push_str(&format!(
                "  <ellipse cx=\"{cx:.1}\" cy=\"{cy:.1}\" rx=\"{rx:.1}\" ry=\"{ry:.1}\" \
                 fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1.6\" />\n",
            ));
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 font-weight=\"600\" fill=\"#333\">{}</text>\n",
                cy - ry + 16.0,
                escape_xml(label),
            ));
            draw_children(children, cx, cy - ry + LABEL_H + PAD, lay.w, out, ports);
        }
        LayKind::Forest { regions } => {
            draw_children(regions, cx, cy - lay.h / 2.0 + PAD, lay.w, out, ports);
        }
        LayKind::Site => {
            let rx = lay.w / 2.0;
            let ry = lay.h / 2.0;
            out.push_str(&format!(
                "  <ellipse cx=\"{cx:.1}\" cy=\"{cy:.1}\" rx=\"{rx:.1}\" ry=\"{ry:.1}\" \
                 fill=\"white\" stroke=\"#888\" stroke-width=\"1.2\" \
                 stroke-dasharray=\"4 3\" />\n",
            ));
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 fill=\"#666\" font-style=\"italic\">◦ site</text>\n",
                cy + 4.0,
            ));
        }
        LayKind::LinkVar(name) => {
            let color = linkvar_color(name);
            let port_cy = cy - lay.h / 2.0 + PORT_R + 4.0;
            out.push_str(&format!(
                "  <circle cx=\"{cx:.1}\" cy=\"{port_cy:.1}\" r=\"{PORT_R:.1}\" \
                 fill=\"{color}\" stroke=\"{color}\" stroke-width=\"1\" />\n",
            ));
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 fill=\"#444\" font-size=\"10\">{}</text>\n",
                port_cy + PORT_R + 10.0,
                escape_xml(name),
            ));
            ports.push(PortPos { name: name.clone(), x: cx, y: port_cy });
        }
        LayKind::Absent(label) => {
            let rx = lay.w / 2.0;
            let ry = lay.h / 2.0;
            out.push_str(&format!(
                "  <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" \
                 rx=\"6\" fill=\"#fff5f5\" stroke=\"#c0392b\" stroke-width=\"1.2\" \
                 stroke-dasharray=\"3 2\" />\n",
                cx - rx, cy - ry, lay.w, lay.h,
            ));
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 fill=\"#c0392b\" font-weight=\"600\" font-size=\"10\">{}</text>\n",
                cy + 4.0,
                escape_xml(label),
            ));
        }
        LayKind::Atom(s) => {
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 fill=\"#555\" font-style=\"italic\">{}</text>\n",
                cy + 4.0,
                escape_xml(s),
            ));
        }
    }
}

fn draw_children(
    children: &[(String, Lay)],
    cx_center: f64,
    y_top: f64,
    container_w: f64,
    out: &mut String,
    ports: &mut Vec<PortPos>,
) {
    if children.is_empty() {
        return;
    }
    let total_w: f64 = children.iter().map(|(_, c)| c.w).sum::<f64>()
        + GAP * (children.len() as f64 - 1.0).max(0.0);
    let mut x = cx_center - total_w / 2.0;
    for (key, child) in children {
        let cx = x + child.w / 2.0;
        // Optional key caption above the child (skipped for unnamed
        // markers like Site to reduce visual noise).
        let show_key = !matches!(
            &child.kind,
            LayKind::Site | LayKind::Absent(_) | LayKind::LinkVar(_) | LayKind::Atom(_)
        ) && !key.is_empty();
        let child_y = if show_key { y_top + KEY_LABEL_H } else { y_top };
        if show_key {
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 fill=\"#888\" font-size=\"9\">{}</text>\n",
                y_top + 9.0,
                escape_xml(key),
            ));
        }
        draw(child, cx, child_y + child.h / 2.0, out, ports);
        x += child.w + GAP;
    }
    let _ = container_w;
}

// ── Palette ─────────────────────────────────────────────────────────

/// (fill, stroke) for a sort label — matching the bigraph-viz palette
/// in `pattern_dot.rs` / `dot.rs` so the two renderings read the same.
fn sort_colors(label: &str) -> (&'static str, &'static str) {
    match label {
        "MEK" => ("#9ecae1", "#3b6fb0"),
        "ERK" => ("#7fbf7b", "#3a7a3a"),
        "pERK" => ("#ef8a62", "#b85a1a"),
        "Cell" => ("#f5f5f5", "#666666"),
        "Compartment" => ("#e8e4d8", "#777067"),
        "Cytoplasm" => ("#cfe3cf", "#5a7a5a"),
        "Nucleus" => ("#f0d8b4", "#7d5b3a"),
        "ERLumen" => ("#f5c6a4", "#a96a3a"),
        _ => ("#C3E1D6", "#98afa6"),
    }
}

/// Colour for a link variable name — hashed so the same variable
/// always renders in the same colour.
pub fn linkvar_color(name: &str) -> &'static str {
    const PALETTE: &[&str] = &[
        "#7570b3", "#d95f02", "#1b9e77", "#e7298a", "#66a61e", "#a6611a", "#e6ab02",
    ];
    let mut h: u32 = 5381;
    for b in name.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    PALETTE[(h as usize) % PALETTE.len()]
}

/// Stable color per rule label. Used by the MAPK report.
pub const RULE_COLORS: &[(&str, &str)] = &[
    ("phosphorylate", "#7570b3"),
    ("dissociate", "#d95f02"),
    ("dephosphorylate", "#a6611a"),
    ("translocate_erk_in", "#1b9e77"),
    ("translocate_erk_out", "#5fbf94"),
    ("translocate_perk_in", "#117a59"),
    ("translocate_perk_out", "#66c2a5"),
];

pub fn rule_color(label: &str) -> &'static str {
    RULE_COLORS
        .iter()
        .find(|(name, _)| *name == label)
        .map(|(_, c)| *c)
        .unwrap_or("#666666")
}

// ── Tiny helpers ────────────────────────────────────────────────────

fn atom_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format!("{}", f.0),
        Value::Bool(b) => b.to_string(),
        Value::None => "none".to_string(),
        _ => "…".to_string(),
    }
}

fn text_width(s: &str) -> f64 {
    7.0 * s.chars().count() as f64 + 8.0
}

fn escape_xml(s: &str) -> String {
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
    fn renders_top_down_ellipse() {
        let p = Pattern::sort("Compartment", [("x", Pattern::site())]);
        let svg = render_pattern_svg(&p, "test");
        assert!(svg.contains("<ellipse"));
        assert!(svg.contains("Compartment"));
    }

    #[test]
    fn nested_compartments_nest_visually() {
        let p = Pattern::map([(
            "region0",
            Pattern::sort(
                "Cell",
                [(
                    "cyto",
                    Pattern::sort(
                        "Compartment",
                        [("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new()))],
                    ),
                )],
            ),
        )]);
        let svg = render_pattern_svg(&p, "");
        // Multiple ellipses, one per compartment.
        assert!(svg.matches("<ellipse").count() >= 3);
    }

    #[test]
    fn pair_renders_both_sides() {
        let a = Pattern::sort("ERK", Vec::<(&str, Pattern)>::new());
        let b = Pattern::sort("pERK", Vec::<(&str, Pattern)>::new());
        let svg = render_rule_pair_svg("phos", &a, &b, "#333");
        assert!(svg.contains("ERK"));
        assert!(svg.contains("pERK"));
        assert!(svg.contains("phos"));
    }
}

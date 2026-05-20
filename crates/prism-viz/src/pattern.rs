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
/// children's bounding box).
const PAD: f64 = 16.0;
/// Gap between sibling children in a uniform-slot grid.
const GAP: f64 = 10.0;
/// Vertical space reserved for the compartment's label band.
const LABEL_H: f64 = 26.0;
/// Vertical space reserved for child-key captions just above a child.
const KEY_LABEL_H: f64 = 14.0;
/// Multiplier converting the children's bounding rectangle to the
/// enclosing ellipse's horizontal axis. The inscribing inequality
/// is `(w/2a)² + (h/2b)² ≤ 1`; √2 ≈ 1.414 just touches every corner.
/// We keep a small margin above that.
const ELLIPSE_FIT_W: f64 = 1.22;
const ELLIPSE_FIT_H: f64 = 1.16;
/// Lower bound on the height-to-width ratio of any compartment
/// ellipse. Keeps long shallow trees from rendering as razor-thin
/// slivers; clamps to a minimum "round enough" aspect.
const MIN_ASPECT: f64 = 0.55;
/// Upper bound on h/w. When the uniform-slot grid produces a
/// nearly-square content box (common when a compartment has a single
/// nested sub-compartment), the enclosing ellipse comes out as a
/// circle. We grow `w` instead of cropping `h`, so content stays
/// uncropped but the shape reads as a horizontal oval.
const MAX_ASPECT: f64 = 0.92;
/// Uniform leaf size — every Site, LinkVar, Absent and Atom renders
/// into a box of these dimensions, regardless of inner content. Any
/// container then sizes its slots to `max(child)` over all siblings,
/// so a row of leaves stays even and intermediate compartments grow
/// only as much as their largest child demands.
const LEAF_W: f64 = 110.0;
const LEAF_H: f64 = 62.0;
/// Minimum width/height of any node.
const MIN_W: f64 = LEAF_W;
const MIN_H: f64 = LEAF_H;
/// LinkVar port (filled circle).
const PORT_R: f64 = 8.0;
/// Font sizes (in SVG units). The SVG is small enough that these
/// also render at roughly these pixel sizes in the report — bigger
/// SVG dimensions would shrink them through `width="100%"` scaling.
const FONT_LABEL: f64 = 22.0;
const FONT_BODY: f64 = 18.0;
const FONT_KEY: f64 = 14.0;
const FONT: &str = "-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif";

// ── Layout pass ─────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Lay {
    w: f64,
    h: f64,
    kind: LayKind,
}

#[derive(Clone, Debug)]
struct PositionedChild {
    key: String,
    lay: Lay,
    /// Position relative to the parent's centre.
    dx: f64,
    dy: f64,
}

#[derive(Clone, Debug)]
enum LayKind {
    /// A sort'd compartment with children laid out inside.
    Sort {
        label: String,
        children: Vec<PositionedChild>,
    },
    /// Forest of regions (top-level multi-rooted bigraph).
    Forest {
        regions: Vec<PositionedChild>,
    },
    Site,
    LinkVar(String),
    Absent(String),
    Atom(String),
}

fn layout(pat: &Pattern) -> Lay {
    match pat {
        Pattern::Site => Lay { w: LEAF_W, h: LEAF_H, kind: LayKind::Site },
        Pattern::Absent => Lay {
            w: LEAF_W,
            h: LEAF_H,
            kind: LayKind::Absent("✗".to_string()),
        },
        Pattern::LinkVar(name) => Lay {
            w: LEAF_W,
            h: LEAF_H,
            kind: LayKind::LinkVar(name.to_string()),
        },
        Pattern::Atom(v) => Lay {
            w: LEAF_W,
            h: LEAF_H,
            kind: LayKind::Atom(atom_str(v)),
        },
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
                            child.kind = LayKind::Absent(format!("✗ {k}"));
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
        // An as-pattern (`?name::Sort`) renders as its inner pattern.
        Pattern::Bind { inner, .. } => layout(inner),
    }
}

fn compartment_layout(label: String, children: Vec<(String, Lay)>) -> Lay {
    let (positioned, content_w, content_h) = position_children(children, true);
    let label_w = text_width(&label, FONT_LABEL) + 36.0;
    let content_w = content_w.max(label_w);
    let mut w = (content_w + 2.0 * PAD).max(MIN_W) * ELLIPSE_FIT_W;
    let mut h = (content_h + LABEL_H + 2.0 * PAD).max(MIN_H) * ELLIPSE_FIT_H;
    if h / w < MIN_ASPECT {
        h = w * MIN_ASPECT;
    }
    if h / w > MAX_ASPECT {
        w = h / MAX_ASPECT;
    }
    Lay { w, h, kind: LayKind::Sort { label, children: positioned } }
}

fn forest_layout(regions: Vec<(String, Lay)>) -> Lay {
    let (positioned, w, h) = position_children(regions, false);
    Lay {
        w: w + 2.0 * PAD,
        h: h + 2.0 * PAD,
        kind: LayKind::Forest { regions: positioned },
    }
}

/// Place each child at a position relative to the parent's centre.
/// Returns `(positioned_children, bounding_w, bounding_h)`.
///
/// For ≤ 4 children we use a single horizontal row of heterogeneous
/// slots — leaves sit at `LEAF_*` size, sub-compartments at their
/// natural size — so a sub-compartment never forces its leaf siblings
/// to inflate to its size. The container's bounding rectangle is
/// rectangular (wide and short), and the enclosing ellipse comes out
/// horizontal, which avoids the near-circle look uniform 2×2 grids
/// produced.
///
/// For ≥ 5 children we fall back to a near-square uniform-slot grid
/// (`cols ≈ √n`) so the row doesn't stretch off the canvas.
fn position_children(
    children: Vec<(String, Lay)>,
    under_label: bool,
) -> (Vec<PositionedChild>, f64, f64) {
    let n = children.len();
    if n == 0 {
        return (vec![], 0.0, 0.0);
    }
    let label_bias = if under_label { LABEL_H / 2.0 } else { 0.0 };

    if n <= 4 {
        // Heterogeneous horizontal row: each child keeps its own
        // width; row height is the tallest child.
        let max_h = children.iter().map(|(_, c)| c.h).fold(0.0_f64, f64::max);
        let total_w: f64 = children.iter().map(|(_, c)| c.w).sum::<f64>()
            + GAP * (n.saturating_sub(1) as f64);
        let mut x_cursor = -total_w / 2.0;
        let mut positioned = Vec::with_capacity(n);
        for (key, lay) in children {
            let dx = x_cursor + lay.w / 2.0;
            let dy = label_bias;
            x_cursor += lay.w + GAP;
            positioned.push(PositionedChild { key, lay, dx, dy });
        }
        return (positioned, total_w, max_h + KEY_LABEL_H);
    }

    // n >= 5: uniform-slot grid (cols = ceil(√n)).
    let slot_w = children.iter().map(|(_, c)| c.w).fold(0.0_f64, f64::max);
    let slot_h = children.iter().map(|(_, c)| c.h).fold(0.0_f64, f64::max);
    let cols = (n as f64).sqrt().ceil().max(1.0) as usize;
    let rows = (n + cols - 1) / cols;
    let cell_w = slot_w + GAP;
    let cell_h = slot_h + KEY_LABEL_H + GAP;
    let mut positioned = Vec::with_capacity(n);
    for (i, (key, lay)) in children.into_iter().enumerate() {
        let r = i / cols;
        let c = i % cols;
        let row_count = if r == rows - 1 {
            n - r * cols
        } else {
            cols
        };
        let row_w = row_count as f64 * cell_w;
        let row_start_x = -row_w / 2.0;
        let dx = row_start_x + (c as f64 + 0.5) * cell_w;
        let col_start_y = -(rows as f64) * cell_h / 2.0 + cell_h / 2.0;
        let dy = col_start_y + r as f64 * cell_h + label_bias;
        positioned.push(PositionedChild { key, lay, dx, dy });
    }
    let bounding_w = cols as f64 * cell_w;
    let bounding_h = rows as f64 * cell_h;
    (positioned, bounding_w, bounding_h)
}

// ── Public renderers ────────────────────────────────────────────────

/// Render a pattern as a self-contained `<svg>...</svg>` string.
/// Top-down Milner-style nested ovals. Same-named link variables get
/// connected by an upward arch in the link graph layer; the canvas
/// grows at the top to accommodate.
pub fn render_pattern_svg(pattern: &Pattern, title: &str) -> String {
    let lay = layout(pattern);
    let title_h = if title.is_empty() { 0.0 } else { 36.0 };
    let width = (lay.w + 2.0 * PAD).max(220.0);
    let base_height = lay.h + 2.0 * PAD + title_h;

    // Draw the place graph first at a provisional y origin (we may
    // need to shift it down later if upward arches need extra room).
    let mut body = String::new();
    let cx = width / 2.0;
    let cy = title_h + PAD + lay.h / 2.0;
    let mut ports: Vec<PortPos> = Vec::new();
    draw(&lay, cx, cy, &mut body, &mut ports);

    // Then the link graph (arches go up). If `bond_min_y` is above
    // the body's top edge, we need to push everything down by the
    // overflow.
    let mut links_body = String::new();
    let bond_min_y = draw_link_edges(&ports, &mut links_body);
    let top_overflow = (title_h + 8.0 - bond_min_y).max(0.0);
    let height = base_height + top_overflow;
    let shift = top_overflow;

    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         viewBox=\"0 0 {width:.0} {height:.0}\" width=\"100%\" \
         font-family=\"{FONT}\" font-size=\"{FONT_BODY}\">\n"
    ));
    if !title.is_empty() {
        out.push_str(&format!(
            "  <text x=\"{:.1}\" y=\"24\" font-size=\"{FONT_LABEL}\" \
             font-weight=\"700\" fill=\"#333\">{}</text>\n",
            PAD,
            escape_xml(title),
        ));
    }
    if shift > 0.0 {
        out.push_str(&format!("  <g transform=\"translate(0,{shift:.1})\">\n"));
    }
    out.push_str(&body);
    out.push_str(&links_body);
    if shift > 0.0 {
        out.push_str("  </g>\n");
    }
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

/// Emit upward-arching curves for every pair of same-named ports.
/// Returns the *minimum* y any curve reaches (i.e. how far above the
/// place-graph the trough rises), so the caller can extend the SVG
/// canvas top-side if the arch would otherwise clip.
fn draw_link_edges(ports: &[PortPos], out: &mut String) -> f64 {
    let mut by_name: std::collections::BTreeMap<&str, Vec<&PortPos>> =
        std::collections::BTreeMap::new();
    for p in ports {
        by_name.entry(p.name.as_str()).or_default().push(p);
    }
    let mut min_y: f64 = f64::INFINITY;
    for (name, anchors) in by_name {
        if anchors.len() < 2 {
            continue;
        }
        let color = linkvar_color(name);
        for pair in anchors.windows(2) {
            let a = pair[0];
            let b = pair[1];
            let arch_top = draw_u_edge(a, b, name, color, out);
            if arch_top < min_y {
                min_y = arch_top;
            }
        }
    }
    if min_y.is_infinite() {
        0.0
    } else {
        min_y
    }
}

/// Draw a single arched two-ended edge between port `a` and port
/// `b`, labelled with `name` and stroked in `color`. The curve rises
/// to a horizontal trough *above* both endpoints — Milner's textbook
/// rendering for chemical bonds and process-port wiring.
///
/// Returns the smallest y the curve reaches (top of arch plus the
/// label's ascender), so the caller can grow the SVG canvas upward
/// if the arch would otherwise clip.
fn draw_u_edge(a: &PortPos, b: &PortPos, name: &str, color: &str, out: &mut String) -> f64 {
    let dx = (b.x - a.x).abs();
    let dy = (b.y - a.y).abs();
    // Rise scales with the horizontal span but is gently bounded so
    // very wide bonds don't shoot off the canvas top. The asymmetry
    // term keeps the trough above both ports even when one port is
    // much higher than the other.
    let rise = (dx * 0.30).clamp(48.0, 110.0) + dy * 0.15;
    let trough_y = a.y.min(b.y) - rise;

    // Cubic Bézier: P0 → C1, C2 → P3.
    let c1x = a.x;
    let c1y = trough_y;
    let c2x = b.x;
    let c2y = trough_y;

    out.push_str(&format!(
        "  <path d=\"M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}\" \
         fill=\"none\" stroke=\"{color}\" stroke-width=\"2.6\" \
         stroke-dasharray=\"6 4\" stroke-linecap=\"round\" />\n",
        a.x, a.y, c1x, c1y, c2x, c2y, b.x, b.y,
    ));

    // Label at the curve's apex (cubic minimum y at t=0.5):
    //   y(0.5) = (a.y + 3*c1y + 3*c2y + b.y) / 8
    let label_x = (a.x + b.x) / 2.0;
    let label_apex = (a.y + 3.0 * c1y + 3.0 * c2y + b.y) / 8.0;
    let label_y = label_apex - 8.0;
    out.push_str(&format!(
        "  <text x=\"{label_x:.1}\" y=\"{label_y:.1}\" text-anchor=\"middle\" \
         fill=\"{color}\" font-size=\"{FONT_BODY}\" font-weight=\"600\">{}</text>\n",
        escape_xml(name),
    ));
    label_y - FONT_BODY
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
    let arrow_w = 64.0;
    let gap = 24.0;
    let title_h = 44.0;
    let inner_w = l.w + arrow_w + 2.0 * gap + r.w + 2.0 * PAD;
    let base_h = l.h.max(r.h) + 2.0 * PAD + title_h;

    let y0 = title_h + PAD;
    let l_cx = PAD + l.w / 2.0;
    let l_cy = y0 + l.h / 2.0;
    let mut l_body = String::new();
    let mut l_ports: Vec<PortPos> = Vec::new();
    draw(&l, l_cx, l_cy, &mut l_body, &mut l_ports);
    let mut l_links = String::new();
    let l_bond_min_y = draw_link_edges(&l_ports, &mut l_links);

    let arrow_x = PAD + l.w + gap;
    let arrow_cy = y0 + l.h.max(r.h) / 2.0;
    let mut arrow = String::new();
    arrow.push_str(&format!(
        "  <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" \
         stroke=\"#777\" stroke-width=\"2.2\" />\n",
        arrow_x + 4.0, arrow_cy, arrow_x + arrow_w - 6.0, arrow_cy,
    ));
    let head_x = arrow_x + arrow_w - 6.0;
    arrow.push_str(&format!(
        "  <polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" \
         fill=\"#777\" />\n",
        head_x, arrow_cy,
        head_x - 12.0, arrow_cy - 8.0,
        head_x - 12.0, arrow_cy + 8.0,
    ));

    let r_cx = arrow_x + arrow_w + gap + r.w / 2.0;
    let r_cy = y0 + r.h / 2.0;
    let mut r_body = String::new();
    let mut r_ports: Vec<PortPos> = Vec::new();
    draw(&r, r_cx, r_cy, &mut r_body, &mut r_ports);
    let mut r_links = String::new();
    let r_bond_min_y = draw_link_edges(&r_ports, &mut r_links);

    let top_overflow = (title_h + 8.0 - l_bond_min_y.min(r_bond_min_y)).max(0.0);
    let inner_h = base_h + top_overflow;
    let shift = top_overflow;

    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         viewBox=\"0 0 {inner_w:.0} {inner_h:.0}\" width=\"100%\" \
         font-family=\"{FONT}\" font-size=\"{FONT_BODY}\">\n"
    ));
    out.push_str(&format!(
        "  <text x=\"{:.1}\" y=\"30\" font-size=\"{FONT_LABEL}\" \
         font-weight=\"700\" fill=\"{}\">{}</text>\n",
        PAD, rule_color, escape_xml(label),
    ));
    if shift > 0.0 {
        out.push_str(&format!("  <g transform=\"translate(0,{shift:.1})\">\n"));
    }
    out.push_str(&l_body);
    out.push_str(&l_links);
    out.push_str(&arrow);
    out.push_str(&r_body);
    out.push_str(&r_links);
    if shift > 0.0 {
        out.push_str("  </g>\n");
    }
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
                 fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2.2\" />\n",
            ));
            // Label baseline sits inside the label band at the top.
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 font-size=\"{FONT_LABEL}\" font-weight=\"700\" fill=\"#222\">{}</text>\n",
                cy - ry + LABEL_H * 0.85,
                escape_xml(label),
            ));
            draw_positioned(children, cx, cy, out, ports);
        }
        LayKind::Forest { regions } => {
            draw_positioned(regions, cx, cy, out, ports);
        }
        LayKind::Site => {
            // Dashed ellipse fills the whole leaf box.
            let rx = lay.w / 2.0 - 6.0;
            let ry = lay.h / 2.0 - 6.0;
            out.push_str(&format!(
                "  <ellipse cx=\"{cx:.1}\" cy=\"{cy:.1}\" rx=\"{rx:.1}\" ry=\"{ry:.1}\" \
                 fill=\"white\" stroke=\"#888\" stroke-width=\"1.8\" \
                 stroke-dasharray=\"7 5\" />\n",
            ));
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 font-size=\"{FONT_BODY}\" fill=\"#666\" font-style=\"italic\">◦ site</text>\n",
                cy + FONT_BODY * 0.35,
            ));
        }
        LayKind::LinkVar(name) => {
            let color = linkvar_color(name);
            let port_cy = cy - lay.h / 4.0;
            out.push_str(&format!(
                "  <circle cx=\"{cx:.1}\" cy=\"{port_cy:.1}\" r=\"{PORT_R:.1}\" \
                 fill=\"{color}\" stroke=\"{color}\" stroke-width=\"1.4\" />\n",
            ));
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 font-size=\"{FONT_BODY}\" font-weight=\"600\" fill=\"#444\">{}</text>\n",
                port_cy + PORT_R + FONT_BODY + 4.0,
                escape_xml(name),
            ));
            ports.push(PortPos { name: name.clone(), x: cx, y: port_cy });
        }
        LayKind::Absent(label) => {
            let pad = 8.0;
            out.push_str(&format!(
                "  <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" \
                 rx=\"10\" fill=\"#fff5f5\" stroke=\"#c0392b\" stroke-width=\"1.8\" \
                 stroke-dasharray=\"5 4\" />\n",
                cx - lay.w / 2.0 + pad,
                cy - lay.h / 2.0 + pad,
                lay.w - 2.0 * pad,
                lay.h - 2.0 * pad,
            ));
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 font-size=\"{FONT_BODY}\" fill=\"#c0392b\" font-weight=\"700\">{}</text>\n",
                cy + FONT_BODY * 0.35,
                escape_xml(label),
            ));
        }
        LayKind::Atom(s) => {
            out.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 font-size=\"{FONT_BODY}\" fill=\"#555\" font-style=\"italic\">{}</text>\n",
                cy + FONT_BODY * 0.35,
                escape_xml(s),
            ));
        }
    }
}

fn draw_positioned(
    children: &[PositionedChild],
    parent_cx: f64,
    parent_cy: f64,
    out: &mut String,
    ports: &mut Vec<PortPos>,
) {
    for pc in children {
        let child_cx = parent_cx + pc.dx;
        let child_cy = parent_cy + pc.dy;
        let show_key = !matches!(
            &pc.lay.kind,
            LayKind::Site | LayKind::Absent(_) | LayKind::LinkVar(_) | LayKind::Atom(_)
        ) && !pc.key.is_empty();
        if show_key {
            // Caption just above the child's bounding box.
            out.push_str(&format!(
                "  <text x=\"{child_cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 font-size=\"{FONT_KEY}\" font-weight=\"500\" fill=\"#666\">{}</text>\n",
                child_cy - pc.lay.h / 2.0 - 8.0,
                escape_xml(&pc.key),
            ));
        }
        draw(&pc.lay, child_cx, child_cy, out, ports);
    }
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

/// Estimate the rendered width of `s` at `font_size`. Rough average
/// glyph advance ≈ 0.58 · font_size for sans-serif body text.
fn text_width(s: &str, font_size: f64) -> f64 {
    0.58 * font_size * s.chars().count() as f64 + 8.0
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

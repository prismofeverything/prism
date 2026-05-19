//! Biology-style cell-cartoon SVG renderer for MAPK BRS states.
//!
//! Ported from spatio-flux's matplotlib code: cell anatomy (plasma
//! membrane, nucleus, ER lumen) drawn as organic shapes, with
//! MEK / ERK / pERK as bilobal-kinase silhouettes placed in fixed
//! per-compartment slots. The same coordinate system feeds the
//! static snapshot renderer ([`render_cell_snapshot_svg`]) and the
//! SMIL-animated trajectory renderer ([`render_cell_animation_svg`]).
//!
//! Coordinate system note. Layout constants are kept in matplotlib
//! `[0, 1] × [0, 1.04]` axis coords with y=0 at the bottom, for
//! parity with the Python original. The [`px`] / [`py`] helpers
//! flip y to SVG's top-down convention. Ellipse rotation angles
//! flip sign for the same reason.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use prism_schema::Value;

// ── Canvas size ───────────────────────────────────────────────────
//
// The matplotlib version uses `ax.set_aspect('equal')` so an axis
// step of 1 in x equals 1 in y in screen pixels — the data area
// renders as a square (well, 1.0 × 1.04). Mirror that here so the
// cell and its organelles keep their intended shape: choose
// `CELL_VIEW_H = CELL_VIEW_W * 1.04` and the [`px`] / [`py`]
// helpers below come out at equal scale.
pub const CELL_VIEW_W: f64 = 640.0;
pub const CELL_VIEW_H: f64 = CELL_VIEW_W * 1.04;

#[inline]
fn px(x: f64) -> f64 {
    x * CELL_VIEW_W
}
#[inline]
fn py(y: f64) -> f64 {
    (1.04 - y) * (CELL_VIEW_H / 1.04)
}

/// Convert a matplotlib radius (units of x ∈ [0, 1]) to SVG pixels.
#[inline]
fn pr(r: f64) -> f64 {
    r * CELL_VIEW_W
}
/// Same for y-radii (units of y ∈ [0, 1.04]) so vertical extents in
/// matplotlib match SVG visually.
#[inline]
fn pry(r: f64) -> f64 {
    r * (CELL_VIEW_H / 1.04)
}

// ── Anatomy positions (in matplotlib axis coords) ─────────────────
const CELL_X0: f64 = 0.04;
const CELL_Y0: f64 = 0.06;
const CELL_X1: f64 = 0.96;
const CELL_Y1: f64 = 0.92;
const NUC_CX: f64 = 0.30;
const NUC_CY: f64 = 0.54;
const NUC_R: f64 = 0.17;
const ER_CX: f64 = 0.72;
const ER_CY: f64 = 0.55;
const ER_HW: f64 = 0.18;
const ER_HH: f64 = 0.22;

// ── Palette ───────────────────────────────────────────────────────
const MEK_FACE_N: &str = "#9ecae1";
const MEK_FACE_C: &str = "#74a9cf";
const MEK_EDGE: &str = "#1c4a78";
const ERK_FACE_N: &str = "#abdb98";
const ERK_FACE_C: &str = "#7fbf7b";
const ERK_EDGE: &str = "#3a7d3a";
const PERK_FACE_N: &str = "#f6a285";
const PERK_FACE_C: &str = "#ef8a62";
const PERK_EDGE: &str = "#a04020";

// ── Public API ────────────────────────────────────────────────────

/// Render one cell state as a self-contained `<svg>` string.
pub fn render_cell_snapshot_svg(state: &Value, title: &str) -> String {
    let mut body = String::new();
    let title_h = if title.is_empty() { 0.0 } else { 24.0 };
    let view_h = CELL_VIEW_H + title_h;

    body.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         viewBox=\"0 0 {:.0} {:.0}\" width=\"100%\" \
         font-family=\"-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif\">\n",
        CELL_VIEW_W, view_h
    ));
    if !title.is_empty() {
        body.push_str(&format!(
            "  <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
             font-size=\"13\" font-weight=\"600\" fill=\"#222\">{}</text>\n",
            CELL_VIEW_W / 2.0,
            16.0,
            escape_xml(title),
        ));
    }
    body.push_str(&format!("  <g transform=\"translate(0,{title_h:.0})\">\n"));

    draw_cell(&mut body);
    let layout_data = layout_state(state);
    let nucleus_present = has_compartment_kind(state, "Nucleus");
    let er_present = has_compartment_kind(state, "ERLumen");
    if nucleus_present {
        draw_nucleus(&mut body);
    }
    if er_present {
        draw_er_lumen(&mut body);
    }
    // Cell label above the membrane
    body.push_str(&format!(
        "    <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
         font-size=\"11\" font-style=\"italic\" fill=\"#3a5b3a\">cell</text>\n",
        px(0.5),
        py(0.97)
    ));

    // Molecules
    for entry in &layout_data {
        match entry.control.as_str() {
            "MEK" => draw_mek(&mut body, entry.x, entry.y, &entry.label, entry.wire.is_some(), 1.0),
            "ERK" => draw_erk(&mut body, entry.x, entry.y, &entry.label, false, 1.0),
            "pERK" => draw_erk(&mut body, entry.x, entry.y, &entry.label, true, 1.0),
            _ => {}
        }
    }

    // Bonds (pairs of endpoints sharing a wire)
    let mut by_wire: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    for entry in &layout_data {
        if let Some(w) = &entry.wire {
            by_wire.entry(w.clone()).or_default().push((entry.x, entry.y));
        }
    }
    for (_, pts) in &by_wire {
        for i in 0..pts.len() {
            for j in (i + 1)..pts.len() {
                draw_bond(&mut body, pts[i].0, pts[i].1, pts[j].0, pts[j].1, 1.0);
            }
        }
    }

    body.push_str("  </g>\n");
    body.push_str("</svg>");
    body
}

/// Render an animated walk through a sequence of states. Each
/// successive snapshot pair becomes one "transition" segment of the
/// animation. Within a segment, position is linearly interpolated
/// and control / bond changes cross-fade at the midpoint.
pub fn render_cell_animation_svg(
    states: &[Value],
    times: &[f64],
    seconds_per_transition: f64,
) -> String {
    assert!(states.len() == times.len());
    if states.is_empty() {
        return render_cell_snapshot_svg(
            &Value::tree(Vec::<(&str, Value)>::new()),
            "",
        );
    }
    if states.len() == 1 {
        return render_cell_snapshot_svg(&states[0], &format!("t = {:.1}", times[0]));
    }

    // Compute per-snapshot layouts so we can emit keyTimes/values.
    let layouts: Vec<Vec<LayoutEntry>> = states.iter().map(layout_state).collect();

    // Collect the union of entity labels across all snapshots so
    // each gets a single <g> with animateTransform.
    let mut all_labels: Vec<String> = Vec::new();
    {
        let mut seen = std::collections::BTreeSet::new();
        for layout_data in &layouts {
            for e in layout_data {
                if seen.insert(e.label.clone()) {
                    all_labels.push(e.label.clone());
                }
            }
        }
    }

    let n_transitions = states.len() - 1;
    let total_dur = seconds_per_transition * n_transitions as f64;

    // keyTimes: 0, 1/n, 2/n, ..., 1 — one entry per snapshot.
    let key_times: Vec<f64> = (0..states.len())
        .map(|i| i as f64 / n_transitions as f64)
        .collect();
    let key_times_str = key_times
        .iter()
        .map(|v| format!("{v:.4}"))
        .collect::<Vec<_>>()
        .join(";");

    let mut body = String::new();
    body.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         viewBox=\"0 0 {:.0} {:.0}\" width=\"100%\" \
         font-family=\"-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif\">\n",
        CELL_VIEW_W,
        CELL_VIEW_H + 28.0,
    ));

    // Static title — `textContent` is not a real SMIL-animatable
    // attribute (browsers silently drop it). The animated content
    // itself shows the dynamics; the title just frames it.
    let t_first = times.first().copied().unwrap_or(0.0);
    let t_last = times.last().copied().unwrap_or(0.0);
    body.push_str(&format!(
        "  <text x=\"{:.1}\" y=\"16\" text-anchor=\"middle\" \
         font-size=\"13\" font-weight=\"600\" fill=\"#222\">MAPK cell — t = {:.1} \u{2192} {:.1}</text>\n",
        CELL_VIEW_W / 2.0,
        t_first,
        t_last,
    ));

    body.push_str("  <g transform=\"translate(0,24)\">\n");

    // Static anatomy (cell membrane, nucleus, ER, cell label).
    draw_cell(&mut body);
    // Show nucleus/ER if present in any snapshot.
    let nuc_any = states.iter().any(|s| has_compartment_kind(s, "Nucleus"));
    let er_any = states.iter().any(|s| has_compartment_kind(s, "ERLumen"));
    if nuc_any {
        draw_nucleus(&mut body);
    }
    if er_any {
        draw_er_lumen(&mut body);
    }
    body.push_str(&format!(
        "    <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
         font-size=\"11\" font-style=\"italic\" fill=\"#3a5b3a\">cell</text>\n",
        px(0.5),
        py(0.97),
    ));

    // Per-entity animated group.
    for label in &all_labels {
        emit_animated_entity(&mut body, label, &layouts, &key_times_str, total_dur);
    }

    // Bonds: animate per-wire endpoint pairs. Approach: for each
    // distinct (substrate-name) seen as bound in any snapshot, emit
    // a <line> from its position to its MEK partner with opacity
    // animation toggling per-snapshot.
    emit_animated_bonds(&mut body, &all_labels, &layouts, &key_times_str, total_dur);

    body.push_str("  </g>\n");
    body.push_str("</svg>");
    body
}

// ── State traversal / layout ──────────────────────────────────────

#[derive(Clone, Debug)]
struct LayoutEntry {
    label: String,
    x: f64,
    y: f64,
    control: String,
    wire: Option<String>,
}

fn layout_state(state: &Value) -> Vec<LayoutEntry> {
    let mut entries: Vec<LayoutEntry> = Vec::new();
    walk_state(state, None, &mut entries);

    // Cleft-snap: any (MEK, substrate) pair sharing a wire — move
    // the substrate to MEK's cleft so binding reads visually.
    let mut by_wire: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate() {
        if let Some(w) = &e.wire {
            by_wire.entry(w.clone()).or_default().push(i);
        }
    }
    for (_, idxs) in by_wire {
        if idxs.len() != 2 {
            continue;
        }
        let (mek_idx, sub_idx) = {
            let (a, b) = (&entries[idxs[0]], &entries[idxs[1]]);
            if a.control == "MEK" && b.control != "MEK" {
                (idxs[0], idxs[1])
            } else if b.control == "MEK" && a.control != "MEK" {
                (idxs[1], idxs[0])
            } else {
                continue;
            }
        };
        let (mek_x, mek_y) = (entries[mek_idx].x, entries[mek_idx].y);
        entries[sub_idx].x = mek_x + 0.045;
        entries[sub_idx].y = mek_y - 0.005;
    }
    entries
}

fn walk_state(node: &Value, comp_name: Option<&str>, entries: &mut Vec<LayoutEntry>) {
    let map = match node.as_map() {
        Some(m) => m,
        None => return,
    };
    for (k, v) in map {
        let child_map = match v.as_map() {
            Some(m) => m,
            None => continue,
        };
        let ctrl = child_map
            .get("_type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        if ctrl == "Compartment" {
            walk_state(v, Some(k.as_str()), entries);
            continue;
        }
        if matches!(ctrl.as_str(), "MEK" | "ERK" | "pERK") {
            let label = child_map
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or(k.as_str())
                .to_string();
            let comp = comp_name.unwrap_or("");
            let (x, y) = position_in_compartment(comp, &label, &ctrl);
            let wire = wire_from_outputs(child_map.get("outputs"));
            entries.push(LayoutEntry {
                label,
                x,
                y,
                control: ctrl,
                wire,
            });
            continue;
        }
        // Unknown tag — recurse so we don't drop nested compartments.
        walk_state(v, comp_name, entries);
    }
}

fn wire_from_outputs(outs: Option<&Value>) -> Option<String> {
    let outs_map = outs.and_then(|v| v.as_map())?;
    let first = outs_map.values().next()?;
    let list = first.as_list()?;
    // Wire path looks like ["_edges", "~bond_N"]. Use the full path
    // string as a stable identifier.
    let parts: Vec<String> = list
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn has_compartment_kind(state: &Value, kind: &str) -> bool {
    fn walk(node: &Value, kind: &str) -> bool {
        let map = match node.as_map() {
            Some(m) => m,
            None => return false,
        };
        for (_, v) in map {
            let m = match v.as_map() {
                Some(m) => m,
                None => continue,
            };
            if m.get("_type").and_then(|t| t.as_str()) == Some("Compartment") {
                if let Some(k) = m.get("kind").and_then(|v| v.as_map()) {
                    if k.get("_type").and_then(|t| t.as_str()) == Some(kind) {
                        return true;
                    }
                }
            }
            if walk(v, kind) {
                return true;
            }
        }
        false
    }
    walk(state, kind)
}

fn position_in_compartment(comp: &str, entity: &str, control: &str) -> (f64, f64) {
    if control == "MEK" {
        return (0.50, 0.50);
    }
    match comp {
        // Cytoplasm gets a fourth slot so all four canonical ERKs
        // (erk0..erk3) have unique positions if they all end up here.
        "cytoplasm" => {
            let slots = [
                (0.50, 0.18), // 0 — bottom-center
                (0.50, 0.84), // 1 — top-center
                (0.10, 0.50), // 2 — left margin
                (0.88, 0.18), // 3 — bottom-right
            ];
            slots[match entity {
                "erk0" => 2,
                "erk1" => 0,
                "erk2" => 1,
                "erk3" => 3,
                _ => (djb2(entity) as usize) % slots.len(),
            }]
        }
        "nucleus" => {
            let slots = [
                (NUC_CX - 0.078, NUC_CY + 0.005),
                (NUC_CX - 0.020, NUC_CY + 0.075),
                (NUC_CX - 0.020, NUC_CY - 0.075),
            ];
            slots[match entity {
                "erk0" => 0,
                "erk1" => 1,
                "erk2" => 2,
                "erk3" => 0,
                _ => (djb2(entity) as usize) % slots.len(),
            }]
        }
        "er_lumen" => {
            let slots = [
                (ER_CX - 0.085, ER_CY + 0.06),
                (ER_CX + 0.085, ER_CY + 0.05),
                (ER_CX - 0.060, ER_CY - 0.07),
            ];
            slots[match entity {
                "erk0" => 0,
                "erk1" => 1,
                "erk2" => 2,
                "erk3" => 0,
                _ => (djb2(entity) as usize) % slots.len(),
            }]
        }
        _ => (0.5, 0.5),
    }
}

fn djb2(s: &str) -> u32 {
    let mut h: u32 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    h
}

// ── Anatomy drawing ───────────────────────────────────────────────

fn draw_cell(out: &mut String) {
    // Cytosol fill: rounded rect inside the bilayer.
    let x0 = px(CELL_X0);
    let y0 = py(CELL_Y1);
    let x1 = px(CELL_X1);
    let y1 = py(CELL_Y0);
    let rx = pr(0.045);
    let ry = pr(0.045);
    out.push_str(&format!(
        "    <rect x=\"{x0:.1}\" y=\"{y0:.1}\" width=\"{:.1}\" height=\"{:.1}\" \
         rx=\"{rx:.1}\" ry=\"{ry:.1}\" fill=\"#f4f7f0\" stroke=\"none\" />\n",
        x1 - x0,
        y1 - y0
    ));
    // Outer leaflet (slightly outset).
    let dx = pr(0.005);
    out.push_str(&format!(
        "    <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" \
         rx=\"{:.1}\" ry=\"{:.1}\" fill=\"none\" stroke=\"#3a5b3a\" stroke-width=\"1.4\" />\n",
        x0 - dx,
        y0 - dx,
        x1 - x0 + 2.0 * dx,
        y1 - y0 + 2.0 * dx,
        rx + dx,
        ry + dx,
    ));
    // Inner leaflet.
    out.push_str(&format!(
        "    <rect x=\"{x0:.1}\" y=\"{y0:.1}\" width=\"{:.1}\" height=\"{:.1}\" \
         rx=\"{rx:.1}\" ry=\"{ry:.1}\" fill=\"none\" stroke=\"#5a7a5a\" stroke-width=\"1.0\" />\n",
        x1 - x0,
        y1 - y0,
    ));
}

fn draw_nucleus(out: &mut String) {
    let cx = px(NUC_CX);
    let cy = py(NUC_CY);
    let r = pr(NUC_R);
    // Pale nucleoplasm.
    out.push_str(&format!(
        "    <circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{r:.1}\" \
         fill=\"#fff7e6\" stroke=\"none\" />\n"
    ));
    // Outer envelope.
    out.push_str(&format!(
        "    <circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{r:.1}\" \
         fill=\"none\" stroke=\"#7d5b3a\" stroke-width=\"1.5\" />\n"
    ));
    // Inner envelope (small inset).
    let r_inner = pr(NUC_R - 0.012);
    out.push_str(&format!(
        "    <circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{r_inner:.1}\" \
         fill=\"none\" stroke=\"#a8835a\" stroke-width=\"0.8\" />\n"
    ));
    // NPCs (six, evenly spaced around the envelope).
    for k in 0..6 {
        let a = 2.0 * PI * (k as f64) / 6.0 + PI / 8.0;
        let nx_mpl = NUC_CX + NUC_R * a.cos();
        let ny_mpl = NUC_CY + NUC_R * a.sin();
        draw_npc_glyph(out, nx_mpl, ny_mpl, 0.022);
    }
    // Nucleolus.
    out.push_str(&format!(
        "    <circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" \
         fill=\"#d4a673\" stroke=\"#7d5b3a\" stroke-width=\"0.6\" opacity=\"0.7\" />\n",
        px(NUC_CX + 0.025),
        py(NUC_CY - 0.018),
        pr(0.024)
    ));
    // Label below nucleus.
    out.push_str(&format!(
        "    <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
         font-size=\"10\" font-style=\"italic\" fill=\"#7d5b3a\">nucleus</text>\n",
        cx,
        py(NUC_CY - NUC_R - 0.020),
    ));
}

fn draw_er_lumen(out: &mut String) {
    let cx = px(ER_CX);
    // Two cisternae.
    for (dy, tilt) in [(0.07, 8.0_f64), (-0.07, -8.0_f64)] {
        let cy_mpl = ER_CY + dy;
        let cy = py(cy_mpl);
        let w = pr(2.0 * ER_HW * 0.95);
        let h = pry(0.075);
        // Outer membrane.
        out.push_str(&format!(
            "    <ellipse cx=\"{cx:.1}\" cy=\"{cy:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" \
             fill=\"#fde4cf\" stroke=\"#a96a3a\" stroke-width=\"1.2\" \
             transform=\"rotate({:.1},{cx:.1},{cy:.1})\" />\n",
            w / 2.0,
            h / 2.0,
            -tilt
        ));
        // Inner membrane (lumen contour).
        let wi = pr(2.0 * ER_HW * 0.78);
        let hi = pry(0.045);
        out.push_str(&format!(
            "    <ellipse cx=\"{cx:.1}\" cy=\"{cy:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" \
             fill=\"none\" stroke=\"#c98a5a\" stroke-width=\"0.6\" \
             transform=\"rotate({:.1},{cx:.1},{cy:.1})\" />\n",
            wi / 2.0,
            hi / 2.0,
            -tilt
        ));
    }
    // Tubular connector between cisternae.
    let tube_x = px(ER_CX - 0.018);
    let tube_y = py(ER_CY + 0.05);
    let tube_w = pr(0.036);
    let tube_h = pry(0.10);
    let tube_r = pr(0.018);
    out.push_str(&format!(
        "    <rect x=\"{tube_x:.1}\" y=\"{tube_y:.1}\" width=\"{tube_w:.1}\" \
         height=\"{tube_h:.1}\" rx=\"{tube_r:.1}\" ry=\"{tube_r:.1}\" \
         fill=\"#fde4cf\" stroke=\"#a96a3a\" stroke-width=\"1.0\" />\n"
    ));
    // Ribosome dots on cisternae.
    for (dy, _tilt) in [(0.07, 8.0_f64), (-0.07, -8.0_f64)] {
        let cy_mpl = ER_CY + dy;
        for k in 0..4 {
            let t = -1.0 + (k as f64) * 0.66;
            let rx_mpl = ER_CX + (ER_HW * 0.85) * t;
            let ry_mpl = cy_mpl + 0.030 * (t * PI).cos();
            out.push_str(&format!(
                "    <circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" \
                 fill=\"#5a3a20\" stroke=\"none\" />\n",
                px(rx_mpl),
                py(ry_mpl),
                pr(0.005)
            ));
        }
    }
    // Label below ER.
    out.push_str(&format!(
        "    <text x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
         font-size=\"10\" font-style=\"italic\" fill=\"#a96a3a\">ER lumen</text>\n",
        py(ER_CY - ER_HH + 0.005),
    ));
}

fn draw_npc_glyph(out: &mut String, cx_mpl: f64, cy_mpl: f64, radius: f64) {
    let cx = px(cx_mpl);
    let cy = py(cy_mpl);
    let r = pr(radius);
    // Outer disc.
    out.push_str(&format!(
        "    <circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{r:.1}\" \
         fill=\"#cfcfcf\" stroke=\"#555\" stroke-width=\"0.8\" />\n"
    ));
    // Inner disc.
    let ri = r * 0.42;
    out.push_str(&format!(
        "    <circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{ri:.1}\" \
         fill=\"#fdfdf9\" stroke=\"#555\" stroke-width=\"0.6\" />\n"
    ));
    // Eight-fold spokes.
    for k in 0..8 {
        let a = (k as f64) * PI / 4.0;
        let x1 = cx + ri * a.cos();
        let y1 = cy - ri * a.sin(); // y flips
        let x2 = cx + r * a.cos();
        let y2 = cy - r * a.sin();
        out.push_str(&format!(
            "    <line x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\" \
             stroke=\"#888\" stroke-width=\"0.4\" />\n"
        ));
    }
}

// ── Molecule cartoons ─────────────────────────────────────────────

fn draw_kinase_bilobal(
    out: &mut String,
    cx_mpl: f64,
    cy_mpl: f64,
    label: &str,
    face_n: &str,
    face_c: &str,
    edge: &str,
    scale: f64,
    p_in_cleft: bool,
    label_color: &str,
    label_weight: &str,
) {
    let s = scale;
    // C-lobe (lower, larger).
    let cx_c = px(cx_mpl - 0.002 * s);
    let cy_c = py(cy_mpl - 0.018 * s);
    let rx_c = pr(0.085 * s) / 2.0;
    let ry_c = pry(0.058 * s) / 2.0;
    out.push_str(&format!(
        "    <ellipse cx=\"{cx_c:.1}\" cy=\"{cy_c:.1}\" rx=\"{rx_c:.1}\" ry=\"{ry_c:.1}\" \
         fill=\"{face_c}\" stroke=\"{edge}\" stroke-width=\"1.0\" \
         transform=\"rotate(6,{cx_c:.1},{cy_c:.1})\" />\n"
    ));
    // N-lobe (upper, smaller).
    let cx_n = px(cx_mpl - 0.005 * s);
    let cy_n = py(cy_mpl + 0.024 * s);
    let rx_n = pr(0.058 * s) / 2.0;
    let ry_n = pry(0.043 * s) / 2.0;
    out.push_str(&format!(
        "    <ellipse cx=\"{cx_n:.1}\" cy=\"{cy_n:.1}\" rx=\"{rx_n:.1}\" ry=\"{ry_n:.1}\" \
         fill=\"{face_n}\" stroke=\"{edge}\" stroke-width=\"1.0\" \
         transform=\"rotate(-12,{cx_n:.1},{cy_n:.1})\" />\n"
    ));
    // Hinge highlight (left of cleft).
    let hx1 = px(cx_mpl - 0.030 * s);
    let hy1 = py(cy_mpl + 0.005 * s);
    let hx2 = px(cx_mpl - 0.020 * s);
    let hy2 = py(cy_mpl);
    out.push_str(&format!(
        "    <line x1=\"{hx1:.1}\" y1=\"{hy1:.1}\" x2=\"{hx2:.1}\" y2=\"{hy2:.1}\" \
         stroke=\"{edge}\" stroke-width=\"0.6\" opacity=\"0.5\" />\n"
    ));
    // Phosphate in cleft.
    if p_in_cleft {
        let px_p = px(cx_mpl + 0.038 * s);
        let py_p = py(cy_mpl + 0.005 * s);
        let r_p = pr(0.012 * s);
        out.push_str(&format!(
            "    <circle cx=\"{px_p:.1}\" cy=\"{py_p:.1}\" r=\"{r_p:.1}\" \
             fill=\"#fde047\" stroke=\"#7c4a00\" stroke-width=\"0.8\" />\n"
        ));
        out.push_str(&format!(
            "    <text x=\"{px_p:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
             font-size=\"7\" font-weight=\"bold\" fill=\"#5a3500\">P</text>\n",
            py_p + 2.5
        ));
    }
    // Label below.
    if !label.is_empty() {
        let lx = px(cx_mpl);
        let ly = py(cy_mpl - 0.072 * s) + 3.0;
        out.push_str(&format!(
            "    <text x=\"{lx:.1}\" y=\"{ly:.1}\" text-anchor=\"middle\" \
             font-size=\"10\" font-weight=\"{label_weight}\" fill=\"{label_color}\">{}</text>\n",
            escape_xml(label)
        ));
    }
}

fn draw_mek(out: &mut String, cx: f64, cy: f64, label: &str, bound: bool, scale: f64) {
    draw_kinase_bilobal(
        out, cx, cy, label,
        MEK_FACE_N, MEK_FACE_C, MEK_EDGE,
        scale, bound,
        MEK_EDGE, "bold",
    );
}

fn draw_erk(out: &mut String, cx_mpl: f64, cy_mpl: f64, label: &str, phosphorylated: bool, scale: f64) {
    let s = 0.7 * scale;
    let (face_n, face_c, edge) = if phosphorylated {
        (PERK_FACE_N, PERK_FACE_C, PERK_EDGE)
    } else {
        (ERK_FACE_N, ERK_FACE_C, ERK_EDGE)
    };
    // C-lobe.
    let cx_c = px(cx_mpl + 0.003 * s);
    let cy_c = py(cy_mpl - 0.014 * s);
    let rx_c = pr(0.075 * s) / 2.0;
    let ry_c = pry(0.052 * s) / 2.0;
    out.push_str(&format!(
        "    <ellipse cx=\"{cx_c:.1}\" cy=\"{cy_c:.1}\" rx=\"{rx_c:.1}\" ry=\"{ry_c:.1}\" \
         fill=\"{face_c}\" stroke=\"{edge}\" stroke-width=\"1.0\" \
         transform=\"rotate(-8,{cx_c:.1},{cy_c:.1})\" />\n"
    ));
    // N-lobe.
    let cx_n = px(cx_mpl + 0.006 * s);
    let cy_n = py(cy_mpl + 0.021 * s);
    let rx_n = pr(0.052 * s) / 2.0;
    let ry_n = pry(0.038 * s) / 2.0;
    out.push_str(&format!(
        "    <ellipse cx=\"{cx_n:.1}\" cy=\"{cy_n:.1}\" rx=\"{rx_n:.1}\" ry=\"{ry_n:.1}\" \
         fill=\"{face_n}\" stroke=\"{edge}\" stroke-width=\"1.0\" \
         transform=\"rotate(12,{cx_n:.1},{cy_n:.1})\" />\n"
    ));
    // Hinge highlight.
    let hx1 = px(cx_mpl + 0.028 * s);
    let hy1 = py(cy_mpl + 0.004 * s);
    let hx2 = px(cx_mpl + 0.018 * s);
    let hy2 = py(cy_mpl);
    out.push_str(&format!(
        "    <line x1=\"{hx1:.1}\" y1=\"{hy1:.1}\" x2=\"{hx2:.1}\" y2=\"{hy2:.1}\" \
         stroke=\"{edge}\" stroke-width=\"0.6\" opacity=\"0.5\" />\n"
    ));
    // Dual phosphorylation: two P discs on activation loop.
    if phosphorylated {
        for (dx, dy) in [(0.038_f64, 0.005_f64), (0.044, -0.012)] {
            let px_p = px(cx_mpl + dx * s);
            let py_p = py(cy_mpl + dy * s);
            let r_p = pr(0.010 * s);
            out.push_str(&format!(
                "    <circle cx=\"{px_p:.1}\" cy=\"{py_p:.1}\" r=\"{r_p:.1}\" \
                 fill=\"#fde047\" stroke=\"#7c4a00\" stroke-width=\"0.7\" />\n"
            ));
            out.push_str(&format!(
                "    <text x=\"{px_p:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                 font-size=\"6\" font-weight=\"bold\" fill=\"#5a3500\">P</text>\n",
                py_p + 2.0
            ));
        }
    }
    if !label.is_empty() {
        let lx = px(cx_mpl);
        let ly = py(cy_mpl - 0.060 * s) + 3.0;
        out.push_str(&format!(
            "    <text x=\"{lx:.1}\" y=\"{ly:.1}\" text-anchor=\"middle\" \
             font-size=\"9\" fill=\"#222\">{}</text>\n",
            escape_xml(label)
        ));
    }
}

fn draw_bond(out: &mut String, x1_mpl: f64, y1_mpl: f64, x2_mpl: f64, y2_mpl: f64, opacity: f64) {
    let x1 = px(x1_mpl);
    let y1 = py(y1_mpl);
    let x2 = px(x2_mpl);
    let y2 = py(y2_mpl);
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        return;
    }
    let off = 3.0; // perpendicular offset for double-line bond
    let ox = -dy / len * off;
    let oy = dx / len * off;
    for sgn in [1.0_f64, -1.0] {
        out.push_str(&format!(
            "    <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" \
             stroke=\"#2ca02c\" stroke-width=\"1.5\" stroke-linecap=\"round\" \
             opacity=\"{opacity:.2}\" />\n",
            x1 + sgn * ox,
            y1 + sgn * oy,
            x2 + sgn * ox,
            y2 + sgn * oy,
        ));
    }
}

// ── Animation helpers ─────────────────────────────────────────────

/// Emit one entity (e.g. erk1) as a <g> whose transform animates
/// between its per-snapshot positions, and whose phosphorylation
/// state cross-fades between ERK and pERK variants.
fn emit_animated_entity(
    out: &mut String,
    label: &str,
    layouts: &[Vec<LayoutEntry>],
    key_times_str: &str,
    total_dur: f64,
) {
    // Per-snapshot (x, y, control, wire) for this entity. If absent
    // from a snapshot, fall back to its last known position.
    let mut last_x = 0.5;
    let mut last_y = 0.5;
    let mut last_ctrl = "ERK".to_string();
    let mut sequence: Vec<(f64, f64, String, bool)> = Vec::new();
    let mut ever_appears = false;
    for layout_data in layouts {
        let found = layout_data.iter().find(|e| e.label == label);
        if let Some(e) = found {
            last_x = e.x;
            last_y = e.y;
            last_ctrl = e.control.clone();
            ever_appears = true;
        }
        let bound = found.and_then(|e| e.wire.as_ref()).is_some();
        sequence.push((last_x, last_y, last_ctrl.clone(), bound));
    }
    if !ever_appears {
        return;
    }
    let initial = &sequence[0];
    // Translate values: convert each mpl center to SVG pixels, then
    // subtract py(0) on y so the inner glyph (drawn via `draw_*` at
    // mpl-origin `(0,0)`, whose internal `py()` already adds 665px
    // of flip offset) lands exactly at the intended pixel position
    // instead of being shoved one canvas-height below the viewBox.
    let y_anchor = py(0.0);
    let values: Vec<String> = sequence
        .iter()
        .map(|(x, y, _, _)| format!("{:.1},{:.1}", px(*x), py(*y) - y_anchor))
        .collect();
    let values_str = values.join(";");

    // Two inner groups, one for ERK / MEK and one for pERK. Each
    // glyph drawn at origin (0,0). Use opacity animation between
    // them to cross-fade when control changes.
    out.push_str(&format!(
        "    <g><animateTransform attributeName=\"transform\" type=\"translate\" \
         values=\"{values_str}\" keyTimes=\"{key_times_str}\" \
         calcMode=\"spline\" \
         keySplines=\"{}\" \
         dur=\"{total_dur}s\" repeatCount=\"indefinite\" />\n",
        repeat_splines(sequence.len()),
    ));

    let ctrl0 = initial.2.as_str();
    let bound0 = initial.3;

    // MEK glyph (only ever non-zero when this entity is MEK).
    let mek_op0 = if ctrl0 == "MEK" { 1.0 } else { 0.0 };
    let mek_vals: Vec<String> = sequence
        .iter()
        .map(|(_, _, c, _)| if c == "MEK" { "1" } else { "0" }.to_string())
        .collect();
    let mek_bound_vals: Vec<String> = sequence
        .iter()
        .map(|(_, _, c, b)| if c == "MEK" && *b { "1" } else { "0" }.to_string())
        .collect();
    out.push_str(&format!("      <g opacity=\"{mek_op0}\">\n"));
    out.push_str(&format!(
        "        <animate attributeName=\"opacity\" values=\"{}\" \
         keyTimes=\"{key_times_str}\" dur=\"{total_dur}s\" repeatCount=\"indefinite\" />\n",
        mek_vals.join(";")
    ));
    // Two MEK variants (unbound and bound) so the cleft phosphate
    // can fade in. We swap opacity per snapshot.
    let mut mek_unbound = String::new();
    draw_kinase_bilobal(&mut mek_unbound, 0.0, 0.0, label,
        MEK_FACE_N, MEK_FACE_C, MEK_EDGE, 1.0, false, MEK_EDGE, "bold");
    let mut mek_bound = String::new();
    draw_kinase_bilobal(&mut mek_bound, 0.0, 0.0, label,
        MEK_FACE_N, MEK_FACE_C, MEK_EDGE, 1.0, true, MEK_EDGE, "bold");
    let unbound_vals: Vec<String> = sequence
        .iter()
        .map(|(_, _, c, b)| if c == "MEK" && !*b { "1" } else { "0" }.to_string())
        .collect();
    out.push_str(&format!(
        "        <g opacity=\"{}\">\n          <animate attributeName=\"opacity\" \
         values=\"{}\" keyTimes=\"{key_times_str}\" dur=\"{total_dur}s\" \
         repeatCount=\"indefinite\" />\n{}        </g>\n",
        if ctrl0 == "MEK" && !bound0 { 1.0 } else { 0.0 },
        unbound_vals.join(";"),
        mek_unbound
    ));
    out.push_str(&format!(
        "        <g opacity=\"{}\">\n          <animate attributeName=\"opacity\" \
         values=\"{}\" keyTimes=\"{key_times_str}\" dur=\"{total_dur}s\" \
         repeatCount=\"indefinite\" />\n{}        </g>\n",
        if ctrl0 == "MEK" && bound0 { 1.0 } else { 0.0 },
        mek_bound_vals.join(";"),
        mek_bound
    ));
    out.push_str("      </g>\n");

    // ERK glyph.
    let mut erk_g = String::new();
    draw_erk(&mut erk_g, 0.0, 0.0, label, false, 1.0);
    let erk_vals: Vec<String> = sequence
        .iter()
        .map(|(_, _, c, _)| if c == "ERK" { "1" } else { "0" }.to_string())
        .collect();
    out.push_str(&format!(
        "      <g opacity=\"{}\">\n        <animate attributeName=\"opacity\" \
         values=\"{}\" keyTimes=\"{key_times_str}\" dur=\"{total_dur}s\" \
         repeatCount=\"indefinite\" />\n{}      </g>\n",
        if ctrl0 == "ERK" { 1.0 } else { 0.0 },
        erk_vals.join(";"),
        erk_g
    ));

    // pERK glyph.
    let mut perk_g = String::new();
    draw_erk(&mut perk_g, 0.0, 0.0, label, true, 1.0);
    let perk_vals: Vec<String> = sequence
        .iter()
        .map(|(_, _, c, _)| if c == "pERK" { "1" } else { "0" }.to_string())
        .collect();
    out.push_str(&format!(
        "      <g opacity=\"{}\">\n        <animate attributeName=\"opacity\" \
         values=\"{}\" keyTimes=\"{key_times_str}\" dur=\"{total_dur}s\" \
         repeatCount=\"indefinite\" />\n{}      </g>\n",
        if ctrl0 == "pERK" { 1.0 } else { 0.0 },
        perk_vals.join(";"),
        perk_g
    ));
    out.push_str("    </g>\n");
}

/// For each unique wire across the timeline, emit a bond line whose
/// endpoints animate between the two paired entities' positions and
/// whose opacity is 1 only on snapshots where the wire is present.
fn emit_animated_bonds(
    out: &mut String,
    _all_labels: &[String],
    layouts: &[Vec<LayoutEntry>],
    key_times_str: &str,
    total_dur: f64,
) {
    // Collect, per snapshot, the bond pairs: { wire → [(x,y), (x,y)] }.
    let n = layouts.len();
    let mut wire_seen: std::collections::BTreeMap<String, Vec<Option<((f64, f64), (f64, f64))>>> =
        std::collections::BTreeMap::new();
    for (i, layout_data) in layouts.iter().enumerate() {
        let mut by_wire: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
        for e in layout_data {
            if let Some(w) = &e.wire {
                by_wire.entry(w.clone()).or_default().push((e.x, e.y));
            }
        }
        for (w, pts) in by_wire {
            if pts.len() >= 2 {
                let entry = wire_seen.entry(w).or_insert_with(|| vec![None; n]);
                entry[i] = Some((pts[0], pts[1]));
            }
        }
    }
    for (_wire, slots) in wire_seen {
        // Build x1/y1/x2/y2 sequences. For frames where the wire
        // isn't present, hold the last known endpoints.
        let mut last: Option<((f64, f64), (f64, f64))> = None;
        let mut x1s = Vec::new();
        let mut y1s = Vec::new();
        let mut x2s = Vec::new();
        let mut y2s = Vec::new();
        let mut ops = Vec::new();
        for slot in &slots {
            if let Some(pair) = slot {
                last = Some(*pair);
            }
            let pair = last.unwrap_or(((0.5, 0.5), (0.5, 0.5)));
            x1s.push(format!("{:.1}", px(pair.0 .0)));
            y1s.push(format!("{:.1}", py(pair.0 .1)));
            x2s.push(format!("{:.1}", px(pair.1 .0)));
            y2s.push(format!("{:.1}", py(pair.1 .1)));
            ops.push(if slot.is_some() { "1" } else { "0" }.to_string());
        }
        // We can't animate offset perp easily, so we approximate the
        // double-line glyph by two parallel lines fixed at a small
        // pixel offset perpendicular to the *initial* endpoint
        // direction. Recomputed at first appearance.
        let first_pair = slots
            .iter()
            .find_map(|s| s.as_ref())
            .copied()
            .unwrap_or(((0.5, 0.5), (0.5, 0.5)));
        let dx0 = px(first_pair.1 .0) - px(first_pair.0 .0);
        let dy0 = py(first_pair.1 .1) - py(first_pair.0 .1);
        let len0 = (dx0 * dx0 + dy0 * dy0).sqrt().max(1.0);
        let off = 3.0_f64;
        let ox = -dy0 / len0 * off;
        let oy = dx0 / len0 * off;

        for sgn in [1.0_f64, -1.0] {
            let x1_vals: Vec<String> = x1s
                .iter()
                .map(|s| format!("{:.1}", s.parse::<f64>().unwrap_or(0.0) + sgn * ox))
                .collect();
            let y1_vals: Vec<String> = y1s
                .iter()
                .map(|s| format!("{:.1}", s.parse::<f64>().unwrap_or(0.0) + sgn * oy))
                .collect();
            let x2_vals: Vec<String> = x2s
                .iter()
                .map(|s| format!("{:.1}", s.parse::<f64>().unwrap_or(0.0) + sgn * ox))
                .collect();
            let y2_vals: Vec<String> = y2s
                .iter()
                .map(|s| format!("{:.1}", s.parse::<f64>().unwrap_or(0.0) + sgn * oy))
                .collect();
            out.push_str(&format!(
                "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" \
                 stroke=\"#2ca02c\" stroke-width=\"1.5\" stroke-linecap=\"round\" opacity=\"{}\">\n\
                  <animate attributeName=\"x1\" values=\"{}\" keyTimes=\"{key_times_str}\" \
                   dur=\"{total_dur}s\" repeatCount=\"indefinite\" />\n\
                  <animate attributeName=\"y1\" values=\"{}\" keyTimes=\"{key_times_str}\" \
                   dur=\"{total_dur}s\" repeatCount=\"indefinite\" />\n\
                  <animate attributeName=\"x2\" values=\"{}\" keyTimes=\"{key_times_str}\" \
                   dur=\"{total_dur}s\" repeatCount=\"indefinite\" />\n\
                  <animate attributeName=\"y2\" values=\"{}\" keyTimes=\"{key_times_str}\" \
                   dur=\"{total_dur}s\" repeatCount=\"indefinite\" />\n\
                  <animate attributeName=\"opacity\" values=\"{}\" keyTimes=\"{key_times_str}\" \
                   dur=\"{total_dur}s\" repeatCount=\"indefinite\" calcMode=\"discrete\" />\n\
                </line>\n",
                x1_vals[0],
                y1_vals[0],
                x2_vals[0],
                y2_vals[0],
                ops[0],
                x1_vals.join(";"),
                y1_vals.join(";"),
                x2_vals.join(";"),
                y2_vals.join(";"),
                ops.join(";"),
            ));
        }
    }
}

/// Spline string for the keyTimes list: `0.42 0 0.58 1` for each
/// segment, joined by `;`. Gives a cubic ease-in-out per segment.
fn repeat_splines(n_snapshots: usize) -> String {
    if n_snapshots < 2 {
        return String::new();
    }
    let one = "0.42 0 0.58 1";
    std::iter::repeat(one)
        .take(n_snapshots - 1)
        .collect::<Vec<_>>()
        .join(";")
}

// ── Tiny helpers ──────────────────────────────────────────────────

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val_str(s: &str) -> Value {
        Value::String(s.to_string())
    }

    fn mini_state() -> Value {
        Value::tree([
            ("_type", val_str("Cell")),
            (
                "cytoplasm",
                Value::tree([
                    ("_type", val_str("Compartment")),
                    ("kind", Value::tree([("_type", val_str("Cytoplasm"))])),
                    ("mek", Value::tree([("_type", val_str("MEK"))])),
                    (
                        "erk1",
                        Value::tree([
                            ("_type", val_str("ERK")),
                            ("name", val_str("erk1")),
                        ]),
                    ),
                    (
                        "nucleus",
                        Value::tree([
                            ("_type", val_str("Compartment")),
                            ("kind", Value::tree([("_type", val_str("Nucleus"))])),
                        ]),
                    ),
                ]),
            ),
        ])
    }

    #[test]
    fn snapshot_contains_anatomy_and_molecules() {
        let s = mini_state();
        let svg = render_cell_snapshot_svg(&s, "t=0");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("cell"));
        assert!(svg.contains("nucleus"));
        assert!(svg.contains("mek") || svg.contains("MEK") || svg.contains(MEK_FACE_C));
        assert!(svg.contains("erk1"));
        assert!(svg.contains(ERK_FACE_C));
    }

    #[test]
    fn animation_uses_smil() {
        let s = mini_state();
        let svg = render_cell_animation_svg(&[s.clone(), s], &[0.0, 1.0], 1.0);
        assert!(svg.contains("<animateTransform"));
        assert!(svg.contains("repeatCount=\"indefinite\""));
    }

    #[test]
    fn bond_renders_double_line_for_paired_wires() {
        let s = Value::tree([
            ("_type", val_str("Cell")),
            (
                "cytoplasm",
                Value::tree([
                    ("_type", val_str("Compartment")),
                    ("kind", Value::tree([("_type", val_str("Cytoplasm"))])),
                    (
                        "mek",
                        Value::tree([
                            ("_type", val_str("MEK")),
                            (
                                "outputs",
                                Value::tree([(
                                    "enzyme_port",
                                    Value::List(vec![val_str("_edges"), val_str("~bond_0")]),
                                )]),
                            ),
                        ]),
                    ),
                    (
                        "erk0",
                        Value::tree([
                            ("_type", val_str("pERK")),
                            ("name", val_str("erk0")),
                            (
                                "outputs",
                                Value::tree([(
                                    "substrate_port",
                                    Value::List(vec![val_str("_edges"), val_str("~bond_0")]),
                                )]),
                            ),
                        ]),
                    ),
                ]),
            ),
        ]);
        let svg = render_cell_snapshot_svg(&s, "");
        // Two perpendicular lines drawn at #2ca02c
        let lines = svg.matches("stroke=\"#2ca02c\"").count();
        assert!(lines >= 2, "expected double-line bond, got {} stroke instances", lines);
    }
}

//! Run the MAPK BRS and emit an HTML report.
//!
//! Steps in lockstep over a Gillespie τ-leap, capturing per-snapshot
//! counts of ERK / pERK by compartment plus the full fired-event log.
//! Writes an SVG of the trajectories and an HTML report wrapping them.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use plotters::prelude::*;
use prism_bigraph::{BigraphicalReactiveSystem, BrsMode, Process, Update};
use prism_mapk::{initial_mapk_state, mapk_rules};
use prism_schema::reaction::ReactionRule;
use prism_schema::{Key, StateMap, Value};
use prism_viz::{
    render_pattern_dot, render_pattern_svg, render_to_file, rule_color,
};

/// Per-snapshot population counts. Order matches the legend.
#[derive(Clone, Debug, Default)]
struct Counts {
    cyto_erk: usize,
    cyto_perk: usize,
    nuc_erk: usize,
    nuc_perk: usize,
    er_erk: usize,
    er_perk: usize,
    complexes: usize,
}

#[derive(Clone, Debug)]
struct Snapshot {
    t: f64,
    counts: Counts,
}

struct Args {
    duration: f64,
    interval: f64,
    seed: u64,
    out_dir: PathBuf,
    name: String,
    max_per_tick: usize,
}

impl Args {
    fn parse() -> Self {
        let mut a = Self {
            duration: 80.0,
            interval: 1.0,
            seed: 42,
            out_dir: PathBuf::from("out"),
            name: "brs_mapk".to_string(),
            max_per_tick: 1_000_000,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--time" => a.duration = args.next().and_then(|s| s.parse().ok()).unwrap_or(a.duration),
                "--interval" => a.interval = args.next().and_then(|s| s.parse().ok()).unwrap_or(a.interval),
                "--seed" => a.seed = args.next().and_then(|s| s.parse().ok()).unwrap_or(a.seed),
                "--out" => a.out_dir = args.next().map(PathBuf::from).unwrap_or(a.out_dir),
                "--name" => a.name = args.next().unwrap_or(a.name),
                "--max-per-tick" => a.max_per_tick = args.next().and_then(|s| s.parse().ok()).unwrap_or(a.max_per_tick),
                "-h" | "--help" => {
                    println!(
                        "Usage: mapk [--time 80] [--interval 1.0] [--seed 42] \
                                    [--out out] [--name brs_mapk] [--max-per-tick N]"
                    );
                    std::process::exit(0);
                }
                _ => {}
            }
        }
        a
    }
}

fn main() {
    let args = Args::parse();
    fs::create_dir_all(&args.out_dir).expect("create out dir");

    println!(
        "⏱  MAPK BRS: duration={} interval={} seed={} mode=gillespie",
        args.duration, args.interval, args.seed
    );

    let rules = mapk_rules();
    let brs = BigraphicalReactiveSystem::with_config(
        rules,
        BrsMode::Gillespie,
        None,
        args.seed,
        Some(args.max_per_tick),
        args.interval,
    );

    let mut state = initial_mapk_state();
    let start = Instant::now();
    let mut snapshots = Vec::new();
    snapshots.push(Snapshot {
        t: 0.0,
        counts: count_populations(&state),
    });

    let n_steps = (args.duration / args.interval).ceil() as usize;
    for i in 0..n_steps {
        let t = (i + 1) as f64 * args.interval;
        let input = wrap_state(state.clone());
        let update = brs.update(&input, args.interval);
        if let Update::Value(delta) = update {
            if let Some(state_delta) = delta.get_field("state") {
                state = apply_delta(&state, state_delta);
            }
        }
        snapshots.push(Snapshot {
            t,
            counts: count_populations(&state),
        });
    }
    let elapsed = start.elapsed();
    let fired = brs.fired_log();
    println!(
        "✅ {} firings in {:.2}s ({} snapshots)",
        fired.len(),
        elapsed.as_secs_f64(),
        snapshots.len()
    );

    // Write SVG + HTML report.
    let svg_path = args.out_dir.join(format!("{}_trajectories.svg", args.name));
    plot_trajectories(&snapshots, &svg_path).expect("svg");

    // Render each rule's redex + reactum two ways: as a Graphviz
    // bigraph-viz tree (the "side view") and as a top-down nested-
    // oval Milner-style diagram (the "top view"). Both go into the
    // report side by side.
    let use_graphviz = which_dot();
    let rule_images = render_rules(&mapk_rules(), &args.out_dir, &args.name, use_graphviz);

    let report_path = args.out_dir.join(format!("{}_report.html", args.name));
    write_report(
        &report_path,
        &args,
        &snapshots,
        &fired,
        elapsed.as_secs_f64(),
        &rule_images,
    )
    .expect("report");

    println!("📂 {}", svg_path.display());
    println!(
        "🎨 rules: side-view via {}{} + top-view via in-process SVG",
        if use_graphviz { "graphviz" } else { "in-process SVG (fallback)" },
        if use_graphviz { "" } else { "" },
    );
    println!("📰 {}", report_path.display());
}

/// File names for one rule's four images: dot redex/reactum (side view)
/// and svg redex/reactum (top view).
#[derive(Clone, Debug)]
struct RuleImages {
    label: String,
    dot_redex: String,
    dot_reactum: String,
    svg_redex: String,
    svg_reactum: String,
    /// True if the side-view PNGs are actually Graphviz output; false
    /// means we fell back to in-process SVG for the side view too.
    side_is_graphviz: bool,
}

/// Check whether `dot` is on PATH so we can decide DOT vs in-process SVG.
fn which_dot() -> bool {
    std::process::Command::new("dot")
        .arg("-V")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Render each rule's redex / reactum two ways and persist all four
/// files per rule (plus the `.dot` source for the side view).
fn render_rules(
    rules: &[ReactionRule],
    out_dir: &Path,
    name_prefix: &str,
    use_graphviz: bool,
) -> Vec<RuleImages> {
    let mut out = Vec::new();
    for rule in rules {
        let dot_redex_name = format!("{}_rule_{}_dot_redex.svg", name_prefix, rule.label);
        let dot_reactum_name = format!("{}_rule_{}_dot_reactum.svg", name_prefix, rule.label);
        let svg_redex_name = format!("{}_rule_{}_top_redex.svg", name_prefix, rule.label);
        let svg_reactum_name = format!("{}_rule_{}_top_reactum.svg", name_prefix, rule.label);

        // Top-down nested-oval view (always renderable).
        let _ = fs::write(
            out_dir.join(&svg_redex_name),
            render_pattern_svg(&rule.redex, ""),
        );
        let _ = fs::write(
            out_dir.join(&svg_reactum_name),
            render_pattern_svg(&rule.reactum, ""),
        );

        // Side-view tree via Graphviz, falling back to top-view SVG
        // if `dot` isn't available or fails at runtime.
        let mut side_is_graphviz = false;
        if use_graphviz {
            let dot_redex = render_pattern_dot(&rule.redex, "");
            let dot_reactum = render_pattern_dot(&rule.reactum, "");
            let _ = fs::write(
                out_dir.join(format!("{}_rule_{}_dot_redex.dot", name_prefix, rule.label)),
                &dot_redex,
            );
            let _ = fs::write(
                out_dir.join(format!("{}_rule_{}_dot_reactum.dot", name_prefix, rule.label)),
                &dot_reactum,
            );
            let r1 = render_to_file(&dot_redex, &out_dir.join(&dot_redex_name), "svg");
            let r2 = render_to_file(&dot_reactum, &out_dir.join(&dot_reactum_name), "svg");
            side_is_graphviz = r1.is_ok() && r2.is_ok();
        }
        if !side_is_graphviz {
            let _ = fs::write(
                out_dir.join(&dot_redex_name),
                render_pattern_svg(&rule.redex, ""),
            );
            let _ = fs::write(
                out_dir.join(&dot_reactum_name),
                render_pattern_svg(&rule.reactum, ""),
            );
        }

        out.push(RuleImages {
            label: rule.label.clone(),
            dot_redex: dot_redex_name,
            dot_reactum: dot_reactum_name,
            svg_redex: svg_redex_name,
            svg_reactum: svg_reactum_name,
            side_is_graphviz,
        });
    }
    out
}

// ── Helpers ─────────────────────────────────────────────────────────

fn wrap_state(subtree: Value) -> Value {
    let mut m = StateMap::new();
    m.insert(Key::from("state"), subtree);
    Value::Map(m)
}

/// Walk a Value-delta and apply it to `initial`, recognizing
/// `_add`/`_remove` sentinels at any nesting level. Same logic as the
/// engine apply pipeline.
fn apply_delta(initial: &Value, delta: &Value) -> Value {
    fn walk(target: &mut Value, delta: &Value) {
        let delta_map = match delta.as_map() {
            Some(m) => m,
            None => {
                *target = delta.clone();
                return;
            }
        };
        let target_map = match target.as_map_mut() {
            Some(m) => m,
            None => {
                *target = delta.clone();
                return;
            }
        };
        if let Some(Value::List(rm)) = delta_map.get("_remove") {
            for k in rm {
                if let Some(s) = k.as_str() {
                    target_map.shift_remove(s);
                }
            }
        }
        if let Some(Value::Map(adds)) = delta_map.get("_add") {
            for (k, v) in adds {
                target_map.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in delta_map {
            if k == "_add" || k == "_remove" {
                continue;
            }
            match target_map.get_mut(k) {
                Some(child) => walk(child, v),
                None => {
                    target_map.insert(k.clone(), v.clone());
                }
            }
        }
    }
    let mut next = initial.clone();
    walk(&mut next, delta);
    next
}

/// Count ERK / pERK molecules by compartment and total complexes
/// (pairs of MEK with a bound substrate).
fn count_populations(state: &Value) -> Counts {
    let mut c = Counts::default();
    let cyto = match state.get_field("cytoplasm") {
        Some(v) => v,
        None => return c,
    };

    count_in_compartment(cyto, &mut c.cyto_erk, &mut c.cyto_perk, &mut c.complexes);

    if let Some(nuc) = cyto.get_field("nucleus") {
        count_in_compartment(nuc, &mut c.nuc_erk, &mut c.nuc_perk, &mut c.complexes);
    }
    if let Some(er) = cyto.get_field("er_lumen") {
        count_in_compartment(er, &mut c.er_erk, &mut c.er_perk, &mut c.complexes);
    }
    c
}

fn count_in_compartment(
    compartment: &Value,
    erk: &mut usize,
    perk: &mut usize,
    complexes: &mut usize,
) {
    let iter = match compartment.iter_fields() {
        Some(i) => i,
        None => return,
    };
    for (key, val) in iter {
        if key.starts_with('_') || key == "kind" {
            continue;
        }
        // Skip nested child Compartments (we tally them separately).
        if val.get_field("_type").and_then(|t| t.as_str()) == Some("Compartment") {
            continue;
        }
        match val.get_field("_type").and_then(|t| t.as_str()) {
            Some("ERK") => *erk += 1,
            Some("pERK") => {
                *perk += 1;
                if val.get_field("outputs").is_some() {
                    // pERK with outputs is in a complex; count once per pair.
                    *complexes += 1;
                }
            }
            _ => {}
        }
    }
}

// ── Plot ────────────────────────────────────────────────────────────

fn plot_trajectories(
    snapshots: &[Snapshot],
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = SVGBackend::new(path, (900, 500)).into_drawing_area();
    root.fill(&WHITE)?;

    let t_min = snapshots.first().map(|s| s.t).unwrap_or(0.0);
    let t_max = snapshots.last().map(|s| s.t).unwrap_or(1.0);
    let max_y = snapshots
        .iter()
        .flat_map(|s| {
            [
                s.counts.cyto_erk,
                s.counts.cyto_perk,
                s.counts.nuc_erk,
                s.counts.nuc_perk,
                s.counts.er_erk,
                s.counts.er_perk,
            ]
        })
        .max()
        .unwrap_or(1)
        .max(1) as f64;

    let mut chart = ChartBuilder::on(&root)
        .caption("MAPK populations over time", ("sans-serif", 24))
        .margin(20)
        .x_label_area_size(40)
        .y_label_area_size(50)
        .build_cartesian_2d(t_min..t_max, 0.0..(max_y + 0.5))?;

    chart
        .configure_mesh()
        .x_desc("time")
        .y_desc("count")
        .draw()?;

    // Six series: (selector, label, color)
    let series: Vec<(fn(&Counts) -> usize, &str, RGBColor)> = vec![
        (|c| c.cyto_erk, "cyto ERK", RGBColor(0x1f, 0x77, 0xb4)),
        (|c| c.cyto_perk, "cyto pERK", RGBColor(0xff, 0x7f, 0x0e)),
        (|c| c.nuc_erk, "nuc ERK", RGBColor(0x2c, 0xa0, 0x2c)),
        (|c| c.nuc_perk, "nuc pERK", RGBColor(0xd6, 0x27, 0x28)),
        (|c| c.er_erk, "ER ERK", RGBColor(0x94, 0x67, 0xbd)),
        (|c| c.er_perk, "ER pERK", RGBColor(0x8c, 0x56, 0x4b)),
    ];

    for (sel, label, color) in series {
        let style = color.stroke_width(2);
        chart
            .draw_series(LineSeries::new(
                snapshots.iter().map(|s| (s.t, sel(&s.counts) as f64)),
                style,
            ))?
            .label(label)
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], color.stroke_width(2)));
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    root.present()?;
    Ok(())
}

// ── HTML report ─────────────────────────────────────────────────────

fn write_report(
    path: &Path,
    args: &Args,
    snapshots: &[Snapshot],
    fired: &[prism_bigraph::FiredEvent],
    elapsed_s: f64,
    rule_images: &[RuleImages],
) -> std::io::Result<()> {
    let svg = format!("{}_trajectories.svg", args.name);

    // Tally firings per rule label.
    let mut rule_counts: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    for ev in fired {
        *rule_counts.entry(ev.rule_label.as_str()).or_insert(0) += 1;
    }
    let rules_html: String = rule_counts
        .iter()
        .map(|(rule, n)| {
            let color = rule_color(rule);
            format!(
                "<tr><td><span class=\"swatch\" style=\"background:{color}\"></span>{rule}</td>\
                 <td style=\"text-align:right\">{n}</td></tr>"
            )
        })
        .collect();

    let first = &snapshots.first().map(|s| s.counts.clone()).unwrap_or_default();
    let last = &snapshots.last().map(|s| s.counts.clone()).unwrap_or_default();

    let row = |label: &str, f: fn(&Counts) -> usize| {
        format!(
            "<tr><td>{label}</td><td>{}</td><td>{}</td></tr>",
            f(first),
            f(last)
        )
    };
    let count_rows = format!(
        "{}{}{}{}{}{}{}",
        row("cyto ERK", |c| c.cyto_erk),
        row("cyto pERK", |c| c.cyto_perk),
        row("nuc ERK", |c| c.nuc_erk),
        row("nuc pERK", |c| c.nuc_perk),
        row("ER ERK", |c| c.er_erk),
        row("ER pERK", |c| c.er_perk),
        row("complexes", |c| c.complexes),
    );

    // Per-rule cards: each shows both the side-view (Graphviz tree)
    // and the top-view (Milner nested-ovals SVG), side by side, with
    // a colored title bar and biology blurb.
    let mut rule_cards = String::new();
    for img in rule_images {
        let label = &img.label;
        let color = rule_color(label);
        let n_fired = rule_counts.get(label.as_str()).copied().unwrap_or(0);
        let blurb = rule_blurb(label);
        rule_cards.push_str(&format!(
            r##"
<div class="rule-card">
  <h4 style="color:{color}">
    <span class="swatch" style="background:{color}"></span>
    {label}
    <span class="tag">fired ×{n_fired}</span>
  </h4>
  <p class="rule-blurb">{blurb}</p>

  <div class="view-label">side view (bigraph tree)</div>
  <div class="rule-pair">
    <div><img src="{dot_redex}" alt="{label} redex (side view)"></div>
    <div class="arrow" aria-hidden="true">&rarr;</div>
    <div><img src="{dot_reactum}" alt="{label} reactum (side view)"></div>
  </div>

  <div class="view-label">top view (Milner nested compartments)</div>
  <div class="rule-pair">
    <div><img src="{svg_redex}" alt="{label} redex (top view)"></div>
    <div class="arrow" aria-hidden="true">&rarr;</div>
    <div><img src="{svg_reactum}" alt="{label} reactum (top view)"></div>
  </div>
</div>"##,
            dot_redex = img.dot_redex,
            dot_reactum = img.dot_reactum,
            svg_redex = img.svg_redex,
            svg_reactum = img.svg_reactum,
        ));
    }

    let html = format!(
        r##"<!doctype html>
<html lang="en"><head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>MAPK BRS report — {name}</title>
<style>
  :root {{
    --bg: #fcfcfc; --fg: #222; --muted: #555;
    --rule-line: #d0d0d0;
    --card-bg: #fff; --card-border: #e6e6e6;
    --tag-bg: #f4f4f4;
  }}
  html {{ scroll-behavior: smooth; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI",
                 system-ui, sans-serif;
    background: var(--bg); color: var(--fg);
    max-width: 1100px; margin: 0 auto;
    padding: 28px 24px 80px; line-height: 1.5;
  }}
  h1 {{ margin: 0 0 18px 0; }}
  h2 {{
    margin-top: 2em; padding-bottom: 4px;
    border-bottom: 1px solid var(--rule-line);
  }}
  h4 {{ margin: 0 0 6px 0; font-size: 15px; }}
  p.meta {{ color: var(--muted); font-size: 14px; }}
  p.rule-blurb {{ color: var(--muted); margin: 6px 0 10px 0; font-size: 13px; }}
  table {{ border-collapse: collapse; margin: 1em 0; }}
  th, td {{ padding: 4px 12px; border-bottom: 1px solid #eee; text-align: left; }}
  th {{ background: var(--tag-bg); font-weight: 600; }}
  .tag {{
    display: inline-block; margin-left: 8px;
    padding: 2px 8px; background: var(--tag-bg);
    border-radius: 999px; font-size: 11px; color: #333;
    vertical-align: middle; font-weight: 400;
  }}
  .swatch {{
    display: inline-block; width: 10px; height: 10px;
    border-radius: 2px; margin-right: 6px; vertical-align: middle;
  }}
  figure {{
    margin: 12px 0; padding: 12px;
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 10px;
  }}
  figure img {{ width: 100%; height: auto; display: block; }}
  .grid-2 {{ display: grid; grid-template-columns: 1fr 1fr; gap: 2em; }}
  .rule-grid {{
    display: grid;
    grid-template-columns: 1fr;
    gap: 28px;
    margin-top: 20px;
  }}
  .rule-card {{
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 12px;
    padding: 22px 26px 26px;
  }}
  .rule-card h4 {{
    font-size: 17px;
  }}
  .rule-card p.rule-blurb {{
    margin-bottom: 14px;
  }}
  .rule-pair {{
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center; gap: 8px;
  }}
  .rule-pair img {{ width: 100%; height: auto; }}
  .rule-pair .arrow {{
    font-size: 22px; color: var(--muted); text-align: center;
  }}
  .view-label {{
    margin: 14px 0 6px 0; color: var(--muted);
    font-size: 11px; font-weight: 600; text-transform: uppercase;
    letter-spacing: 0.04em;
  }}
  .view-label:first-of-type {{ margin-top: 8px; }}
</style>
</head><body>

<h1>MAPK as a bigraphical reactive system</h1>
<p class="meta">
  Gillespie SSA over {duration} time units (Δ = {interval}, seed = {seed}).
  {fired_n} firings, {snap_n} snapshots, {elapsed:.2}s wall-clock.
</p>

<h2>Population trajectories</h2>
<figure>
  <img src="{svg}" alt="trajectories" />
</figure>

<div class="grid-2">
  <div>
    <h2>Populations (initial → final)</h2>
    <table>
      <thead><tr><th>species/location</th><th>t=0</th><th>t={final_t:.1}</th></tr></thead>
      <tbody>{count_rows}</tbody>
    </table>
  </div>
  <div>
    <h2>Firings by rule</h2>
    <table>
      <thead><tr><th>rule</th><th>count</th></tr></thead>
      <tbody>{rules_html}</tbody>
    </table>
  </div>
</div>

<h2>Rule catalog</h2>
<p>Each rule's redex (left) and reactum (right) — parametric bigraph
rewrites. <em>Dashed gray</em> ellipses are open <code>Site</code>
holes (capture any subtree); <em>filled circles</em> are
<code>LinkVar</code> endpoints (same color ⇒ same edge);
<em>dashed red</em> boxes are <code>Absent</code> negative-application
conditions (the key must be missing or empty).</p>
<div class="rule-grid">{rule_cards}
</div>

<h2>Notes</h2>
<ul>
  <li>Asymmetric <code>translocate_perk_in</code> (k=2.0) vs <code>translocate_perk_out</code> (k=0.1) drives the steady-state nuclear pERK accumulation — the biological "signal" of the cycle.</li>
  <li>The cycle keeps firing past steady state because nuclear pERK is dephosphorylated back to ERK and re-exported.</li>
  <li>This report was generated by <code>prism-mapk</code>, a Rust port of <code>kaleidoscope.systems.mapk</code>.</li>
</ul>

</body></html>
"##,
        name = args.name,
        duration = args.duration,
        interval = args.interval,
        seed = args.seed,
        fired_n = fired.len(),
        snap_n = snapshots.len(),
        elapsed = elapsed_s,
        final_t = snapshots.last().map(|s| s.t).unwrap_or(0.0),
        svg = svg,
        count_rows = count_rows,
        rules_html = rules_html,
        rule_cards = rule_cards,
    );
    fs::write(path, html)
}

/// One-line biology explanation for each rule. Used as the card blurb.
fn rule_blurb(label: &str) -> &'static str {
    match label {
        "phosphorylate" => {
            "Free MEK + free ERK in the same compartment form the MEK·pERK complex \
             (a shared link-graph edge); ERK control flips to pERK."
        }
        "dissociate" => {
            "The MEK·pERK complex dissociates inside its compartment; the bond \
             is destroyed and both endpoints go free."
        }
        "dephosphorylate" => {
            "A nuclear MAP-kinase phosphatase removes the phosphate from a free \
             nuclear pERK, returning it to ERK."
        }
        "translocate_erk_in" => {
            "A free unphosphorylated ERK descends from cytoplasm into a child \
             compartment (nucleus or ER lumen)."
        }
        "translocate_erk_out" => {
            "A free unphosphorylated ERK ascends from a child compartment back \
             into cytoplasm."
        }
        "translocate_perk_in" => {
            "Active nuclear import of free phospho-ERK — biased forward to drive \
             nuclear accumulation."
        }
        "translocate_perk_out" => {
            "Slow nuclear export / leak of free phospho-ERK — the reverse path \
             of the import bias."
        }
        _ => "",
    }
}

//! The MAPK report as a **step-network workflow composite**.
//!
//! The existing `src/bin/run.rs` produces the report as one imperative `main`.
//! Here the *same* report is a **composite whose subprocesses are steps**, wired
//! into a DAG and executed by the prism engine. Nothing here schedules or
//! orders the steps: the engine derives the dependency graph from the wiring
//! (a step that reads a path another step writes depends on it — see
//! `Engine::from_state` → topological firing) and runs them in order. The
//! "simulation, then independent analyses" shape falls straight out:
//!
//! ```text
//!   RunBrs ──> snapshots ──┬─> PlotTrajectories ─> trajectory ─┐
//!                          ├─> RenderAnimation  ─> animation  ─┼─> WriteReport ─> report.html
//!   (no inputs) RenderRules ──────────────────── rule_panels ─┘
//! ```
//!
//! `RenderRules` has no inputs, and `PlotTrajectories` / `RenderAnimation` read
//! only `snapshots` (not each other) — so those three are an independent
//! frontier (parallelizable; today the engine runs the topological order
//! sequentially). `WriteReport` joins them.
//!
//! The step bodies reuse the public renderers (`prism_viz`, the BRS); the sim
//! helpers mirror `src/bin/run.rs` for now (factoring both onto one shared
//! palette of reusable operations is task #10).

use std::any::Any;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use indexmap::IndexMap;

use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::{
    BigraphicalReactiveSystem, BrsMode, Engine, Process, ProcessNode, Step, Update,
};
use prism_schema::{Key, Schema, StateMap, Value};
use prism_viz::{render_cell_animation_svg, render_pattern_svg};

use crate::{initial_mapk_state, mapk_rules};

/// How to run the report workflow.
#[derive(Clone, Debug)]
pub struct ReportConfig {
    pub duration: f64,
    pub interval: f64,
    pub seed: u64,
    pub out_dir: PathBuf,
    pub name: String,
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            duration: 80.0,
            interval: 1.0,
            seed: 42,
            out_dir: PathBuf::from("out"),
            name: "brs_mapk_workflow".to_string(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// Simulation helpers (mirror src/bin/run.rs; task #10 unifies them)
// ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
struct Counts {
    cyto_erk: usize,
    cyto_perk: usize,
    nuc_erk: usize,
    nuc_perk: usize,
    er_erk: usize,
    er_perk: usize,
}

fn count_populations(state: &Value) -> Counts {
    let mut c = Counts::default();
    let cyto = match state.get_field("cytoplasm") {
        Some(v) => v,
        None => return c,
    };
    count_in_compartment(cyto, &mut c.cyto_erk, &mut c.cyto_perk);
    if let Some(nuc) = cyto.get_field("nucleus") {
        count_in_compartment(nuc, &mut c.nuc_erk, &mut c.nuc_perk);
    }
    if let Some(er) = cyto.get_field("er_lumen") {
        count_in_compartment(er, &mut c.er_erk, &mut c.er_perk);
    }
    c
}

fn count_in_compartment(compartment: &Value, erk: &mut usize, perk: &mut usize) {
    let iter = match compartment.iter_fields() {
        Some(i) => i,
        None => return,
    };
    for (key, val) in iter {
        if key.starts_with('_') || key == "kind" {
            continue;
        }
        if val.get_field("_type").and_then(|t| t.as_str()) == Some("Compartment") {
            continue;
        }
        match val.get_field("_type").and_then(|t| t.as_str()) {
            Some("ERK") => *erk += 1,
            Some("pERK") => *perk += 1,
            _ => {}
        }
    }
}

fn wrap_state(subtree: Value) -> Value {
    let mut m = StateMap::new();
    m.insert(Key::from("state"), subtree);
    Value::Map(m)
}

/// Apply a Value-delta, recognizing `_add`/`_remove` sentinels at any level
/// (same as the engine apply pipeline; mirrors `src/bin/run.rs`).
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

/// Encode one snapshot as a flat `Value` so it can flow through a port:
/// `{ t, cyto_erk, …, er_perk, state }`.
fn encode_snapshot(t: f64, state: &Value) -> Value {
    let c = count_populations(state);
    Value::tree([
        ("t", Value::float(t)),
        ("cyto_erk", Value::Int(c.cyto_erk as i64)),
        ("cyto_perk", Value::Int(c.cyto_perk as i64)),
        ("nuc_erk", Value::Int(c.nuc_erk as i64)),
        ("nuc_perk", Value::Int(c.nuc_perk as i64)),
        ("er_erk", Value::Int(c.er_erk as i64)),
        ("er_perk", Value::Int(c.er_perk as i64)),
        ("state", state.clone()),
    ])
}

// ─────────────────────────────────────────────────────────────────────
// Steps
// ─────────────────────────────────────────────────────────────────────

/// Run the MAPK BRS over a τ-leap and emit `snapshots` (a list of encoded
/// snapshots). The root of the DAG — no inputs.
#[derive(Debug)]
struct RunBrsStep {
    duration: f64,
    interval: f64,
    seed: u64,
}

impl Step for RunBrsStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::new()
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("snapshots".into(), Schema::overwrite(Schema::Any))])
    }
    fn update(&self, _state: &Value) -> Update {
        let brs = BigraphicalReactiveSystem::with_config(
            mapk_rules(),
            BrsMode::Gillespie,
            None,
            self.seed,
            Some(1_000_000),
            self.interval,
        );
        let mut sim_state = initial_mapk_state();
        let mut snapshots: Vec<Value> = vec![encode_snapshot(0.0, &sim_state)];
        let n_steps = (self.duration / self.interval).ceil() as usize;
        for i in 0..n_steps {
            let t = (i + 1) as f64 * self.interval;
            let input = wrap_state(sim_state.clone());
            if let Update::Value(delta) = brs.update(&input, self.interval) {
                if let Some(state_delta) = delta.get_field("state") {
                    sim_state = apply_delta(&sim_state, state_delta);
                }
            }
            snapshots.push(encode_snapshot(t, &sim_state));
        }
        Update::value(Value::tree([("snapshots", Value::List(snapshots))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Plot the population trajectories from `snapshots` → an SVG; output the file
/// name on `trajectory`.
#[derive(Debug)]
struct PlotTrajectoriesStep {
    out_dir: PathBuf,
    name: String,
}

impl Step for PlotTrajectoriesStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("snapshots".into(), Schema::Any)])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("trajectory".into(), Schema::overwrite(Schema::Any))])
    }
    fn update(&self, state: &Value) -> Update {
        let snaps = match state.get_field("snapshots").and_then(|v| v.as_list()) {
            Some(s) if !s.is_empty() => s.to_vec(),
            _ => return Update::Noop,
        };
        let fname = format!("{}_trajectories.svg", self.name);
        let path = self.out_dir.join(&fname);
        if plot_trajectories(&snaps, &path).is_err() {
            return Update::Noop;
        }
        Update::value(Value::tree([("trajectory", Value::String(fname))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render each rule's redex / reactum to SVG (Milner-style nested ovals);
/// output `rule_panels` = a list of `{label, redex, reactum}` file names. No
/// inputs — the rules are intrinsic, so this is an independent DAG branch.
#[derive(Debug)]
struct RenderRulesStep {
    out_dir: PathBuf,
    name: String,
}

impl Step for RenderRulesStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::new()
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("rule_panels".into(), Schema::overwrite(Schema::Any))])
    }
    fn update(&self, _state: &Value) -> Update {
        let mut panels: Vec<Value> = Vec::new();
        for rule in &mapk_rules() {
            let redex = format!("{}_rule_{}_redex.svg", self.name, rule.label);
            let reactum = format!("{}_rule_{}_reactum.svg", self.name, rule.label);
            let _ = fs::write(self.out_dir.join(&redex), render_pattern_svg(&rule.redex, ""));
            let _ = fs::write(
                self.out_dir.join(&reactum),
                render_pattern_svg(&rule.reactum, ""),
            );
            panels.push(Value::tree([
                ("label", Value::String(rule.label.clone())),
                ("redex", Value::String(redex)),
                ("reactum", Value::String(reactum)),
            ]));
        }
        Update::value(Value::tree([("rule_panels", Value::List(panels))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a SMIL-animated walk through the snapshot cell states → an SVG;
/// output the file name on `animation`.
#[derive(Debug)]
struct RenderAnimationStep {
    out_dir: PathBuf,
    name: String,
}

impl Step for RenderAnimationStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("snapshots".into(), Schema::Any)])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("animation".into(), Schema::overwrite(Schema::Any))])
    }
    fn update(&self, state: &Value) -> Update {
        let snaps = match state.get_field("snapshots").and_then(|v| v.as_list()) {
            Some(s) if !s.is_empty() => s.to_vec(),
            _ => return Update::Noop,
        };
        let states: Vec<Value> = snaps
            .iter()
            .filter_map(|s| s.get_field("state").cloned())
            .collect();
        let times: Vec<f64> = snaps
            .iter()
            .filter_map(|s| s.get_field("t").and_then(|v| v.as_f64()))
            .collect();
        let svg = render_cell_animation_svg(&states, &times, 1.0);
        let fname = format!("{}_animation.svg", self.name);
        let _ = fs::write(self.out_dir.join(&fname), svg);
        Update::value(Value::tree([("animation", Value::String(fname))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Collate the panels into one HTML report; output the file name on `report`.
/// The join of the DAG — depends on `trajectory`, `rule_panels`, `animation`.
#[derive(Debug)]
struct WriteReportStep {
    out_dir: PathBuf,
    name: String,
}

impl Step for WriteReportStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("trajectory".into(), Schema::Any),
            ("rule_panels".into(), Schema::Any),
            ("animation".into(), Schema::Any),
        ])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("report".into(), Schema::overwrite(Schema::Any))])
    }
    fn update(&self, state: &Value) -> Update {
        let trajectory = state.get_field("trajectory").and_then(|v| v.as_str());
        let animation = state.get_field("animation").and_then(|v| v.as_str());
        let panels = state.get_field("rule_panels").and_then(|v| v.as_list());
        // Wait for the upstream branches to have produced their artifacts.
        if trajectory.is_none() || animation.is_none() || panels.is_none() {
            return Update::Noop;
        }
        let html = render_html(
            &self.name,
            trajectory.unwrap(),
            animation.unwrap(),
            panels.unwrap(),
        );
        let fname = format!("{}_report.html", self.name);
        let _ = fs::write(self.out_dir.join(&fname), html);
        Update::value(Value::tree([("report", Value::String(fname))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ─────────────────────────────────────────────────────────────────────
// Rendering helpers
// ─────────────────────────────────────────────────────────────────────

fn plot_trajectories(snaps: &[Value], path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let keys = ["cyto_erk", "cyto_perk", "nuc_erk", "nuc_perk", "er_erk", "er_perk"];
    let t = |s: &Value| s.get_field("t").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let count = |s: &Value, k: &str| s.get_field(k).and_then(|v| v.as_f64()).unwrap_or(0.0);

    let times: Vec<f64> = snaps.iter().map(t).collect();
    let mut series: IndexMap<String, Vec<f64>> = IndexMap::new();
    for key in keys {
        series.insert(key.to_string(), snaps.iter().map(|s| count(s, key)).collect());
    }
    let chart =
        prism_viz::plot::time_series_chart(&times, &series, "MAPK populations over time", false);
    std::fs::write(path, prism_viz::svg::to_svg(&chart))?;
    Ok(())
}

fn render_html(name: &str, trajectory: &str, animation: &str, panels: &[Value]) -> String {
    let mut rules_html = String::new();
    for p in panels {
        let label = p.get_field("label").and_then(|v| v.as_str()).unwrap_or("");
        let redex = p.get_field("redex").and_then(|v| v.as_str()).unwrap_or("");
        let reactum = p.get_field("reactum").and_then(|v| v.as_str()).unwrap_or("");
        rules_html.push_str(&format!(
            "<div class=\"rule\"><h3>{label}</h3>\
             <img src=\"{redex}\" alt=\"{label} redex\"> <span class=\"arrow\">⇒</span> \
             <img src=\"{reactum}\" alt=\"{label} reactum\"></div>\n"
        ));
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{name} — MAPK report (workflow)</title>\
         <style>body{{font-family:sans-serif;margin:2rem;max-width:1000px}}\
         .rule{{margin:1rem 0;display:flex;align-items:center;gap:1rem}}\
         .rule img{{max-height:160px;border:1px solid #ddd}} .arrow{{font-size:2rem;color:#888}}\
         img.full{{max-width:100%;border:1px solid #ddd}}</style></head><body>\
         <h1>{name} — MAPK report</h1>\
         <p><em>Produced by a step-network workflow composite (RunBrs → analyses → WriteReport).</em></p>\
         <h2>Populations over time</h2><img class=\"full\" src=\"{trajectory}\">\
         <h2>Reaction rules</h2>{rules_html}\
         <h2>Cell animation</h2><img class=\"full\" src=\"{animation}\">\
         </body></html>"
    )
}

// ─────────────────────────────────────────────────────────────────────
// Assembly + run
// ─────────────────────────────────────────────────────────────────────

/// A registry of the report's step factories (`local:RunBrs`, …).
fn report_registry(cfg: &ReportConfig) -> ProcessRegistry {
    let mut reg = ProcessRegistry::new();
    let (out_dir, name) = (cfg.out_dir.clone(), cfg.name.clone());
    let (duration, interval, seed) = (cfg.duration, cfg.interval, cfg.seed);

    reg.register("RunBrs", move |_config| {
        ProcessNode::Step(Box::new(RunBrsStep { duration, interval, seed }))
    });
    let (od, nm) = (out_dir.clone(), name.clone());
    reg.register("PlotTrajectories", move |_config| {
        ProcessNode::Step(Box::new(PlotTrajectoriesStep { out_dir: od.clone(), name: nm.clone() }))
    });
    let (od, nm) = (out_dir.clone(), name.clone());
    reg.register("RenderRules", move |_config| {
        ProcessNode::Step(Box::new(RenderRulesStep { out_dir: od.clone(), name: nm.clone() }))
    });
    let (od, nm) = (out_dir.clone(), name.clone());
    reg.register("RenderAnimation", move |_config| {
        ProcessNode::Step(Box::new(RenderAnimationStep { out_dir: od.clone(), name: nm.clone() }))
    });
    let (od, nm) = (out_dir.clone(), name.clone());
    reg.register("WriteReport", move |_config| {
        ProcessNode::Step(Box::new(WriteReportStep { out_dir: od.clone(), name: nm.clone() }))
    });
    reg
}

/// A process/step spec node: `{address, config, inputs, outputs}`.
fn spec(address: &str, inputs: &[(&str, &str)], outputs: &[(&str, &str)]) -> Value {
    let wire = |pairs: &[(&str, &str)]| {
        Value::Map(
            pairs
                .iter()
                .map(|(port, path)| {
                    (Key::from(*port), Value::List(vec![Value::String(path.to_string())]))
                })
                .collect::<IndexMap<Key, Value>>(),
        )
    };
    Value::tree([
        ("address", Value::String(format!("local:{address}"))),
        ("config", Value::Map(StateMap::new())),
        ("inputs", wire(inputs)),
        ("outputs", wire(outputs)),
    ])
}

/// A `StepLink` for the workflow schema: marks a state slot as a step with the
/// given input/output ports. This is what tells the engine the slot is a step
/// (and to fire it in dependency order) — a composite is *typed*, never `Any`.
fn step_link(inputs: &[&str], outputs: &[&str]) -> Schema {
    let ports = |names: &[&str]| -> IndexMap<Key, Schema> {
        names.iter().map(|p| (Key::from(*p), Schema::Any)).collect()
    };
    Schema::step_link(ports(inputs), ports(outputs))
}

/// The workflow composite's schema: each subprocess slot is a `StepLink`; the
/// data slots carry their value types. This *is* the composite's type — the
/// engine reads it to discover the steps and derive the DAG.
fn workflow_schema() -> Schema {
    let list = || Schema::List { element: Box::new(Schema::Any) };
    Schema::Tree {
        branches: IndexMap::from([
            (Key::from("RunBrs"), step_link(&[], &["snapshots"])),
            (Key::from("PlotTrajectories"), step_link(&["snapshots"], &["trajectory"])),
            (Key::from("RenderRules"), step_link(&[], &["rule_panels"])),
            (Key::from("RenderAnimation"), step_link(&["snapshots"], &["animation"])),
            (
                Key::from("WriteReport"),
                step_link(&["trajectory", "rule_panels", "animation"], &["report"]),
            ),
            (Key::from("snapshots"), list()),
            (Key::from("trajectory"), Schema::Any),
            (Key::from("rule_panels"), list()),
            (Key::from("animation"), Schema::Any),
            (Key::from("report"), Schema::Any),
        ]),
    }
}

/// The workflow composite's state: five step specs wired into the DAG, plus
/// the data slots they read/write. The wiring (shared paths) IS the DAG.
fn workflow_state() -> Value {
    Value::tree([
        ("RunBrs", spec("RunBrs", &[], &[("snapshots", "snapshots")])),
        (
            "PlotTrajectories",
            spec("PlotTrajectories", &[("snapshots", "snapshots")], &[("trajectory", "trajectory")]),
        ),
        ("RenderRules", spec("RenderRules", &[], &[("rule_panels", "rule_panels")])),
        (
            "RenderAnimation",
            spec("RenderAnimation", &[("snapshots", "snapshots")], &[("animation", "animation")]),
        ),
        (
            "WriteReport",
            spec(
                "WriteReport",
                &[("trajectory", "trajectory"), ("rule_panels", "rule_panels"), ("animation", "animation")],
                &[("report", "report")],
            ),
        ),
        // Data slots the steps wire through.
        ("snapshots", Value::List(vec![])),
        ("trajectory", Value::None),
        ("rule_panels", Value::List(vec![])),
        ("animation", Value::None),
        ("report", Value::None),
    ])
}

/// Build + run the report workflow. The engine fires the steps in dependency
/// order at construction; on return the report HTML and its assets are written
/// to `cfg.out_dir`. Returns the report's path.
pub fn run_report_workflow(cfg: &ReportConfig) -> Result<PathBuf, String> {
    fs::create_dir_all(&cfg.out_dir).map_err(|e| e.to_string())?;
    let registry = Arc::new(report_registry(cfg));
    let _engine = Engine::from_state(workflow_schema(), workflow_state(), registry)?;
    Ok(cfg.out_dir.join(format!("{}_report.html", cfg.name)))
}

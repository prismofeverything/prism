//! The **composite-process use case** (#6): a spatio-flux composite defined
//! once (`Dish`, a diffusion field), **imported** into a larger composite
//! (`Culture`, which nests two `Dish`es), then **simulated as one step in a
//! workflow** that renders a spatio-flux report section.
//!
//! Pre-parser, a `.ys` "file" is a `Program` (a set of defs) and "import" is
//! merging one program's defs into another (`import_dish` below) — the
//! mechanism the parser's `import Dish from "dish.ys"` will drive. Everything
//! else is real: chrysalis composites, the native `DiffusionAdvection`
//! process, the engine running the nested composite, and (next) a step DAG.

use std::any::Any;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{Engine, ProcessNode, ProcessRegistry, Step, Update};
use prism_schema::{Key, Schema, Value};

use chrysalis::ast::{
    CompositeDef, Def, Expr, ExternDef, Interface, Param, PortDecl, Program, SchemaExpr, StringLit,
};
use chrysalis::compile::compile_with_registry;
use spatio_flux::processes::diffusion_advection::DiffusionAdvection;

// ── helpers ─────────────────────────────────────────────────────────

fn field_list(vals: &[f64]) -> Expr {
    Expr::List(vals.iter().map(|&v| Expr::float(v)).collect())
}

fn fieldmap() -> SchemaExpr {
    SchemaExpr::map_of(SchemaExpr::array(vec![3, 3], SchemaExpr::Float))
}

/// A `{glucose, acetate}` field map literal (acetate flat zero).
fn gradient(glucose: &[f64]) -> Expr {
    Expr::Map(vec![
        (StringLit::plain("glucose"), field_list(glucose)),
        (StringLit::plain("acetate"), field_list(&[0.0; 9])),
    ])
}

// ── "dish.ys" — the reusable inner composite ────────────────────────

/// Merge the `Dish` composite (a diffusion field) + its `Diffusion` extern
/// into `p`. This is the **import**: pre-parser, bringing another file's defs
/// into scope = merging them (`import Dish from "dish.ys"` later).
fn import_dish(p: &mut Program) {
    p.push(Def::Extern(ExternDef {
        name: "Diffusion".into(),
        params: vec![],
        interface: Interface::new()
            .with_input("fields", PortDecl::required(fieldmap()))
            .with_output("fields", PortDecl::required(fieldmap())),
    }));
    // composite Dish[fields] ->{fields} ( fields: fields | Diffusion ~{fields}->{fields} )
    p.push(Def::Composite(CompositeDef {
        name: "Dish".into(),
        params: vec![Param::required("fields", fieldmap())],
        interface: Interface::new().with_output("fields", PortDecl::required(fieldmap())),
        using: vec![],
        body: Expr::parallel(vec![
            Expr::entry("fields", Expr::var("fields")),
            Expr::entry(
                "diffusion",
                Expr::term("Diffusion")
                    .input("fields", Expr::var("fields"))
                    .output("fields", Expr::var("fields"))
                    .build(),
            ),
        ]),
    }));
}

// ── "culture.ys" — the larger composite that imports + nests Dish ───

/// `import Dish from "dish.ys"` + `composite Culture ( well_a = Dish[…] |
/// well_b = Dish[…] )` + `main = Culture()`.
fn culture_program() -> Program {
    let mut p = Program::new();
    import_dish(&mut p); // ← the import

    // Two wells with different glucose gradients (vertical vs horizontal).
    let grad_a = || gradient(&[0.0, 0.0, 0.0, 5.0, 5.0, 5.0, 10.0, 10.0, 10.0]);
    let grad_b = || gradient(&[0.0, 5.0, 10.0, 0.0, 5.0, 10.0, 0.0, 5.0, 10.0]);

    // Each nested Dish is a subengine; we observe its BRIDGED output port on
    // the parent — so wire each well's `fields` output to a DISTINCT Culture
    // slot (`fields_a` / `fields_b`), else both would bridge to one `fields`.
    p.push(Def::Composite(CompositeDef {
        name: "Culture".into(),
        params: vec![],
        interface: Interface::new(),
        using: vec![],
        body: Expr::parallel(vec![
            // Seed the observable slots with the initial gradient; each nested
            // Dish's bridge then lands its per-step diffusion *diffs* here, so
            // the slot tracks the live (mass-conserving) field.
            Expr::entry("fields_a", grad_a()),
            Expr::entry("fields_b", grad_b()),
            Expr::entry(
                "well_a",
                Expr::term("Dish")
                    .arg_named("fields", grad_a())
                    .output("fields", Expr::var("fields_a"))
                    .build(),
            ),
            Expr::entry(
                "well_b",
                Expr::term("Dish")
                    .arg_named("fields", grad_b())
                    .output("fields", Expr::var("fields_b"))
                    .build(),
            ),
        ]),
    }));

    p.push(Def::Binding { name: "main".into(), value: Expr::term("Culture").build() });
    p
}

/// The native factory backing `extern process Diffusion`.
fn diffusion_natives() -> ProcessRegistry {
    let mut natives = ProcessRegistry::new();
    natives.register("Diffusion", |_cfg| {
        ProcessNode::Process(Box::new(DiffusionAdvection::new(
            (3, 3),
            (3.0, 3.0),
            IndexMap::from([("glucose".into(), 1e-1), ("acetate".into(), 1e-1)]),
            1.0,
        )))
    });
    natives
}

/// The address of a nested process/composite spec at `state.<path…>`.
fn address_at(state: &Value, path: &[&str]) -> Option<String> {
    let mut node = state;
    for seg in path {
        node = node.get_field(seg)?;
    }
    node.get_field("address").and_then(|v| v.as_str()).map(str::to_string)
}

/// A Culture slot's glucose field: `state.<slot>.glucose` — the nested Dish's
/// diffused field, surfaced through its bridge to the parent slot.
fn slot_glucose(state: &Value, slot: &str) -> Vec<f64> {
    state
        .get_field(slot)
        .and_then(|f| f.get_field("glucose"))
        .and_then(|v| v.as_list())
        .map(|l| l.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default()
}

/// The composite-process use case end-to-end: a `Dish` composite defined once,
/// **imported** into `Culture`, **nested** twice, **run**, and each nested
/// Dish's diffused field **observed through its bridge** — additive `Array`
/// fields now reconstruct across the nested bridge (the declared inner schema
/// travels in the spec; the engine promotes the output-port `Array` schema onto
/// the parent slot, so per-cell diffusion deltas apply element-wise).
#[test]
fn culture_imports_nests_and_runs_dish() {
    let program = culture_program();
    let result =
        compile_with_registry(&program, diffusion_natives()).expect("compile Culture (imports Dish)");

    // Import + nest: each well is a nested COMPOSITE spec carrying the imported
    // Dish's `Diffusion` sub-process.
    for well in ["well_a", "well_b"] {
        assert_eq!(
            address_at(&result.initial_state, &[well]).as_deref(),
            Some("local:Composite"),
            "{well} is a nested composite"
        );
        assert_eq!(
            address_at(&result.initial_state, &[well, "config", "state", "diffusion"]).as_deref(),
            Some("local:Diffusion"),
            "{well} nests the imported Dish's Diffusion process"
        );
    }

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(30.0);

    let state = engine.state();
    let before_a = vec![0.0, 0.0, 0.0, 5.0, 5.0, 5.0, 10.0, 10.0, 10.0];
    let before_b = vec![0.0, 5.0, 10.0, 0.0, 5.0, 10.0, 0.0, 5.0, 10.0];
    let after_a = slot_glucose(state, "fields_a");
    let after_b = slot_glucose(state, "fields_b");

    // Each nested Dish ran its diffusion, surfaced via its bridge to Culture's
    // fields_a / fields_b: the field changed (diffused) AND conserved its total
    // glucose (no-flux boundary) — i.e. the additive field reconstructs across
    // the nested bridge, not collapses to ~0.
    let sum = |v: &[f64]| v.iter().sum::<f64>();
    assert_eq!(after_a.len(), 9, "well_a bridged its 3×3 field to fields_a");
    assert_eq!(after_b.len(), 9, "well_b bridged its 3×3 field to fields_b");
    assert!(before_a != after_a, "well_a diffused");
    assert!(before_b != after_b, "well_b diffused");
    assert!((sum(&after_a) - 45.0).abs() < 1e-6, "well_a conserves glucose (got {})", sum(&after_a));
    assert!((sum(&after_b) - 45.0).abs() < 1e-6, "well_b conserves glucose (got {})", sum(&after_b));
}

// ─────────────────────────────────────────────────────────────────────
// #12: the spatio-flux report SECTION as a workflow
//
// A workflow composite whose `RunCulture` step SIMULATES the Culture
// composite (which imports + nests Dish) and snapshots each well's field
// over time, and a `RenderSection` step emits field heatmaps as an HTML
// section — the spatio-flux analogue of the MAPK report workflow, but the
// simulated thing is an imported+nested composite-process.
// ─────────────────────────────────────────────────────────────────────

/// Encode one snapshot of the Culture: `{ t, well_a:[9], well_b:[9] }`.
fn encode_culture_snapshot(t: f64, state: &Value) -> Value {
    let glucose = |slot: &str| Value::List(slot_glucose(state, slot).into_iter().map(Value::float).collect());
    Value::tree([
        ("t", Value::float(t)),
        ("well_a", glucose("fields_a")),
        ("well_b", glucose("fields_b")),
    ])
}

/// Simulate the Culture composite (imports + nests Dish) and emit `snapshots`
/// of each well's field over time. Runs the whole sub-simulation internally —
/// the workflow sees it as one step (the root of the section's DAG).
#[derive(Debug)]
struct RunCultureStep {
    steps: usize,
}

impl Step for RunCultureStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::new()
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("snapshots".into(), Schema::overwrite(Schema::Any))])
    }
    fn update(&self, _state: &Value) -> Update {
        let result = compile_with_registry(&culture_program(), diffusion_natives())
            .expect("compile Culture");
        let mut engine = Engine::from_state(
            result.topology.state_schema.clone(),
            result.initial_state.clone(),
            Arc::clone(&result.registry),
        )
        .expect("Culture engine");
        engine.discover_all_processes();
        let mut snapshots = vec![encode_culture_snapshot(0.0, engine.state())];
        for i in 0..self.steps {
            engine.run(1.0);
            snapshots.push(encode_culture_snapshot((i + 1) as f64, engine.state()));
        }
        Update::value(Value::tree([("snapshots", Value::List(snapshots))]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

/// Render the field snapshots into an HTML section of 3×3 heatmaps (rows =
/// wells, cols = a few timepoints); output the section file name on `section`.
#[derive(Debug)]
struct RenderSectionStep {
    out_dir: PathBuf,
    name: String,
}

impl Step for RenderSectionStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("snapshots".into(), Schema::Any)])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("section".into(), Schema::overwrite(Schema::Any))])
    }
    fn update(&self, state: &Value) -> Update {
        let snaps = match state.get_field("snapshots").and_then(|v| v.as_list()) {
            Some(s) if !s.is_empty() => s.to_vec(),
            _ => return Update::Noop,
        };
        // Pick ~4 evenly-spaced timepoints.
        let n = snaps.len();
        let picks: Vec<usize> = (0..4).map(|k| (k * (n - 1)) / 3).collect();
        let glucose = |snap: &Value, well: &str| -> Vec<f64> {
            snap.get_field(well).and_then(|v| v.as_list())
                .map(|l| l.iter().filter_map(|x| x.as_f64()).collect())
                .unwrap_or_default()
        };
        let mut rows = String::new();
        for well in ["well_a", "well_b"] {
            rows.push_str(&format!("<tr><th>{well}</th>"));
            for &i in &picks {
                let t = snaps[i].get_field("t").and_then(|v| v.as_f64()).unwrap_or(0.0);
                rows.push_str(&format!(
                    "<td><div class=\"t\">t={t:.0}</div>{}</td>",
                    heatmap_svg(&glucose(&snaps[i], well))
                ));
            }
            rows.push_str("</tr>");
        }
        let html = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
             <title>{name} — spatio-flux section</title>\
             <style>body{{font-family:sans-serif;margin:2rem}} td{{padding:.5rem;text-align:center}} \
             th{{text-align:right;padding-right:1rem}} .t{{font-size:.8rem;color:#666}}</style></head><body>\
             <h1>{name} — diffusion field section</h1>\
             <p><em>Produced by a workflow: RunCulture (imports+nests Dish) → RenderSection.</em></p>\
             <table>{rows}</table></body></html>",
            name = self.name
        );
        let fname = format!("{}_section.html", self.name);
        let _ = fs::write(self.out_dir.join(&fname), html);
        Update::value(Value::tree([("section", Value::String(fname))]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

/// A 3×3 glucose field as an inline SVG heatmap (white → green by value).
fn heatmap_svg(glucose: &[f64]) -> String {
    let vmax = glucose.iter().cloned().fold(1.0_f64, f64::max);
    let cell = 24;
    let mut svg = format!(
        "<svg width=\"{w}\" height=\"{w}\" xmlns=\"http://www.w3.org/2000/svg\">",
        w = cell * 3
    );
    for (i, &v) in glucose.iter().take(9).enumerate() {
        let (r, c) = (i / 3, i % 3);
        let t = (v / vmax).clamp(0.0, 1.0);
        let g = 255 - (t * 175.0) as u32; // white → green
        svg.push_str(&format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{cell}\" height=\"{cell}\" fill=\"rgb({rb},255,{rb})\" stroke=\"#ccc\"/>",
            x = c * cell, y = r * cell, rb = g
        ));
    }
    svg.push_str("</svg>");
    svg
}

/// A process/step spec node `{address, config, inputs, outputs}`.
fn step_spec(address: &str, inputs: &[(&str, &str)], outputs: &[(&str, &str)]) -> Value {
    let wire = |pairs: &[(&str, &str)]| {
        Value::Map(
            pairs.iter()
                .map(|(p, path)| (Key::from(*p), Value::List(vec![Value::String(path.to_string())])))
                .collect::<IndexMap<Key, Value>>(),
        )
    };
    Value::tree([
        ("address", Value::String(format!("local:{address}"))),
        ("config", Value::Map(IndexMap::new())),
        ("inputs", wire(inputs)),
        ("outputs", wire(outputs)),
    ])
}

fn section_step_link(inputs: &[&str], outputs: &[&str]) -> Schema {
    let ports = |ns: &[&str]| ns.iter().map(|p| (Key::from(*p), Schema::Any)).collect();
    Schema::step_link(ports(inputs), ports(outputs))
}

#[test]
fn spatio_flux_section_produced_by_a_workflow() {
    let dir = std::env::temp_dir().join(format!("prism_sflux_section_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let name = "culture";

    // Native step factories for the section workflow.
    let mut registry = ProcessRegistry::new();
    registry.register("RunCulture", |_cfg| ProcessNode::Step(Box::new(RunCultureStep { steps: 20 })));
    let (od, nm) = (dir.clone(), name.to_string());
    registry.register("RenderSection", move |_cfg| {
        ProcessNode::Step(Box::new(RenderSectionStep { out_dir: od.clone(), name: nm.clone() }))
    });
    let registry = Arc::new(registry);

    // The workflow composite: RunCulture → RenderSection (DAG via shared paths).
    let schema = Schema::Tree {
        branches: IndexMap::from([
            ("RunCulture".into(), section_step_link(&[], &["snapshots"])),
            ("RenderSection".into(), section_step_link(&["snapshots"], &["section"])),
            ("snapshots".into(), Schema::List { element: Box::new(Schema::Any) }),
            ("section".into(), Schema::Any),
        ]),
    };
    let state = Value::tree([
        ("RunCulture", step_spec("RunCulture", &[], &[("snapshots", "snapshots")])),
        ("RenderSection", step_spec("RenderSection", &[("snapshots", "snapshots")], &[("section", "section")])),
        ("snapshots", Value::List(vec![])),
        ("section", Value::None),
    ]);

    // Building the engine fires the step DAG in dependency order → the section.
    let _engine = Engine::from_state(schema, state, registry).expect("section workflow engine");

    let section = dir.join(format!("{name}_section.html"));
    assert!(section.exists(), "section HTML produced at {section:?}");
    let html = fs::read_to_string(&section).unwrap();
    assert!(html.contains("diffusion field section"), "section has the heading");
    assert!(html.contains("well_a") && html.contains("well_b"), "both wells rendered");
    assert!(html.matches("<svg").count() >= 8, "heatmaps for 2 wells × ~4 timepoints");
    let _ = fs::remove_dir_all(&dir);
}

//! Grow-divide-glucose over the **`stream:` protocol** — the boundary proof that
//! cell division crosses a real serialized wire (Arrow over OS pipes, a separate
//! OS process per cell), not just an in-process call.
//!
//! This is the EXACT analog of the proven rest test
//! (`crates/prism-bigraph/tests/cells_division.rs::grow_divide_glucose_is_identical_local_and_over_rest`):
//! the same environment, the same Form-3 division (cell proposes via its `divide`
//! face, the env's `Divider` enacts via the `_divide` sentinel,
//! `divide_by_schema(CompositeLink)` splits), the same mass-balance invariant
//! `glucose + Σmass + acetate = const` every tick — **only the address changes**,
//! `local:Composite` → `stream:cell.ys`. The cell runs as a separate
//! `chrysalis run cell.ys --serve-...` child; the parent's `Divider` divides the
//! stream-addressed cell NODES, and daughters inherit the stream protocol (so
//! division is cross-protocol for free).
//!
//! Until the stream CHILD forwards the inner reconciled update (like the local
//! `Composite::update` / the rest server already do), this FAILS: the child
//! synthesizes the wire frame by diffing absolute output snapshots, which
//! double-counts the shared glucose pool the moment ≥2 cells draw on it. Capturing
//! that here as a red test; the fix (a delta-forwarding stream child) turns it
//! green with NUMBERS identical to local/rest.

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{ProcessNode, Step};
use prism_bigraph::protocol::ProtocolRegistry;
use prism_bigraph::{Core, Engine, Key, Schema, Update, Value};

use chrysalis::stream::StreamProtocol;

fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String(s.to_string())).collect())
}

// ── The environment's Divider — IDENTICAL to cells_division.rs. The OUTSIDE half
//    of Form 3: it reads the cells map, finds a cell whose exposed face says
//    `divide`, and emits the `_divide` INTENT. The schema-holding `apply` ENACTS
//    the split via `divide_by_schema(CompositeLink)`; the Divider never authors
//    daughter contents or reaches into a cell. ──
#[derive(Debug)]
struct Divider;
impl Step for Divider {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("cells".to_string(), Schema::Map { value: Box::new(Schema::Any) })])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("cells".to_string(), Schema::Map { value: Box::new(Schema::Any) })])
    }
    fn update(&self, state: &Value) -> Update {
        let Some(cells) = state.get_field("cells").and_then(|v| v.as_map()) else {
            return Update::Noop;
        };
        for (key, cell) in cells {
            if key.starts_with('_') {
                continue;
            }
            let proposes = cell
                .get_field("divide")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if proposes {
                let d0 = format!("{key}_0");
                let d1 = format!("{key}_1");
                let reset = || Value::tree([("divide", Value::Bool(false))]);
                return Update::value(Value::tree([(
                    "cells",
                    Value::tree([(
                        "_divide",
                        Value::tree([
                            ("mother", Value::String(key.to_string())),
                            (
                                "daughters",
                                Value::tree([(d0.as_str(), reset()), (d1.as_str(), reset())]),
                            ),
                        ]),
                    )]),
                )]));
            }
        }
        Update::Noop
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The cell's `CompositeLink` schema as it sits in `cells: Map{Cell}` — the honest
/// type that makes `cells.N` discoverable + divisible schema-first. `outputs` is
/// the exported face: `mass` (extensive `Delta`, the divisible quantity),
/// `glucose`/`acetate` (pool deltas), `divide` (the proposal flag). The inner
/// schema is the child's concern (it runs `cell.ys`), so an empty tree here.
fn cell_link_schema() -> Schema {
    Schema::CompositeLink {
        inputs: IndexMap::from([
            (Key::from("mass"), Schema::Delta { default: None }),
            (Key::from("glucose"), Schema::float()),
        ]),
        outputs: IndexMap::from([
            (Key::from("mass"), Schema::Delta { default: None }),
            (Key::from("glucose"), Schema::float()),
            (Key::from("acetate"), Schema::float()),
            (Key::from("divide"), Schema::Bool { default: None }),
        ]),
        interval: 1.0,
        inner_schema: Box::new(Schema::Tree {
            branches: IndexMap::new(),
        }),
    }
}

/// A stream-addressed cell node. The only difference from the local/rest cell:
/// `address = stream:<cell.ys>` and `config = {}` (the StreamProtocol runs the
/// `.ys` file; per-instance dynamic state crosses via the bridge each tick — the
/// face `mass` seeds the child, the deltas come back). Outer wires are identical
/// to cells_division.rs: face on the own node (`%`), pools up at the env (`..`).
fn stream_cell_node(cell_ys: &str, mass: f64) -> Value {
    Value::tree([
        ("address", Value::String(format!("stream:{cell_ys}"))),
        ("config", Value::map()),
        ("interval", Value::float(1.0)),
        (
            "inputs",
            Value::tree([
                ("mass", wire(&["%", "mass"])),
                ("glucose", wire(&["..", "glucose"])),
            ]),
        ),
        (
            "outputs",
            Value::tree([
                ("mass", wire(&["%", "mass"])),
                ("glucose", wire(&["..", "glucose"])),
                ("acetate", wire(&["..", "acetate"])),
                ("divide", wire(&["%", "divide"])),
            ]),
        ),
        ("mass", Value::float(mass)),
        ("divide", Value::Bool(false)),
    ])
}

/// The environment's `Divider` step node.
fn divider_node() -> Value {
    Value::tree([
        ("address", Value::String("local:Divider".into())),
        ("config", Value::map()),
        ("inputs", Value::tree([("cells", wire(&["cells"]))])),
        ("outputs", Value::tree([("cells", wire(&["cells"]))])),
    ])
}

/// Top schema: env `glucose`/`acetate` pools + `cells: Map{CompositeLink}` + the
/// `Divider` step — honest, no `Schema::Any` at the cells.
fn top_schema() -> Schema {
    Schema::Tree {
        branches: IndexMap::from([
            (Key::from("glucose"), Schema::float()),
            (Key::from("acetate"), Schema::float()),
            (
                Key::from("cells"),
                Schema::Map {
                    value: Box::new(cell_link_schema()),
                },
            ),
            (
                Key::from("divider"),
                Schema::StepLink {
                    inputs: IndexMap::from([(
                        Key::from("cells"),
                        Schema::Map { value: Box::new(Schema::Any) },
                    )]),
                    outputs: IndexMap::from([(
                        Key::from("cells"),
                        Schema::Map { value: Box::new(Schema::Any) },
                    )]),
                    priority: 0.0,
                },
            ),
        ]),
    }
}

fn cell_mass_sum(state: &Value) -> f64 {
    state
        .get_field("cells")
        .and_then(|v| v.as_map())
        .map(|m| {
            m.iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .map(|(_, c)| c.get_field("mass").and_then(|v| v.as_f64()).unwrap_or(0.0))
                .sum()
        })
        .unwrap_or(0.0)
}

fn system_total(state: &Value) -> f64 {
    let glucose = state.get_field("glucose").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let acetate = state.get_field("acetate").and_then(|v| v.as_f64()).unwrap_or(0.0);
    glucose + acetate + cell_mass_sum(state)
}

fn cell_count(state: &Value) -> usize {
    state
        .get_field("cells")
        .and_then(|v| v.as_map())
        .map(|m| m.iter().filter(|(k, v)| !k.starts_with('_') && v.as_map().is_some()).count())
        .unwrap_or(0)
}

fn cells_keys(state: &Value) -> Vec<String> {
    state
        .get_field("cells")
        .and_then(|v| v.as_map())
        .map(|m| m.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default()
}

/// The parent core: the `Divider` factory (local) + the `stream` protocol pointed
/// at the cargo-built chrysalis binary (so a `stream:cell.ys` cell is spawned as a
/// real child process). No `Composite`/`Metabolism`/`Trigger` factories — those
/// live in `cell.ys`, run by the child.
fn parent_core() -> Core {
    let mut registry = ProcessRegistry::new();
    registry.register("Divider", |_config| ProcessNode::Step(Box::new(Divider)));
    let mut protocols = ProtocolRegistry::new(); // includes `local` (for the Divider)
    protocols.register(Arc::new(StreamProtocol {
        binary: Some(env!("CARGO_BIN_EXE_chrysalis").to_string()),
    }));
    Core::from(Arc::new(registry)).with_protocols(Arc::new(protocols))
}

fn cell_ys_path() -> String {
    format!("{}/ys/cell.ys", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn grow_divide_glucose_over_stream_conserves_mass() {
    // One stream-addressed cell on a finite glucose pool. The cell grows (its
    // metabolism runs in a separate `chrysalis run cell.ys --serve-...` process,
    // driven over Arrow pipes), proposes division at the threshold, and the env's
    // Divider enacts it. Mass is CONSERVED every tick — across growth AND division
    // AND the wire — exactly as the rest test asserts. The numbers must match
    // local/rest because the cell, the bridge, and the division are identical;
    // only the address changed.
    let cell_ys = cell_ys_path();
    let initial = Value::tree([
        ("glucose", Value::float(40.0)),
        ("acetate", Value::float(0.0)),
        ("cells", Value::tree([("c0", stream_cell_node(&cell_ys, 1.0))])),
        ("divider", divider_node()),
    ]);
    let total0 = system_total(&initial);
    assert!((total0 - 41.0).abs() < 1e-9, "initial total = glucose 40 + mass 1 = 41");

    let mut engine = Engine::from_state(top_schema(), initial, parent_core()).expect("engine");
    engine.set_max_nodes(64); // generous backstop — a real runaway fails via the asserts below
    engine.discover_all_processes();

    let mut counts: Vec<usize> = Vec::new();
    for tick in 0..30 {
        engine.run(1.0);
        let total = system_total(engine.state());
        assert!(
            (total - total0).abs() < 1e-6,
            "tick {tick}: mass balance violated over stream: total={total} vs {total0}\n  \
             g={:?} a={:?} mΣ={} cells={:?}",
            engine.state().get_field("glucose").and_then(|v| v.as_f64()),
            engine.state().get_field("acetate").and_then(|v| v.as_f64()),
            cell_mass_sum(engine.state()),
            cells_keys(engine.state()),
        );
        counts.push(cell_count(engine.state()));
    }

    // Division TERMINATES: the cell subdivides its fixed biomass (25 pg, once
    // glucose is exhausted) until every daughter falls below the threshold —
    // 25/2⁴ = 1.5625 < 2 — i.e. 16 cells, then it stops. A delta-forwarding stream
    // is what makes this terminate (a snapshot-diff would lose mass / never settle).
    assert_eq!(
        counts[25..],
        [16usize; 5],
        "division terminates at a stable 16 cells over stream; got tail {:?}",
        &counts[20..]
    );

    let s = engine.state();
    let keys = cells_keys(s);
    let n = cell_count(s);
    let glucose = s.get_field("glucose").and_then(|v| v.as_f64()).unwrap_or(-1.0);
    let acetate = s.get_field("acetate").and_then(|v| v.as_f64()).unwrap_or(-1.0);
    let mass = cell_mass_sum(s);

    // The exact terminal numbers — IDENTICAL to the proven local/rest run
    // (cells_division.rs): all glucose consumed → biomass + acetate at yield 0.6.
    assert!(glucose.abs() < 1e-6, "all glucose consumed over stream: {glucose}");
    assert!((mass - 25.0).abs() < 1e-6, "biomass = 1 + 0.6·40 = 25 pg: {mass}");
    assert!((acetate - 16.0).abs() < 1e-6, "acetate = 0.4·40 = 16: {acetate}");
    assert_eq!(n, 16, "16 daughters of mass 1.5625 each: {keys:?}");
    assert!(
        !keys.iter().any(|k| k == "c0"),
        "mother c0 replaced by its daughters (true division over stream): {keys:?}"
    );
    assert!(
        keys.iter().filter(|k| !k.starts_with('_')).all(|k| k.starts_with("c0")),
        "every surviving cell descends from c0: {keys:?}"
    );
}

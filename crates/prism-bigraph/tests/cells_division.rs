//! Cells as addressed `CompositeLink` nodes — the foundation under boundary-
//! crossing division (cells-and-division.md, Form-3-canonical model).
//!
//! A cell is **not** a `{_type, mass, body}` container; it IS an addressed
//! composite node `{address, config, mass}` sitting in a `cells: Map{CompositeLink}`.
//! That's what lets it be discovered schema-first, divided by the algebra, and
//! addressed by ANY protocol (local/stream/rest).
//!
//! This test proves the metabolic substrate, **mass-balanced** (no mass from
//! nothing): the cell takes up env `glucose` (env→cell), converts it to biomass
//! + `acetate` conserving mass, excretes `acetate` (cell→env), and exposes its
//! `mass` on its OWN node via the `%` self-node wire (the matchable face). The
//! invariant `glucose + Σ(cell mass) + acetate = const` is asserted across the
//! whole run — a sharp check that every bridge delta applies exactly once.

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use prism_bigraph::composite::Composite;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode, Step};
use prism_bigraph::protocol::ProtocolRegistry;
use prism_bigraph::protocols::{RestProcessServer, RestProtocol};
use prism_bigraph::{Core, Engine, Key, Schema, Update, Value};
use prism_schema::schema_to_value;

// ── Metabolism: the mass-balanced exchange (no mass from nothing) ────
//
// uptake u = k·glucose·mass·dt (autocatalytic, bounded by available glucose);
// Δmass = u·yield, Δacetate = u·(1-yield), Δglucose = -u. So
// Δmass + Δacetate + Δglucose = 0 — the system total is conserved by growth.
#[derive(Debug)]
struct Metabolism {
    k: f64,
    yield_frac: f64,
}
impl Process for Metabolism {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("mass".to_string(), Schema::float()),
            ("glucose".to_string(), Schema::float()),
        ])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("mass".to_string(), Schema::Delta { default: None, dimension: None }),
            ("glucose".to_string(), Schema::float()),
            ("acetate".to_string(), Schema::float()),
        ])
    }
    fn update(&self, state: &Value, interval: f64) -> Update {
        let mass = state.get_field("mass").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let glucose = state
            .get_field("glucose")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        // Bounded so a cell can never consume more glucose than is present
        // (keeps the pool non-negative even under the BSP snapshot, where every
        // cell reads the same pre-tick glucose).
        let u = (self.k * glucose * mass * interval).min(glucose).max(0.0);
        Update::value(Value::tree([
            ("mass", Value::float(u * self.yield_frac)),
            ("glucose", Value::float(-u)),
            ("acetate", Value::float(u * (1.0 - self.yield_frac))),
        ]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Trigger: the cell's INTERNAL "propose" — sets `divide` when mass crosses
//    the threshold. This is the inside half of Form 3: the cell decides, and the
//    decision propagates OUT onto its own face (`%.divide`) via the bridge. It
//    never reaches up to its container; it only exposes its readiness. ──
#[derive(Debug)]
struct Trigger {
    threshold: f64,
}
impl Process for Trigger {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("divide".to_string(), Schema::Bool { default: None })])
    }
    fn update(&self, state: &Value, _interval: f64) -> Update {
        let mass = state.get_field("mass").and_then(|v| v.as_f64()).unwrap_or(0.0);
        Update::value(Value::tree([("divide", Value::Bool(mass > self.threshold))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Divider: the environment's "dispose" — the outside half of Form 3. It
//    reads the cells map, finds a cell whose exposed face says `divide`, and
//    emits the `_divide` INTENT (`{_divide:{mother, daughters}}`). It does NOT
//    author the daughters' contents — the schema-holding `apply` ENACTS the
//    split via `divide_by_schema(CompositeLink)`. So structure (a cell becoming
//    two siblings) is enacted at the scope that owns structure — the container —
//    from the cell's proposal, never by reaching into the cell. One per tick,
//    so the `_divide` singleton never collides. ──
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
                // Override resets the divide marker so daughters (each below
                // threshold after the split) don't immediately re-propose.
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

fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String(s.to_string())).collect())
}

/// The inner-state schema of a cell (its body), honest (no `Any`): `mass` is the
/// extensive biomass (`Delta` — halves on divide), `glucose`/`acetate` are
/// working pools, `metabolism` is the process.
fn cell_inner_schema() -> Schema {
    Schema::Tree {
        branches: IndexMap::from([
            (Key::from("mass"), Schema::Delta { default: None, dimension: None }),
            (Key::from("glucose"), Schema::float()),
            (Key::from("acetate"), Schema::float()),
            (Key::from("divide"), Schema::Bool { default: None }),
            (
                Key::from("metabolism"),
                Schema::ProcessLink {
                    inputs: IndexMap::from([
                        (Key::from("mass"), Schema::float()),
                        (Key::from("glucose"), Schema::float()),
                    ]),
                    outputs: IndexMap::from([
                        (Key::from("mass"), Schema::Delta { default: None, dimension: None }),
                        (Key::from("glucose"), Schema::float()),
                        (Key::from("acetate"), Schema::float()),
                    ]),
                    interval: 1.0,
                },
            ),
            (
                Key::from("trigger"),
                Schema::ProcessLink {
                    inputs: IndexMap::from([(Key::from("mass"), Schema::float())]),
                    outputs: IndexMap::from([(Key::from("divide"), Schema::Bool { default: None })]),
                    interval: 1.0,
                },
            ),
        ]),
    }
}

/// The `CompositeLink` schema for a cell as it sits in `cells: Map{Cell}` — the
/// honest type that makes `cells.N` discoverable + divisible schema-first.
/// `outputs` is the exported face (incl. `mass:Delta`, the divisible quantity).
fn cell_link_schema() -> Schema {
    Schema::CompositeLink {
        inputs: IndexMap::from([
            (Key::from("mass"), Schema::Delta { default: None, dimension: None }),
            (Key::from("glucose"), Schema::float()),
        ]),
        outputs: IndexMap::from([
            (Key::from("mass"), Schema::Delta { default: None, dimension: None }),
            (Key::from("glucose"), Schema::float()),
            (Key::from("acetate"), Schema::float()),
            (Key::from("divide"), Schema::Bool { default: None }),
        ]),
        interval: 1.0,
        inner_schema: Box::new(cell_inner_schema()),
    }
}

/// An addressed cell node: `{address: local:Composite, config:{state,bridge,schema},
/// inputs, outputs, mass}`. NO container.
/// - env→cell: `glucose` read from the env pool (`^.glucose` = `[..,"glucose"]`).
/// - cell→env: `glucose` consumed back to the pool; `acetate` excreted to it.
/// - the FACE: `mass` read in and written out on the cell's OWN node
///   (`%.mass` = `["%","mass"]`) — the matchable/divisible exported quantity.
fn cell_node(address: Value, mass: f64, k: f64, yield_frac: f64, threshold: f64) -> Value {
    let metabolism = Value::tree([
        ("address", Value::String("local:Metabolism".into())),
        (
            "config",
            Value::tree([("k", Value::float(k)), ("yield", Value::float(yield_frac))]),
        ),
        (
            "inputs",
            Value::tree([("mass", wire(&["mass"])), ("glucose", wire(&["glucose"]))]),
        ),
        (
            "outputs",
            Value::tree([
                ("mass", wire(&["mass"])),
                ("glucose", wire(&["glucose"])),
                ("acetate", wire(&["acetate"])),
            ]),
        ),
    ]);
    let trigger = Value::tree([
        ("address", Value::String("local:Trigger".into())),
        ("config", Value::tree([("threshold", Value::float(threshold))])),
        ("inputs", Value::tree([("mass", wire(&["mass"]))])),
        ("outputs", Value::tree([("divide", wire(&["divide"]))])),
    ]);
    let inner_state = Value::tree([
        ("mass", Value::float(mass)),
        ("glucose", Value::float(0.0)),
        ("acetate", Value::float(0.0)),
        ("divide", Value::Bool(false)),
        ("metabolism", metabolism),
        ("trigger", trigger),
    ]);
    // The composite's INTERNAL bridge: ports ↔ inner paths.
    let bridge = Value::tree([
        (
            "inputs",
            Value::tree([("mass", wire(&["mass"])), ("glucose", wire(&["glucose"]))]),
        ),
        (
            "outputs",
            Value::tree([
                ("mass", wire(&["mass"])),
                ("glucose", wire(&["glucose"])),
                ("acetate", wire(&["acetate"])),
                ("divide", wire(&["divide"])),
            ]),
        ),
    ]);
    let config = Value::tree([
        ("state", inner_state),
        ("bridge", bridge),
        ("schema", schema_to_value(&cell_inner_schema())),
    ]);
    Value::tree([
        // local:Composite OR rest:Composite@server — the SAME cell, addressed by
        // whatever protocol. divide_by_schema shares `address`, so daughters
        // inherit the protocol (division is cross-protocol for free).
        ("address", address),
        ("config", config),
        // OUTER wires (engine-resolved): face on own node (`%`), pools up at env (`..`).
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
                // the divide INTENT, exposed on the cell's own node (`%.divide`)
                ("divide", wire(&["%", "divide"])),
            ]),
        ),
        // The initial exported face — populated/maintained by the bridge thereafter.
        ("mass", Value::float(mass)),
        ("divide", Value::Bool(false)),
    ])
}

fn core() -> Core {
    let mut registry = ProcessRegistry::new();
    registry.register("Metabolism", |config| {
        let k = config.get_field("k").and_then(|v| v.as_f64()).unwrap_or(0.1);
        let yield_frac = config
            .get_field("yield")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);
        ProcessNode::Process(Box::new(Metabolism { k, yield_frac }))
    });
    registry.register("Trigger", |config| {
        let threshold = config
            .get_field("threshold")
            .and_then(|v| v.as_f64())
            .unwrap_or(f64::INFINITY);
        ProcessNode::Process(Box::new(Trigger { threshold }))
    });
    registry.register("Divider", |_config| ProcessNode::Step(Box::new(Divider)));
    let handle: Arc<std::sync::OnceLock<Core>> = Arc::new(std::sync::OnceLock::new());
    {
        let handle = Arc::clone(&handle);
        registry.register("Composite", move |config| {
            let core = handle.get().expect("core handle");
            ProcessNode::Process(Box::new(
                Composite::from_config(&config, core).expect("from_config"),
            ))
        });
    }
    let registry = Arc::new(registry);
    let core = Core::from(Arc::clone(&registry));
    let _ = handle.set(core.clone());
    core
}

/// Top schema: env `glucose`/`acetate` pools + `cells: Map{CompositeLink}` —
/// honest, no `Schema::Any`.
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

fn cells_keys(state: &Value) -> Vec<String> {
    state
        .get_field("cells")
        .and_then(|v| v.as_map())
        .map(|m| m.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default()
}

/// The environment's `Divider` step node — sits beside `cells`, reads/writes it.
fn divider_node() -> Value {
    Value::tree([
        ("address", Value::String("local:Divider".into())),
        ("config", Value::map()),
        ("inputs", Value::tree([("cells", wire(&["cells"]))])),
        ("outputs", Value::tree([("cells", wire(&["cells"]))])),
    ])
}

/// Top schema with the `Divider` step beside the pools + cells.
fn top_schema_with_divider() -> Schema {
    let Schema::Tree { mut branches } = top_schema() else {
        unreachable!()
    };
    branches.insert(
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
    );
    Schema::Tree { branches }
}

fn cell_count(state: &Value) -> usize {
    state
        .get_field("cells")
        .and_then(|v| v.as_map())
        .map(|m| m.iter().filter(|(k, v)| !k.starts_with('_') && v.as_map().is_some()).count())
        .unwrap_or(0)
}

#[test]
fn schema_first_cell_grows_by_mass_balanced_exchange() {
    // One cell in `cells: Map{CompositeLink}`, an honest top schema (no `Any`).
    // The cell takes up glucose (env→cell), converts to biomass + acetate
    // (cell→env), exposes its mass on its own node. Mass is CONSERVED throughout.
    let initial = Value::tree([
        ("glucose", Value::float(10.0)),
        ("acetate", Value::float(0.0)),
        // threshold 1e9 → never divides in this growth-only test.
        ("cells", Value::tree([("c0", cell_node(local_addr(), 1.0, 0.2, 0.6, 1e9))])),
    ]);
    let total0 = system_total(&initial);
    assert!((total0 - 11.0).abs() < 1e-9, "initial total = glucose 10 + mass 1 = 11");

    let mut engine = Engine::from_state(top_schema(), initial, core()).expect("engine");
    engine.set_max_nodes(64); // runaway fails fast + light, not at the 16k default
    engine.discover_all_processes();

    // Step and check conservation at every tick.
    for _ in 0..30 {
        engine.run(1.0);
        let total = system_total(engine.state());
        assert!(
            (total - total0).abs() < 1e-6,
            "mass balance violated: total={total} vs {total0}\n  glucose={:?} acetate={:?} massΣ={}",
            engine.state().get_field("glucose").and_then(|v| v.as_f64()),
            engine.state().get_field("acetate").and_then(|v| v.as_f64()),
            cell_mass_sum(engine.state()),
        );
    }

    let final_state = engine.state();
    let glucose = final_state.get_field("glucose").and_then(|v| v.as_f64()).unwrap();
    let acetate = final_state.get_field("acetate").and_then(|v| v.as_f64()).unwrap();
    let mass = cell_mass_sum(final_state);

    assert!(glucose < 10.0, "glucose was consumed (env→cell): {glucose}");
    assert!(mass > 1.0, "biomass grew from the uptake: {mass}");
    assert!(acetate > 0.0, "acetate was excreted (cell→env): {acetate}");
    assert_eq!(cell_count(final_state), 1, "no division in this test");
    // The face on the cell's own node tracks the inner mass.
    let face = final_state
        .get_field("cells")
        .and_then(|v| v.get_field("c0"))
        .and_then(|c| c.get_field("mass"))
        .and_then(|v| v.as_f64())
        .unwrap_or(-1.0);
    assert!(
        (face - mass).abs() < 1e-9,
        "the exported %.mass face ({face}) equals the cell's biomass ({mass})"
    );
}

/// `local:Composite` — the cell runs in-process.
fn local_addr() -> Value {
    Value::String("local:Composite".into())
}

/// `rest:Composite@127.0.0.1:port` — the SAME cell, run over HTTP by a
/// RestProcessServer. (Shape per the rest protocol: `{protocol, data:{process,host,port}}`.)
fn rest_addr(port: u16) -> Value {
    Value::Map(IndexMap::from([
        (Key::from("protocol"), Value::String("rest".into())),
        (
            Key::from("data"),
            Value::Map(IndexMap::from([
                (Key::from("process"), Value::String("Composite".into())),
                (Key::from("host"), Value::String("127.0.0.1".into())),
                (Key::from("port"), Value::String(port.to_string())),
            ])),
        ),
    ]))
}

/// The CLIENT core for the rest case: the same full registry (so check_references
/// is satisfied — a rest cell's `config.state` references Metabolism/Trigger,
/// which run on the SERVER but must resolve as known names) + the `rest` protocol
/// so cells addressed `rest:` are driven over HTTP. The Composite/Metabolism/
/// Trigger factories aren't exercised locally (cells run remotely); the Divider
/// runs locally. (check_references not recursing into a remote node's config is a
/// cleaner future fix; registering the names is the minimal correct thing here.)
fn client_core() -> Core {
    let mut protocols = ProtocolRegistry::new(); // includes `local` (for the Divider node)
    protocols.register(Arc::new(RestProtocol));
    core().with_protocols(Arc::new(protocols))
}

/// The grow-divide-glucose environment: one cell at the given address, the env
/// glucose/acetate pools, and the Divider. Identical regardless of protocol.
fn make_initial(cell_addr: Value) -> Value {
    Value::tree([
        ("glucose", Value::float(40.0)),
        ("acetate", Value::float(0.0)),
        // threshold 2.0 → a cell at mass>2 proposes division.
        ("cells", Value::tree([("c0", cell_node(cell_addr, 1.0, 0.35, 0.6, 2.0))])),
        ("divider", divider_node()),
    ])
}

/// Run the grow-divide-glucose sim for 30 ticks, asserting mass conservation
/// EVERY tick + a runaway guard, then assert TRUE Form-3 division (mother
/// replaced by descendants). Returns the final `(glucose, acetate, massΣ, count)`
/// so callers can prove protocol-equivalence. `label` tags assert messages.
fn run_grow_divide(initial: Value, core: Core, label: &str) -> (f64, f64, f64, usize) {
    let total0 = system_total(&initial);
    assert!((total0 - 41.0).abs() < 1e-9, "[{label}] initial total = glucose 40 + mass 1 = 41");

    let mut engine =
        Engine::from_state(top_schema_with_divider(), initial, core).expect("engine");
    engine.set_max_nodes(64); // runaway fails fast + light, not at the default backstop
    engine.discover_all_processes();

    for _ in 0..30 {
        engine.run(1.0);
        // Conservation EVERY tick — across growth AND division. The SHARP check
        // that every bridge delta (incl. across the wire) applies exactly once.
        let total = system_total(engine.state());
        assert!(
            (total - total0).abs() < 1e-6,
            "[{label}] mass balance violated: total={total} vs {total0}\n  g={:?} a={:?} mΣ={} cells={:?}",
            engine.state().get_field("glucose").and_then(|v| v.as_f64()),
            engine.state().get_field("acetate").and_then(|v| v.as_f64()),
            cell_mass_sum(engine.state()),
            cells_keys(engine.state()),
        );
        assert!(
            cell_count(engine.state()) < 64,
            "[{label}] runaway division — cells unbounded ({})",
            cell_count(engine.state())
        );
    }

    let s = engine.state();
    let n = cell_count(s);
    let keys = cells_keys(s);
    assert!(n >= 2, "[{label}] the cell proposed division and the env enacted it; got {n} ({keys:?})");
    assert!(
        !keys.iter().any(|k| k == "c0"),
        "[{label}] mother c0 replaced by its daughters (true division): {keys:?}"
    );
    assert!(
        keys.iter().filter(|k| !k.starts_with('_')).all(|k| k.starts_with("c0")),
        "[{label}] every surviving cell descends from c0: {keys:?}"
    );
    let glucose = s.get_field("glucose").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let acetate = s.get_field("acetate").and_then(|v| v.as_f64()).unwrap_or(0.0);
    (glucose, acetate, cell_mass_sum(s), n)
}

#[test]
fn form3_cell_divides_via_intent_then_enact_conserving_mass() {
    // Form 3, canonical. The cell PROPOSES: its inner Trigger sets the %.divide
    // face when mass crosses the threshold (the inside decision, propagated out
    // through its own bridge). The environment DISPOSES: the Divider step turns
    // that proposal into a `_divide` INTENT, and the schema-holding apply ENACTS
    // the split via divide_by_schema(CompositeLink). Neither reaches across the
    // boundary — the cell exposes a face, the container rewrites its own
    // membership. Mass conserved across BOTH growth and division.
    run_grow_divide(make_initial(local_addr()), core(), "local");
}

#[test]
fn grow_divide_glucose_is_identical_local_and_over_rest() {
    // THE PROTOCOL-EQUIVALENCE PROOF. The SAME grow-divide-glucose cell, addressed
    // `local:` vs `rest:`, must divide + exchange glucose/acetate + conserve mass
    // IDENTICALLY. Because the composite bridge forwards the inner UPDATE intact
    // (the wire == the local bridge), nothing about the cell or the test changes —
    // only the address. The cell runs remotely on the server; the parent's Divider
    // still divides the cell NODES (rest-addressed → daughters inherit the
    // protocol), so division crosses the boundary for free.
    let server = RestProcessServer::start(core()).expect("start rest server");
    std::thread::sleep(Duration::from_millis(50));

    let local = run_grow_divide(make_initial(local_addr()), core(), "local");
    let remote = run_grow_divide(make_initial(rest_addr(server.port())), client_core(), "rest");

    assert!(
        (local.0 - remote.0).abs() < 1e-6
            && (local.1 - remote.1).abs() < 1e-6
            && (local.2 - remote.2).abs() < 1e-6
            && local.3 == remote.3,
        "grow-divide-glucose must be IDENTICAL local vs rest (glucose, acetate, massΣ, count):\n  \
         local={local:?}\n  rest ={remote:?}"
    );
    drop(server);
}

//! Port of `../process-bigraph/process_bigraph/processes/growth_division.py`:
//! a cell is a **subengine composite** (`from_config` with `{state, bridge}`)
//! that grows internally and **divides itself** by writing daughters *up*
//! through `outputs.environment = ['..']`. Division is observed by the cell
//! COUNT in the environment changing — never by reading a cell's inner mass.
//!
//! This proves subengine composites + internal division work end to end,
//! the upstream way (no flattening, no reaching into a composite).

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::composite::Composite;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode, Step};
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::{Core, Engine, Key, Schema, Update, Value};

// ── Grow: mass += mass * rate * interval (a delta) ──────────────────
#[derive(Debug)]
struct Grow {
    rate: f64,
}
impl Process for Grow {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".to_string(), Schema::float())])
    }
    fn update(&self, state: &Value, interval: f64) -> Update {
        let mass = state
            .get_field("mass")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        Update::value(Value::tree([(
            "mass",
            Value::float(mass * self.rate * interval),
        )]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Divide: when inner mass crosses threshold, replace self with two
//    daughters at the parent (environment) via the bridge's `['..']`. ──
#[derive(Debug)]
struct Divide {
    threshold: f64,
    agent_id: String,
}
impl Step for Divide {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("trigger".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("environment".to_string(), Schema::map(Schema::Any))])
    }
    fn update(&self, state: &Value) -> Update {
        let trigger = state
            .get_field("trigger")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if trigger <= self.threshold {
            return Update::Noop;
        }
        let d0 = format!("{}_0", self.agent_id);
        let d1 = format!("{}_1", self.agent_id);
        let half = trigger / 2.0;
        Update::value(Value::tree([(
            "environment",
            Value::Map(IndexMap::from([
                (
                    Key::from("_remove"),
                    Value::List(vec![Value::String(self.agent_id.clone())]),
                ),
                (
                    Key::from("_add"),
                    Value::tree([
                        (d0.as_str(), cell_spec(half, &d0)),
                        (d1.as_str(), cell_spec(half, &d1)),
                    ]),
                ),
            ])),
        )]))
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

/// A cell = a subengine composite `{address, config:{state, bridge}, …}`.
/// Inner state: `{mass, grow, divide}`. The cell exposes `environment`
/// (where Divide writes `_add`/`_remove`) up to its parent via `['..']`.
fn cell_spec(mass: f64, id: &str) -> Value {
    let grow = Value::tree([
        ("_type", Value::String("process".into())),
        ("address", Value::String("local:Grow".to_string())),
        ("config", Value::tree([("rate", Value::float(0.6))])),
        ("inputs", Value::tree([("mass", wire(&["mass"]))])),
        ("outputs", Value::tree([("mass", wire(&["mass"]))])),
    ]);
    let divide = Value::tree([
        ("_type", Value::String("step".into())),
        ("address", Value::String("local:Divide".to_string())),
        (
            "config",
            Value::tree([
                ("threshold", Value::float(2.0)),
                ("agent_id", Value::String(id.to_string())),
            ]),
        ),
        ("inputs", Value::tree([("trigger", wire(&["mass"]))])),
        (
            "outputs",
            Value::tree([("environment", wire(&["environment"]))]),
        ),
    ]);
    // NO inner `environment` slot: that makes the `environment` output port a
    // pure CONDUIT — the Divide step's structural `{_remove,_add}` is forwarded
    // to the PARENT intact (mother removed → true division 1→2), never applied
    // inside the cell (which would nest daughters → explosion). An inner slot
    // here would make the bridge round-trip through state and drop the `_remove`.
    let state = Value::tree([
        ("mass", Value::float(mass)),
        ("grow", grow),
        ("divide", divide),
    ]);
    let bridge = Value::tree([
        ("inputs", Value::map()),
        // the `environment` port has no inner slot → conduit to the parent
        (
            "outputs",
            Value::tree([("environment", wire(&["environment"]))]),
        ),
    ]);
    Value::tree([
        ("_type", Value::String("composite".into())),
        ("address", Value::String("local:Composite".to_string())),
        (
            "config",
            Value::tree([("state", state), ("bridge", bridge)]),
        ),
        ("inputs", Value::map()),
        // The composite's `environment` output lands on its PARENT store
        // (the map holding the cells). prism is parent-relative, so the
        // empty wire `[]` == "my parent" (upstream's self-relative `['..']`).
        ("outputs", Value::tree([("environment", wire(&[]))])),
    ])
}

#[test]
fn subengine_cell_grows_and_divides_via_bridge() {
    let mut registry = ProcessRegistry::new();
    registry.register("Grow", |config| {
        let rate = config
            .get_field("rate")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.1);
        ProcessNode::Process(Box::new(Grow { rate }))
    });
    registry.register("Divide", |config| {
        let threshold = config
            .get_field("threshold")
            .and_then(|v| v.as_f64())
            .unwrap_or(2.0);
        let agent_id = config
            .get_field("agent_id")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        ProcessNode::Step(Box::new(Divide {
            threshold,
            agent_id,
        }))
    });
    // The cell composite is discovered/instantiated through from_config too —
    // it captures the whole Core (set once everything is built).
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

    // Top engine: an `environment` map holding one cell, plus the cell's
    // process spec so it is discovered.
    let mut topo = Topology::new();
    topo.state_schema = Schema::Any;
    topo.initial_state = Value::tree([(
        "environment",
        Value::tree([("cell", cell_spec(1.6, "cell"))]),
    )]);
    // The REAL schema (not Schema::Any): the environment is a Map of cell
    // composites. Typed so the engine resolves cells as CompositeLink nodes and
    // apply handles the division `_remove`/`_add` — so the mother is actually
    // removed (true division 1→2) instead of lingering as a zombie (the
    // Schema::Any path → full binary tree → explosion).
    let cell_link = Schema::CompositeLink {
        inputs: IndexMap::new(),
        outputs: IndexMap::from([(Key::from("environment"), Schema::map(Schema::Any))]),
        interval: 1.0,
        inner_schema: Box::new(Schema::Tree {
            branches: IndexMap::from([
                (Key::from("mass"), Schema::float()),
                (Key::from("environment"), Schema::map(Schema::Any)),
            ]),
        }),
    };
    let real_schema = Schema::Tree {
        branches: IndexMap::from([(
            Key::from("environment"),
            Schema::Map { value: Box::new(cell_link) },
        )]),
    };
    let mut engine =
        Engine::from_state(real_schema, topo.initial_state.clone(), core).expect("engine");
    engine.set_max_nodes(64); // bounded if correct; trips fast + light if not
    engine.discover_all_processes();

    let count = |e: &Engine| {
        e.state()
            .get_field("environment")
            .and_then(|v| {
                v.as_map()
                    .map(|m| m.keys().filter(|k| !k.starts_with('_')).count())
            })
            .unwrap_or(0)
    };
    let keys = |e: &Engine| -> Vec<String> {
        e.state()
            .get_field("environment")
            .and_then(|v| {
                v.as_map().map(|m| {
                    m.keys()
                        .filter(|k| !k.starts_with('_'))
                        .map(|k| k.to_string())
                        .collect()
                })
            })
            .unwrap_or_default()
    };
    // A few ticks: the cell grows past threshold and divides via the bridge. With
    // the bridge forwarding the division UPDATE intact (the `environment` port is
    // a conduit), the mother is truly REMOVED and replaced by its daughters.
    for _ in 0..4 {
        engine.run(1.0);
    }
    let final_keys = keys(&engine);
    let n = final_keys.len();

    assert!(n >= 2, "the cell should grow past threshold and divide (got {n})");
    // TRUE division (not zombie budding): the root mother and every intermediate
    // mother were removed — no surviving cell is an ancestor of another. The old
    // diff-bridge dropped the `_remove`, leaving the whole ancestor tree (zombies)
    // → exponential blow-up. The conduit bridge forwards `_remove` intact.
    assert!(
        !final_keys.iter().any(|k| k == "cell"),
        "root mother `cell` removed by true division: {final_keys:?}"
    );
    for a in &final_keys {
        let prefix = format!("{a}_");
        assert!(
            !final_keys.iter().any(|b| b != a && b.starts_with(&prefix)),
            "no surviving cell is an ancestor of another (zombie!): {a} among {final_keys:?}"
        );
    }
}

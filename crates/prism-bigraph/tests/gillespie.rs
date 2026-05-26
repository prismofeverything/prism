//! `MinimalGillespie` recreated (port of
//! `../process-bigraph/process_bigraph/experiments/minimal_gillespie.py`):
//! a Gillespie made of **two separate nodes** — a Process that fires the next
//! reaction event, and a Step that chooses the *time interval* (the wait τ) for
//! that process. The Step **overwrites** the event's `interval` in state, and the
//! engine schedules the event by its CURRENT `[event, "interval"]` — so the
//! timestep is **dynamic**: `interval` lives in state, a step rewrites it, and the
//! process always runs its local interval at the time of the update call.
//!
//! (Deterministic τ = 1/(k·a) rather than `exponential(1/Σpropensity)`, so the
//! schedule is exactly assertable; the *mechanism* — dynamic interval-from-state
//! — is identical to the stochastic original.)

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode, Step};
use prism_bigraph::{Core, Engine, Schema, Update, Value};

fn overwrite_float() -> Schema {
    Schema::Overwrite {
        inner: Box::new(Schema::float()),
    }
}

fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String(s.to_string())).collect())
}

/// GillespieEvent — fires one reaction per update (consumes one `a`).
#[derive(Debug)]
struct GillespieEvent;
impl Process for GillespieEvent {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("a".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("a".to_string(), Schema::float())])
    }
    // Initial wait before the Step takes over (= 1/(k·a₀) for k=1, a₀=5).
    fn interval(&self) -> f64 {
        0.2
    }
    fn update(&self, state: &Value, _interval: f64) -> Update {
        let a = state.get_field("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let da = if a > 0.0 { -1.0 } else { 0.0 }; // one reaction event
        Update::value(Value::tree([("a", Value::float(da))])) // additive: a -= 1
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// GillespieInterval — the wait τ = 1/(k·a) (propensity-based; as `a` is consumed
/// τ grows). OVERWRITES the event's interval, making the next timestep dynamic.
#[derive(Debug)]
struct GillespieInterval {
    k: f64,
}
impl Step for GillespieInterval {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("a".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("interval".to_string(), overwrite_float())])
    }
    fn update(&self, state: &Value) -> Update {
        let a = state.get_field("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let propensity = (self.k * a).max(1e-9);
        Update::value(Value::tree([("interval", Value::float(1.0 / propensity))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn step_drives_a_dynamic_process_interval() {
    let mut reg = ProcessRegistry::new();
    reg.register("GillespieEvent", |_| {
        ProcessNode::Process(Box::new(GillespieEvent))
    });
    reg.register("GillespieInterval", |c| {
        let k = c.get_field("k").and_then(|v| v.as_f64()).unwrap_or(1.0);
        ProcessNode::Step(Box::new(GillespieInterval { k }))
    });
    let core = Core::from(Arc::new(reg));

    // Two nodes sharing the species `a`. The Step's `interval` output is wired to
    // the EVENT node's interval slot — `[event, "interval"]` — which the engine
    // reads (and the Step overwrites) to schedule the event.
    let event = Value::tree([
        ("_type", Value::from("process")),
        ("address", Value::from("local:GillespieEvent")),
        ("inputs", Value::tree([("a", wire(&["a"]))])),
        ("outputs", Value::tree([("a", wire(&["a"]))])),
    ]);
    let ticker = Value::tree([
        ("_type", Value::from("step")),
        ("address", Value::from("local:GillespieInterval")),
        ("config", Value::tree([("k", Value::float(1.0))])),
        ("inputs", Value::tree([("a", wire(&["a"]))])),
        (
            "outputs",
            Value::tree([("interval", wire(&["event", "interval"]))]),
        ),
    ]);
    let state = Value::tree([
        ("a", Value::float(5.0)),
        ("event", event),
        ("ticker", ticker),
    ]);

    let mut engine = Engine::from_state(Schema::Any, state, core).expect("engine");
    engine.discover_all_processes();

    // Step event-by-event; the gap between consecutive event times IS the interval
    // the engine actually scheduled by.
    let round3 = |x: f64| (x * 1000.0).round() / 1000.0;
    let mut times = Vec::new();
    for _ in 0..5 {
        if engine.tick().is_none() {
            break;
        }
        times.push(engine.time());
    }
    let mut intervals = Vec::new();
    let mut prev = 0.0;
    for &t in &times {
        intervals.push(round3(t - prev));
        prev = t;
    }
    let a_left = engine
        .state()
        .get_field("a")
        .and_then(|v| v.as_f64())
        .unwrap_or(-1.0);
    eprintln!("event times: {times:?}\ndynamic intervals used: {intervals:?}\na left: {a_left}");

    // The DECISIVE check: the engine scheduled each event by τ = 1/(k·a) for
    // a = 5,4,3,2,1 — i.e. the Step's OVERWRITE landed and the scheduler re-read
    // the live interval each tick. A fixed-interval engine gives [0.2; 0.2; …];
    // an *additive* (non-overwrite) interval gives [0.2, 0.45, 0.783, …].
    assert_eq!(
        intervals,
        vec![0.2, 0.25, 0.333, 0.5, 1.0],
        "engine must schedule by the dynamic τ = 1/(k·a); got {intervals:?}"
    );
    assert_eq!(a_left, 0.0, "all five `a` reacted");
}

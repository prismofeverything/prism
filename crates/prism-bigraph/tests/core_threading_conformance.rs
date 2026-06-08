//! **Core-threading conformance — the missing test dimension.**
//!
//! Every existing rest/stream/parallel test crosses a boundary with BASE types
//! (`float`/`map`) and a process-only registry, so none ever asks the boundary to
//! dispatch a value only the full [`Core`] knows how to encode. That blind spot is
//! exactly why the protocol layer could thread a registry *subset* and the gap go
//! uncaught. This test adds the dimension: a non-base `Custom` type (`Boxed`)
//! whose payload is a [`Value::Foreign`], hence OPAQUE to plain JSON
//! (`value_to_json` turns a `Foreign` into `null`). The only way it survives a real
//! `rest:` round-trip is for the boundary codec to dispatch the type's
//! `serialize`/`realize` — which needs the `TypeRegistry`, which lives in the
//! `Core`. With the protocol layer threading the whole `Core`, the rest client +
//! server call the codec with `Some(core.types())`, the `Foreign` survives as data
//! and is rebuilt on the far side. (Before that, both ends pass `reg = None`, the
//! `Foreign` nulls, and the value is LOST — this test fails.)
//!
//! The property pinned: **a transport differs from `local` only in protocol, never
//! in the values it can carry.** `local == rest == parallel` for the SAME
//! non-base-typed program. (Stream is the chrysalis-level analogue, already
//! type-aware via `runner::serve_process`; this is the prism-bigraph half.)

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocols::{RestProcessServer, RestProtocol};
use prism_bigraph::{Core, ParallelProtocol, Protocol, Schema, Update, Value};
use prism_schema::registry::TypeMethods;
use prism_schema::value::Foreign;
use prism_schema::{DivideContext, MethodRegistry, TypeRegistry};

const BOXED: &str = "Boxed";

/// A `Boxed` value: an `i64` hidden inside a `Foreign`, so it is opaque to JSON.
fn boxed(n: i64) -> Value {
    Value::Foreign(Foreign::new(BOXED, n))
}

/// Recover the `out` port's `Boxed` payload, or `None` if it was lost (nulled by a
/// registry-blind JSON codec) or was never a `Boxed`.
fn unbox_out(update: &Value) -> Option<i64> {
    match update.as_map()?.get("out")? {
        Value::Foreign(f) if f.type_name == BOXED => f.downcast_ref::<i64>().copied(),
        _ => None,
    }
}

fn boxed_schema() -> Schema {
    Schema::Custom {
        name: BOXED.to_string(),
        parameters: IndexMap::new(),
    }
}

/// `Boxed`'s type methods — serialize a `Foreign` down to an `Int`; realize an
/// `Int` back up to the `Foreign`. Neither runs unless the boundary has the
/// registry, which is the whole point.
#[derive(Debug)]
struct BoxedMethods;
impl TypeMethods for BoxedMethods {
    fn default(&self, _r: &TypeRegistry, _s: &Schema) -> Value {
        boxed(0)
    }
    fn apply(&self, _r: &TypeRegistry, _s: &Schema, _state: &Value, update: &Value) -> Value {
        update.clone()
    }
    fn divide(
        &self,
        _r: &TypeRegistry,
        _s: &Schema,
        state: &Value,
        ctx: &DivideContext,
    ) -> Vec<Value> {
        vec![state.clone(); ctx.n_daughters.max(2)]
    }
    fn serialize(&self, _r: &TypeRegistry, _s: &Schema, state: &Value) -> Value {
        match state {
            Value::Foreign(f) if f.type_name == BOXED => f
                .downcast_ref::<i64>()
                .map(|n| Value::Int(*n))
                .unwrap_or(Value::None),
            other => other.clone(),
        }
    }
    fn realize(&self, _r: &TypeRegistry, _s: &Schema, encoded: &Value) -> Value {
        if let Value::Int(n) = encoded {
            return boxed(*n);
        }
        match encoded.as_f64() {
            Some(f) => boxed(f as i64),
            None => encoded.clone(),
        }
    }
}

/// A process emitting a `Boxed` on its `out` port — the value only a type-aware
/// boundary can carry.
#[derive(Debug)]
struct EmitBoxed;
impl Process for EmitBoxed {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("tick".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("out".to_string(), boxed_schema())])
    }
    fn update(&self, _state: &Value, _interval: f64) -> Update {
        Update::value(Value::tree([("out", boxed(42))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A process that, server-side, dispatches a CUSTOM METHOD (`Boxed.describe`)
/// through the Core it was `set_core`'d with — the method axis. `subject :: Boxed`
/// crosses by schema (type axis); the dispatch result (a `String`) and a doubled
/// `Boxed` cross back. Reaching `core.methods` at all requires the WHOLE Core on
/// the node — a `core.types` subset carries no method matrix.
#[derive(Debug, Default)]
struct BoxedDescribe {
    core: Option<Core>,
}
impl Process for BoxedDescribe {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("subject".to_string(), boxed_schema())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("note".to_string(), Schema::string()),
            ("out".to_string(), boxed_schema()),
        ])
    }
    fn set_core(&mut self, core: Core) {
        self.core = Some(core);
    }
    fn update(&self, state: &Value, _interval: f64) -> Update {
        let subject = state
            .as_map()
            .and_then(|m| m.get("subject"))
            .cloned()
            .unwrap_or(Value::None);
        let n = match &subject {
            Value::Foreign(f) if f.type_name == BOXED => {
                f.downcast_ref::<i64>().copied().unwrap_or(0)
            }
            _ => 0,
        };
        // Dispatch the custom method through the threaded Core — NOT reimplemented
        // here. A node without the Core (a subset-threaded one) yields `note = None`
        // and the test fails: that is the method axis crossing, made observable.
        let note = self
            .core
            .as_ref()
            .and_then(|c| c.methods.dispatch(&subject, "describe", &[]).ok())
            .unwrap_or(Value::None);
        Update::value(Value::tree([("note", note), ("out", boxed(n * 2))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// One `Core` carrying every non-default registry this suite exercises: the `Boxed`
/// TYPE (+ its `TypeMethods`), the `EmitBoxed`/`BoxedDescribe` PROCESS factories,
/// and the `(Boxed, describe)` METHOD — the whole core, shared by server and
/// clients (the #48 "type package shared both ends", in-process). `rest` is itself
/// the non-default PROTOCOL the crossings exercise.
fn boxed_core() -> Core {
    let mut types = TypeRegistry::new();
    types.register_full(
        BOXED,
        boxed_schema(),
        None,
        Some(Arc::new(BoxedMethods)),
        Vec::new(),
    );

    let mut processes = ProcessRegistry::new();
    processes.register("EmitBoxed", |_| ProcessNode::Process(Box::new(EmitBoxed)));
    processes.register("BoxedDescribe", |_| {
        ProcessNode::Process(Box::new(BoxedDescribe::default()))
    });

    // A new COLUMN in the open type×method matrix: `(Boxed, describe)`. Registered
    // against the type NAME with no trait edit — that's the expression-problem
    // matrix (`MethodRegistry`), distinct from the closed `TypeMethods` algebra.
    // It crosses the boundary only because a server-side node is `set_core`'d with
    // the WHOLE Core; a `core.types` subset would carry no methods at all.
    let mut methods = MethodRegistry::new();
    methods.register(BOXED, "describe", |recv, _| {
        let n = match recv {
            Value::Foreign(f) if f.type_name == BOXED => f.downcast_ref::<i64>().copied(),
            _ => None,
        };
        Ok(Value::String(format!("boxed#{}", n.unwrap_or(-1))))
    });

    Core::new()
        .with_types(Arc::new(types))
        .with_processes(Arc::new(processes))
        .with_methods(Arc::new(methods))
}

/// Drive one tick of a freshly-instantiated node. `set_core` first — exactly what
/// the engine does for every node during discovery (and the rest server does for
/// the node it builds server-side), so a method-dispatching process reaches
/// `core.methods`. For a `RestProcess` this is a no-op (it already holds the Core
/// from `initialize`); the dispatch then happens server-side.
fn run(mut node: ProcessNode, core: &Core, state: &Value) -> Value {
    node.set_core(core.clone());
    match node {
        ProcessNode::Process(p) => p.update(state, 1.0).into_value().unwrap_or(Value::None),
        ProcessNode::Step(s) => s.update(state).into_value().unwrap_or(Value::None),
    }
}

#[test]
fn a_custom_foreign_typed_port_survives_rest_identically_to_local() {
    let core = boxed_core();
    let state = Value::tree([("tick", Value::float(0.0))]);

    // ── local: the in-process baseline ──
    let local_node = core
        .processes
        .create("EmitBoxed", Value::None)
        .expect("local EmitBoxed");
    let local_out = run(local_node, &core, &state);
    assert_eq!(unbox_out(&local_out), Some(42), "local carries the Boxed payload");

    // ── parallel: in-process pool, no serialization — must also carry it ──
    let parallel = ParallelProtocol::default();
    let p_node = parallel
        .instantiate(&Value::String("EmitBoxed".into()), Value::None, &core)
        .expect("parallel EmitBoxed");
    let parallel_out = run(p_node, &core, &state);
    assert_eq!(
        unbox_out(&parallel_out),
        Some(42),
        "parallel carries the Boxed payload"
    );

    // ── rest: a REAL HTTP boundary. The Foreign is opaque to JSON, so only the
    //    type-aware codec (the Core's registry threaded onto both ends) carries it. ──
    let server = RestProcessServer::start(core.clone()).expect("start rest server");
    std::thread::sleep(Duration::from_millis(50));
    let addr = Value::Map(IndexMap::from_iter([
        ("process".into(), Value::String("EmitBoxed".into())),
        ("host".into(), Value::String("127.0.0.1".into())),
        ("port".into(), Value::Int(server.port() as i64)),
    ]));
    let rest_node = RestProtocol
        .instantiate(&addr, Value::None, &core)
        .expect("rest EmitBoxed");
    let rest_out = run(rest_node, &core, &state);
    assert_eq!(
        unbox_out(&rest_out),
        Some(42),
        "rest must CARRY the Boxed payload, not null it — the Core-threading property"
    );

    // The invariant: a transport differs only in protocol, never in payload.
    assert_eq!(
        unbox_out(&local_out),
        unbox_out(&rest_out),
        "rest == local"
    );
    assert_eq!(
        unbox_out(&local_out),
        unbox_out(&parallel_out),
        "parallel == local"
    );
}

#[test]
fn all_four_core_registries_cross_rest_in_one_node() {
    // The diagnostic the user asked for: ONE crossing that exercises every
    // non-default Core registry, so threading a *subset* could not satisfy it.
    //   • TYPE     — `subject :: Boxed` / `out :: Boxed` cross by schema (codec).
    //   • PROCESS  — `BoxedDescribe` from a non-default factory.
    //   • METHOD   — `Boxed.describe` dispatched SERVER-SIDE (needs the node
    //                `set_core`'d with the whole Core — `core.methods`).
    //   • PROTOCOL — `rest` itself (the default registry is `local`-only).
    // `rest == local` ⇒ a transport differs only in protocol, never in the parts
    // of the system it can carry.
    let core = boxed_core();
    let state = Value::tree([("subject", boxed(7))]);

    let local = run(
        core.processes
            .create("BoxedDescribe", Value::None)
            .expect("local BoxedDescribe"),
        &core,
        &state,
    );

    let server = RestProcessServer::start(core.clone()).expect("start rest server");
    std::thread::sleep(Duration::from_millis(50));
    let addr = Value::Map(IndexMap::from_iter([
        ("process".into(), Value::String("BoxedDescribe".into())),
        ("host".into(), Value::String("127.0.0.1".into())),
        ("port".into(), Value::Int(server.port() as i64)),
    ]));
    let rest = run(
        RestProtocol
            .instantiate(&addr, Value::None, &core)
            .expect("rest BoxedDescribe"),
        &core,
        &state,
    );

    // parallel: the INNER `BoxedDescribe` must also reach `core.methods`. The
    // wrapper threads the Core into its inner at instantiation (it cannot later,
    // through the shared `Arc`) — without that, the dispatch yields `None`.
    let parallel = ParallelProtocol::default();
    let par = run(
        parallel
            .instantiate(&Value::String("BoxedDescribe".into()), Value::None, &core)
            .expect("parallel BoxedDescribe"),
        &core,
        &state,
    );

    let note = |v: &Value| {
        v.as_map()
            .and_then(|m| m.get("note"))
            .and_then(|s| s.as_str().map(str::to_string))
    };

    // The METHOD axis: a String the dispatch produced, server-side over rest.
    assert_eq!(
        note(&local).as_deref(),
        Some("boxed#7"),
        "local dispatches Boxed.describe"
    );
    assert_eq!(
        note(&rest).as_deref(),
        Some("boxed#7"),
        "rest dispatches Boxed.describe SERVER-SIDE — the method matrix crossed via the whole Core"
    );
    assert_eq!(
        note(&par).as_deref(),
        Some("boxed#7"),
        "parallel dispatches Boxed.describe via its INNER's Core (threaded at instantiation)"
    );
    // The TYPE axis: the Boxed output crossed by schema, doubled, same all ways.
    assert_eq!(unbox_out(&local), Some(14), "local doubles the Boxed payload");
    assert_eq!(unbox_out(&rest), Some(14), "rest carries the doubled Boxed payload");
    assert_eq!(unbox_out(&par), Some(14), "parallel carries the doubled Boxed payload");

    // The invariant: a transport differs from local only in protocol.
    assert_eq!(note(&local), note(&rest), "rest == local");
    assert_eq!(note(&local), note(&par), "parallel == local");
    assert_eq!(unbox_out(&local), unbox_out(&rest));
    assert_eq!(unbox_out(&local), unbox_out(&par));
}

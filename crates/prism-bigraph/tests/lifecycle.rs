//! Integration test for the invoke → flush → collect lifecycle.
//!
//! Sync processes work without any protocol runtime registered (the
//! default `Process::invoke` returns an immediate `Defer`). This test
//! also wires up a synthetic batching protocol — a `BatchRuntime` that
//! defers its `Process::invoke` calls into slots and fills them on
//! `flush_pending()` — to prove the `Defer`/`ProtocolRuntime` pair
//! works end to end.

use std::any::Any;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use prism_bigraph::defer::{Defer, DeferSlot};
use prism_bigraph::ports::PortSchema;
use prism_bigraph::process::Process;
use prism_bigraph::protocol_runtime::ProtocolRuntime;
use prism_bigraph::update::Update;
use prism_schema::{Schema, Value};

// ── Sync default: invoke() returns immediate Defer ──────────────────

#[derive(Debug)]
struct DoubleProcess;

impl Process for DoubleProcess {
    fn inputs(&self) -> PortSchema {
        IndexMap::from([("x".to_string(), Schema::float())])
    }
    fn outputs(&self) -> PortSchema {
        IndexMap::from([("x".to_string(), Schema::float())])
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn update(&self, state: &Value, _interval: f64) -> Update {
        let x = state
            .as_map()
            .and_then(|m| m.get("x"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        Update::value(Value::tree([("x", Value::float(x * 2.0))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn sync_process_invoke_returns_immediate_defer() {
    let p = DoubleProcess;
    let state = Value::tree([("x", Value::float(3.0))]);
    let defer = p.invoke(&state, 1.0);
    let upd = defer.get().into_value().unwrap();
    let x = upd.as_map().unwrap().get("x").unwrap().as_f64().unwrap();
    assert!((x - 6.0).abs() < 1e-9);
}

// ── Batching runtime: invoke enqueues, flush resolves ───────────────

/// A synthetic batching protocol. Processes wired to it return
/// `Defer::slot()`s; the runtime collects pending operands during the
/// invoke pass and fills the slots in one `flush_pending` call.
#[derive(Debug, Default)]
struct BatchRuntime {
    /// Pending (slot, operand) pairs queued during the invoke pass.
    pending: Mutex<Vec<(DeferSlot<Update>, f64)>>,
    /// How many times flush_pending was called.
    flush_count: Mutex<usize>,
}

impl BatchRuntime {
    fn enqueue(&self, slot: DeferSlot<Update>, operand: f64) {
        self.pending.lock().unwrap().push((slot, operand));
    }

    fn flush_count(&self) -> usize {
        *self.flush_count.lock().unwrap()
    }
}

impl ProtocolRuntime for BatchRuntime {
    fn flush_pending(&self) {
        *self.flush_count.lock().unwrap() += 1;
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        for (slot, operand) in pending {
            // "Batched" computation — just doubles each operand.
            let result = Update::value(Value::tree([("x", Value::float(operand * 2.0))]));
            slot.fill(result);
        }
    }
    fn label(&self) -> &str {
        "batch"
    }
}

#[derive(Debug)]
struct BatchProcess {
    runtime: Arc<BatchRuntime>,
}

impl Process for BatchProcess {
    fn inputs(&self) -> PortSchema {
        IndexMap::from([("x".to_string(), Schema::float())])
    }
    fn outputs(&self) -> PortSchema {
        IndexMap::from([("x".to_string(), Schema::float())])
    }
    fn interval(&self) -> f64 {
        1.0
    }

    /// Synchronous `update` exists as a fallback, but in real usage
    /// for a batching protocol you'd never call it directly.
    fn update(&self, _state: &Value, _interval: f64) -> Update {
        panic!("BatchProcess::update called directly — should use invoke + flush");
    }

    fn invoke(&self, state: &Value, _interval: f64) -> Defer<Update> {
        let x = state
            .as_map()
            .and_then(|m| m.get("x"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let (defer, slot) = Defer::slot();
        self.runtime.enqueue(slot, x);
        defer
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn batching_runtime_full_lifecycle() {
    let runtime = Arc::new(BatchRuntime::default());

    let processes: Vec<Box<dyn Process>> = (0..4)
        .map(|_| {
            Box::new(BatchProcess {
                runtime: Arc::clone(&runtime),
            }) as Box<dyn Process>
        })
        .collect();

    let inputs: Vec<Value> = (1..=4)
        .map(|i| Value::tree([("x", Value::float(i as f64))]))
        .collect();

    // Phase 1: invoke — enqueues onto the runtime, doesn't compute yet.
    let defers: Vec<Defer<Update>> = processes
        .iter()
        .zip(inputs.iter())
        .map(|(p, s)| p.invoke(s, 1.0))
        .collect();

    assert_eq!(runtime.flush_count(), 0, "no flushes during invoke pass");
    assert_eq!(
        runtime.pending.lock().unwrap().len(),
        4,
        "all 4 calls queued"
    );

    // Phase 2: flush — one batched resolution for all 4 processes.
    runtime.flush_pending();
    assert_eq!(runtime.flush_count(), 1);
    assert!(
        runtime.pending.lock().unwrap().is_empty(),
        "flush drains pending"
    );

    // Phase 3: collect — each defer pulls its filled slot.
    let results: Vec<f64> = defers
        .into_iter()
        .map(|d| {
            d.get()
                .into_value()
                .unwrap()
                .as_map()
                .unwrap()
                .get("x")
                .unwrap()
                .as_f64()
                .unwrap()
        })
        .collect();

    assert_eq!(results, vec![2.0, 4.0, 6.0, 8.0]);
}

#[test]
fn engine_holds_protocol_runtime_registry() {
    use prism_bigraph::Engine;

    let mut engine = Engine::builder()
        .schema(Schema::Any)
        .state(Value::map())
        .build()
        .expect("build");

    assert_eq!(engine.protocol_runtime_count(), 0);

    let rt = Arc::new(BatchRuntime::default());
    engine.register_protocol_runtime(Arc::clone(&rt) as Arc<dyn ProtocolRuntime>);
    assert_eq!(engine.protocol_runtime_count(), 1);

    // Flush is a no-op without pending work; just confirms wiring.
    engine.flush_protocol_runtimes();
    assert_eq!(rt.flush_count(), 1);
}

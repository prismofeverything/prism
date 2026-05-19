//! `parallel` protocol — each process runs on its own worker thread.
//!
//! Upstream's `process_bigraph.protocols.parallel` uses Python's
//! `multiprocessing` to put each process in a separate OS process for
//! true parallelism (Python's GIL means threads don't parallelize CPU
//! work). Rust threads *do* parallelize — they're independent
//! schedulable units sharing the same address space — so the Rust
//! analogue ships work to **worker threads** instead.
//!
//! Semantically the API is identical to the local protocol: a
//! `parallel:Cell` process exposes the same `Process` trait surface
//! as a `local:Cell`. The difference is where its `update()` body
//! runs.
//!
//! ## Today
//!
//! Each [`ParallelProcess`] owns a single worker thread. `update()`
//! sends the input state over a sync channel, waits for the worker to
//! finish, returns the result. Sequential calls serialize on the
//! channel; parallelism gain shows up when multiple `ParallelProcess`
//! instances are invoked from different orchestrator threads.
//!
//! ## Future
//!
//! When the Composite / Engine orchestrator gets the invoke → flush →
//! collect refactor (#22 fully wired), `parallel`'s `Process::invoke`
//! can return a [`crate::defer::Defer`] backed by a `DeferSlot` and
//! enqueue work on a shared thread pool — that's where the
//! parallelism gain becomes visible.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use prism_schema::Value;

use crate::process::{Process, ProcessNode};
use crate::protocol::{Protocol, ProtocolError};
use crate::factory::ProcessRegistry;

// ── Worker protocol ─────────────────────────────────────────────────

enum WorkerRequest {
    Update { state: Value, interval: f64 },
    Shutdown,
}

enum WorkerResponse {
    Update(crate::update::Update),
}

// ── ParallelProcess: a Process that proxies to a worker thread ─────

pub struct ParallelProcess {
    class_name: String,
    cached_inputs: crate::ports::PortSchema,
    cached_outputs: crate::ports::PortSchema,
    cached_interval: f64,
    request_tx: SyncSender<WorkerRequest>,
    response_rx: Mutex<Receiver<WorkerResponse>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for ParallelProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParallelProcess")
            .field("class", &self.class_name)
            .field("interval", &self.cached_interval)
            .finish_non_exhaustive()
    }
}

impl ParallelProcess {
    /// Spawn a worker thread hosting `process`. The worker owns the
    /// process for its lifetime; `Drop` shuts the worker down cleanly.
    pub fn spawn(process: Box<dyn Process>, class_name: String) -> Self {
        let cached_inputs = process.inputs();
        let cached_outputs = process.outputs();
        let cached_interval = process.interval();

        // 0-buffer sync channels: send blocks until receiver is ready,
        // so the parent's `update()` and the worker's loop step in
        // lockstep — no queue buildup.
        let (request_tx, request_rx) = sync_channel::<WorkerRequest>(0);
        let (response_tx, response_rx) = sync_channel::<WorkerResponse>(0);

        let worker_class = class_name.clone();
        let worker = std::thread::Builder::new()
            .name(format!("parallel-{}", worker_class))
            .spawn(move || {
                while let Ok(req) = request_rx.recv() {
                    match req {
                        WorkerRequest::Update { state, interval } => {
                            let upd = process.update(&state, interval);
                            if response_tx.send(WorkerResponse::Update(upd)).is_err() {
                                break;
                            }
                        }
                        WorkerRequest::Shutdown => break,
                    }
                }
            })
            .expect("spawn worker thread");

        Self {
            class_name,
            cached_inputs,
            cached_outputs,
            cached_interval,
            request_tx,
            response_rx: Mutex::new(response_rx),
            worker: Mutex::new(Some(worker)),
        }
    }
}

impl Drop for ParallelProcess {
    fn drop(&mut self) {
        // Signal shutdown; ignore error (worker may have died already).
        let _ = self.request_tx.send(WorkerRequest::Shutdown);
        if let Some(handle) = self.worker.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

impl Process for ParallelProcess {
    fn inputs(&self) -> crate::ports::PortSchema {
        self.cached_inputs.clone()
    }
    fn outputs(&self) -> crate::ports::PortSchema {
        self.cached_outputs.clone()
    }
    fn interval(&self) -> f64 {
        self.cached_interval
    }

    fn update(&self, state: &Value, interval: f64) -> crate::update::Update {
        if self
            .request_tx
            .send(WorkerRequest::Update {
                state: state.clone(),
                interval,
            })
            .is_err()
        {
            return crate::update::Update::Noop;
        }
        match self.response_rx.lock().unwrap().recv() {
            Ok(WorkerResponse::Update(u)) => u,
            Err(_) => crate::update::Update::Noop,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ── ParallelProtocol: dispatches addresses with protocol == "parallel" ──

/// The `parallel` protocol. Looks up the underlying class in the
/// local process registry, instantiates it, then wraps the result in
/// a [`ParallelProcess`].
///
/// For now Steps are returned unchanged (they're reactive — no
/// inherent benefit to threading the trigger).
#[derive(Debug, Default)]
pub struct ParallelProtocol;

impl Protocol for ParallelProtocol {
    fn name(&self) -> &str {
        "parallel"
    }

    fn instantiate(
        &self,
        data: &Value,
        config: Value,
        registry: &Arc<ProcessRegistry>,
    ) -> Result<ProcessNode, ProtocolError> {
        let class_name = data.as_str().ok_or_else(|| {
            ProtocolError::MalformedAddress(format!(
                "parallel protocol expects data: String, got {data:?}"
            ))
        })?;
        let node = registry.create(class_name, config).ok_or_else(|| {
            ProtocolError::UnknownClass(class_name.to_string(), "parallel".into())
        })?;
        match node {
            ProcessNode::Process(p) => {
                let parallel = ParallelProcess::spawn(p, class_name.to_string());
                Ok(ProcessNode::Process(Box::new(parallel)))
            }
            // Steps are state-change-triggered and lightweight — leave
            // on the orchestrator thread for now. A future
            // `ParallelStep` wrapper could be added symmetrically.
            ProcessNode::Step(s) => Ok(ProcessNode::Step(s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::PortSchema;
    use crate::protocol::{ParsedAddress, ProtocolRegistry};
    use crate::update::Update;
    use indexmap::IndexMap;
    use prism_schema::Schema;

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
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    fn make_registry() -> Arc<ProcessRegistry> {
        let mut r = ProcessRegistry::new();
        r.register("Double", |_| ProcessNode::Process(Box::new(DoubleProcess)));
        Arc::new(r)
    }

    #[test]
    fn parallel_protocol_round_trip() {
        let registry = make_registry();
        let protocols = {
            let mut p = ProtocolRegistry::new();
            p.register(Arc::new(ParallelProtocol::default()));
            p
        };
        let addr =
            ParsedAddress::parse(&Value::String("parallel:Double".into())).unwrap();
        let node = protocols.instantiate(&addr, Value::None, &registry).unwrap();
        let proc = match node {
            ProcessNode::Process(p) => p,
            _ => panic!("expected Process"),
        };

        let state = Value::tree([("x", Value::float(3.0))]);
        let upd = proc.update(&state, 1.0).into_value().unwrap();
        let x = upd.as_map().unwrap().get("x").unwrap().as_f64().unwrap();
        assert!((x - 6.0).abs() < 1e-9);
    }

    #[test]
    fn parallel_protocol_concurrent_updates() {
        // Two ParallelProcess instances run on independent worker
        // threads — their updates can happen concurrently from
        // different driver threads.
        let registry = make_registry();
        let protocols = {
            let mut p = ProtocolRegistry::new();
            p.register(Arc::new(ParallelProtocol::default()));
            p
        };
        let addr =
            ParsedAddress::parse(&Value::String("parallel:Double".into())).unwrap();
        let a = protocols
            .instantiate(&addr, Value::None, &registry)
            .unwrap();
        let b = protocols
            .instantiate(&addr, Value::None, &registry)
            .unwrap();

        let a = match a {
            ProcessNode::Process(p) => Arc::new(p),
            _ => panic!(),
        };
        let b = match b {
            ProcessNode::Process(p) => Arc::new(p),
            _ => panic!(),
        };

        let a_clone = Arc::clone(&a);
        let b_clone = Arc::clone(&b);
        let h1 = std::thread::spawn(move || {
            let s = Value::tree([("x", Value::float(2.0))]);
            a_clone.update(&s, 1.0)
        });
        let h2 = std::thread::spawn(move || {
            let s = Value::tree([("x", Value::float(5.0))]);
            b_clone.update(&s, 1.0)
        });

        let r1 = h1
            .join()
            .unwrap()
            .into_value()
            .unwrap()
            .as_map()
            .unwrap()
            .get("x")
            .unwrap()
            .as_f64()
            .unwrap();
        let r2 = h2
            .join()
            .unwrap()
            .into_value()
            .unwrap()
            .as_map()
            .unwrap()
            .get("x")
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((r1 - 4.0).abs() < 1e-9);
        assert!((r2 - 10.0).abs() < 1e-9);
    }

    #[test]
    fn parallel_drop_shuts_down_worker() {
        let registry = make_registry();
        let protocols = {
            let mut p = ProtocolRegistry::new();
            p.register(Arc::new(ParallelProtocol::default()));
            p
        };
        let addr =
            ParsedAddress::parse(&Value::String("parallel:Double".into())).unwrap();
        let node = protocols.instantiate(&addr, Value::None, &registry).unwrap();
        drop(node); // Drop should send Shutdown and join cleanly.
                    // If the worker hadn't been shut down properly the test
                    // would either hang or panic on drop.
    }
}

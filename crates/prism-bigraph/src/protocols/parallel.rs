//! `parallel` protocol — process `update()`s run on a shared CPU pool.
//!
//! Upstream's `process_bigraph.protocols.parallel` uses Python's
//! `multiprocessing` to put each process in its own OS process for true
//! parallelism (the GIL means threads don't parallelize CPU work). Rust
//! threads *do* parallelize, so the Rust analogue ships each `update()`
//! to a **shared worker pool** — no per-process thread, no GIL caveat.
//!
//! ## How it parallelizes (the seam, see docs/execution-model.md)
//!
//! A `ParallelProcess` overrides [`Process::invoke`]: instead of running
//! `update()` inline, it clones the input, submits `update()` to the
//! shared [`ParallelPool`], and returns a slot-[`Defer`] immediately.
//! The engine's run loop invokes **every** due process first (so all
//! their tasks are now in flight on the pool), then collects each
//! `Defer` — whose `.get()` blocks until that task fills its slot. So a
//! tick's `parallel:` processes run concurrently; the wall-clock is the
//! slowest, not the sum.
//!
//! Because `Process: Send + Sync`, the inner process is shared into a
//! pool task by `Arc`. The pool is created once per `ParallelProtocol`
//! and shared (`Arc`) into every process it instantiates; it also
//! implements [`ProtocolRuntime`], so the engine can flush it as an
//! explicit invoke→collect barrier (and the same registration path
//! serves a future batched `ray:` protocol).

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use prism_schema::Value;

use crate::defer::Defer;
use crate::factory::ProcessRegistry;
use crate::process::{Process, ProcessNode};
use crate::protocol::{Protocol, ProtocolError};
use crate::protocol_runtime::ProtocolRuntime;
use crate::update::Update;

// ── ParallelPool: a shared worker pool + invoke→collect barrier ──────

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Outstanding-task counter with a wait-for-zero barrier. `flush_pending`
/// blocks on this until every task submitted in the current tick is done.
#[derive(Default)]
struct Pending {
    count: Mutex<usize>,
    empty: Condvar,
}

impl Pending {
    fn add(&self) {
        *self.count.lock().unwrap() += 1;
    }
    fn done(&self) {
        let mut c = self.count.lock().unwrap();
        *c -= 1;
        if *c == 0 {
            self.empty.notify_all();
        }
    }
    fn wait_empty(&self) {
        let mut c = self.count.lock().unwrap();
        while *c > 0 {
            c = self.empty.wait(c).unwrap();
        }
    }
}

/// Shared CPU thread pool backing the `parallel` protocol. Process
/// `invoke`s submit their `update()` here; the pool runs them
/// concurrently and fills each task's slot. Created once per
/// [`ParallelProtocol`] and shared (`Arc`) into every process it makes.
pub struct ParallelPool {
    /// `Option` so `Drop` can drop the sender (closing the queue) before
    /// joining workers. `None` only during teardown.
    job_tx: Option<Sender<Job>>,
    pending: Arc<Pending>,
    workers: Vec<JoinHandle<()>>,
}

impl ParallelPool {
    /// Spawn a pool with `workers` threads (clamped to ≥ 1).
    pub fn new(workers: usize) -> Self {
        let workers = workers.max(1);
        let (job_tx, job_rx) = channel::<Job>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let pending = Arc::new(Pending::default());

        let handles = (0..workers)
            .map(|i| Self::spawn_worker(i, Arc::clone(&job_rx)))
            .collect();

        Self {
            job_tx: Some(job_tx),
            pending,
            workers: handles,
        }
    }

    /// A pool sized to the machine's parallelism (fallback 4).
    pub fn default_sized() -> Self {
        let n = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self::new(n)
    }

    fn spawn_worker(i: usize, job_rx: Arc<Mutex<Receiver<Job>>>) -> JoinHandle<()> {
        std::thread::Builder::new()
            .name(format!("parallel-pool-{i}"))
            .spawn(move || {
                loop {
                    // Hold the lock only for the dequeue; the guard is a
                    // temporary dropped at the `;`, so the job runs UNLOCKED
                    // and workers execute concurrently.
                    let job = job_rx.lock().unwrap().recv();
                    match job {
                        Ok(job) => job(),
                        Err(_) => break, // all senders dropped → shut down
                    }
                }
            })
            .expect("spawn parallel-pool worker")
    }

    /// Submit `f` to run on a worker thread. Returns immediately; the
    /// caller awaits the result through the slot it filled.
    fn submit<F: FnOnce() + Send + 'static>(&self, f: F) {
        self.pending.add();
        let pending = Arc::clone(&self.pending);
        let job: Job = Box::new(move || {
            f();
            pending.done();
        });
        if let Some(tx) = &self.job_tx {
            // Workers outlive every submit (dropped only in our `Drop`),
            // so this fails only if the pool is already torn down.
            let _ = tx.send(job);
        }
    }
}

impl ProtocolRuntime for ParallelPool {
    /// Wait for the current tick's submitted tasks to finish — the
    /// explicit invoke→collect barrier. (Slot `Defer`s also block on
    /// `.get()`, so concurrency is correct with or without this flush;
    /// the barrier just makes "all done" explicit before collect.)
    fn flush_pending(&self) {
        self.pending.wait_empty();
    }
    fn label(&self) -> &str {
        "parallel"
    }
}

impl std::fmt::Debug for ParallelPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParallelPool")
            .field("workers", &self.workers.len())
            .finish_non_exhaustive()
    }
}

impl Drop for ParallelPool {
    fn drop(&mut self) {
        // Drop the sender so workers' recv() returns Err and they exit,
        // then join them for a clean, deterministic shutdown.
        self.job_tx.take();
        for h in self.workers.drain(..) {
            let _ = h.join();
        }
    }
}

// ── ParallelProcess: a Process that runs its update() on the pool ────

/// A [`Process`] that runs its inner process's `update()` on a shared
/// [`ParallelPool`]. `invoke` enqueues + returns a slot-`Defer`; the
/// engine collects all such Defers after the invoke pass, so every
/// `parallel:` process in a tick runs concurrently.
pub struct ParallelProcess {
    class_name: String,
    inner: Arc<dyn Process>,
    pool: Arc<ParallelPool>,
}

impl std::fmt::Debug for ParallelProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParallelProcess")
            .field("class", &self.class_name)
            .finish_non_exhaustive()
    }
}

impl ParallelProcess {
    pub fn new(inner: Arc<dyn Process>, pool: Arc<ParallelPool>, class_name: String) -> Self {
        Self {
            class_name,
            inner,
            pool,
        }
    }
}

impl Process for ParallelProcess {
    fn inputs(&self) -> crate::ports::PortSchema {
        self.inner.inputs()
    }
    fn outputs(&self) -> crate::ports::PortSchema {
        self.inner.outputs()
    }
    fn interval(&self) -> f64 {
        self.inner.interval()
    }

    /// Enqueue the inner `update()` on the shared pool and return a
    /// slot-`Defer`; the pool fills it when the task finishes. The
    /// engine resolves it in the collect pass (`.get()` blocks until
    /// then) — which is where parallelism becomes visible.
    fn invoke(&self, state: &Value, interval: f64) -> Defer<Update> {
        let (defer, slot) = Defer::slot();
        let inner = Arc::clone(&self.inner);
        let state = state.clone();
        self.pool.submit(move || {
            slot.fill(inner.update(&state, interval));
        });
        defer
    }

    /// Blocking convenience — submit and await. The engine uses `invoke`
    /// for concurrency; a direct `update()` caller still gets the result.
    fn update(&self, state: &Value, interval: f64) -> Update {
        self.invoke(state, interval).get()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ── ParallelProtocol: dispatches addresses with protocol == "parallel" ──

/// The `parallel` protocol. Looks up the underlying class in the local
/// process registry, instantiates it, and wraps it in a
/// [`ParallelProcess`] bound to this protocol's shared [`ParallelPool`].
///
/// Steps are returned unchanged (they're reactive — no inherent benefit
/// to threading the trigger).
pub struct ParallelProtocol {
    pool: Arc<ParallelPool>,
}

impl Default for ParallelProtocol {
    fn default() -> Self {
        Self {
            pool: Arc::new(ParallelPool::default_sized()),
        }
    }
}

impl std::fmt::Debug for ParallelProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParallelProtocol").finish_non_exhaustive()
    }
}

impl ParallelProtocol {
    /// A protocol whose pool has exactly `workers` threads.
    pub fn new(workers: usize) -> Self {
        Self {
            pool: Arc::new(ParallelPool::new(workers)),
        }
    }

    /// The shared pool — register this with the engine
    /// (`register_protocol_runtime`) so it is flushed between the invoke
    /// and collect passes (the explicit barrier).
    pub fn pool(&self) -> Arc<ParallelPool> {
        Arc::clone(&self.pool)
    }
}

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
                let inner: Arc<dyn Process> = Arc::from(p);
                let wrapped = ParallelProcess::new(inner, self.pool(), class_name.to_string());
                Ok(ProcessNode::Process(Box::new(wrapped)))
            }
            // Steps are state-change-triggered and lightweight — leave on
            // the orchestrator thread for now.
            ProcessNode::Step(s) => Ok(ProcessNode::Step(s)),
        }
    }

    /// The shared pool IS this protocol's batching runtime — the engine registers
    /// it (via the `Core`) and flushes it as the invoke→collect barrier.
    fn runtime(&self) -> Option<Arc<dyn ProtocolRuntime>> {
        Some(Arc::clone(&self.pool) as Arc<dyn ProtocolRuntime>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::PortSchema;
    use crate::protocol::{ParsedAddress, ProtocolRegistry};
    use indexmap::IndexMap;
    use prism_schema::Schema;
    use std::time::{Duration, Instant};

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

    fn parallel_registry() -> ProtocolRegistry {
        let mut p = ProtocolRegistry::new();
        p.register(Arc::new(ParallelProtocol::default()));
        p
    }

    #[test]
    fn parallel_protocol_round_trip() {
        let registry = make_registry();
        let protocols = parallel_registry();
        let addr = ParsedAddress::parse(&Value::String("parallel:Double".into())).unwrap();
        let node = protocols
            .instantiate(&addr, Value::None, &registry)
            .unwrap();
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
    fn parallel_invoke_returns_a_defer_resolved_later() {
        // invoke() must not block — it returns a Defer the pool fills.
        let registry = make_registry();
        let protocols = parallel_registry();
        let addr = ParsedAddress::parse(&Value::String("parallel:Double".into())).unwrap();
        let node = protocols
            .instantiate(&addr, Value::None, &registry)
            .unwrap();
        let proc = match node {
            ProcessNode::Process(p) => p,
            _ => panic!("expected Process"),
        };

        let defer = proc.invoke(&Value::tree([("x", Value::float(4.0))]), 1.0);
        let x = defer
            .get()
            .into_value()
            .unwrap()
            .as_map()
            .unwrap()
            .get("x")
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((x - 8.0).abs() < 1e-9);
    }

    #[test]
    fn pool_runs_submitted_tasks_concurrently() {
        // The core wall-clock proof: 4 tasks × 50ms on a 4-thread pool
        // finish in ~50ms (concurrent), not ~200ms (sequential). Sleeping
        // tasks parallelize even on a 1-core box (no CPU contention).
        let pool = ParallelPool::new(4);
        let start = Instant::now();
        let mut defers = Vec::new();
        for i in 0..4u32 {
            let (defer, slot) = Defer::<u32>::slot();
            pool.submit(move || {
                std::thread::sleep(Duration::from_millis(50));
                slot.fill(i * 10);
            });
            defers.push(defer);
        }
        pool.flush_pending(); // barrier: every task done, every slot filled
        let elapsed = start.elapsed();

        let mut got: Vec<u32> = defers.into_iter().map(|d| d.get()).collect();
        got.sort();
        assert_eq!(got, vec![0, 10, 20, 30]);
        assert!(
            elapsed < Duration::from_millis(150),
            "expected concurrency, but took {elapsed:?} (sequential would be ~200ms)"
        );
    }

    #[test]
    fn flush_pending_waits_for_all_tasks() {
        // After flush_pending returns, every slot must have been filled.
        let pool = ParallelPool::new(2);
        let done = Arc::new(Mutex::new(0u32));
        for _ in 0..6 {
            let done = Arc::clone(&done);
            pool.submit(move || {
                std::thread::sleep(Duration::from_millis(10));
                *done.lock().unwrap() += 1;
            });
        }
        pool.flush_pending();
        assert_eq!(*done.lock().unwrap(), 6);
    }

    #[test]
    fn parallel_drop_shuts_down_cleanly() {
        // Dropping the protocol (last pool owner) joins the workers; if
        // shutdown were broken this would hang or panic on drop.
        let registry = make_registry();
        let protocols = parallel_registry();
        let addr = ParsedAddress::parse(&Value::String("parallel:Double".into())).unwrap();
        let node = protocols
            .instantiate(&addr, Value::None, &registry)
            .unwrap();
        drop(node);
        drop(protocols); // last Arc<ParallelPool> → Drop joins workers
    }
}

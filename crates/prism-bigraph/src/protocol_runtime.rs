//! Protocol-level batching runtime.
//!
//! Most protocols dispatch one update call at a time (local, parallel,
//! basic REST). Some protocols — Ray with its sharded actor pools is
//! the motivating example — batch many per-process calls into one
//! cross-process RPC for efficiency. To work transparently with the
//! Composite lifecycle, those protocols register a [`ProtocolRuntime`]
//! handle with the engine; the engine calls
//! `flush_pending()` between the invoke pass and the collect phase.
//!
//! Synchronous protocols don't need this — they finish work during
//! `Process::invoke` itself and their `Defer`s are immediate.
//!
//! See `crates/prism-bigraph/src/defer.rs` for the per-Defer side of
//! the lifecycle.

use std::sync::Arc;

/// A protocol's per-tick batching runtime.
///
/// Implementers collate `Process::invoke` calls during the invoke pass
/// and resolve them in one batched operation when
/// [`ProtocolRuntime::flush_pending`] is called. The Composite /
/// Engine drives this between invoke and apply_updates.
///
/// Method `flush_pending` is intentionally `&self` (not `&mut self`)
/// so the runtime can be shared via `Arc` across the processes that
/// enqueue onto it. Internal mutation lives behind whatever
/// synchronization the protocol chooses (Mutex / channel / actor).
pub trait ProtocolRuntime: Send + Sync + std::fmt::Debug {
    /// Resolve all pending batched calls. Called once per tick. After
    /// this returns, every `Defer::slot()` issued by this runtime in
    /// the current tick must have been filled.
    fn flush_pending(&self);

    /// Optional shutdown — called when the engine releases the runtime
    /// (e.g. at end of run). Default no-op.
    fn close(&self) {}

    /// Human-readable runtime identifier for debug output.
    fn label(&self) -> &str;
}

/// Type-erased collection of registered protocol runtimes. Cheap to
/// clone — runtimes themselves live behind `Arc`.
#[derive(Default, Clone, Debug)]
pub struct ProtocolRuntimes {
    runtimes: Vec<Arc<dyn ProtocolRuntime>>,
}

impl ProtocolRuntimes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, rt: Arc<dyn ProtocolRuntime>) {
        self.runtimes.push(rt);
    }

    pub fn is_empty(&self) -> bool {
        self.runtimes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.runtimes.len()
    }

    /// Run flush on every registered runtime in registration order.
    /// Called by the orchestrator between the invoke pass and the
    /// collect phase.
    pub fn flush_all(&self) {
        for rt in &self.runtimes {
            rt.flush_pending();
        }
    }

    /// Tear down — calls `close()` on every runtime.
    pub fn close_all(&self) {
        for rt in &self.runtimes {
            rt.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct CounterRuntime {
        name: String,
        flushes: AtomicUsize,
        closes: AtomicUsize,
    }

    impl CounterRuntime {
        fn new(name: &str) -> Arc<Self> {
            Arc::new(Self {
                name: name.into(),
                flushes: AtomicUsize::new(0),
                closes: AtomicUsize::new(0),
            })
        }
    }

    impl ProtocolRuntime for CounterRuntime {
        fn flush_pending(&self) {
            self.flushes.fetch_add(1, Ordering::SeqCst);
        }
        fn close(&self) {
            self.closes.fetch_add(1, Ordering::SeqCst);
        }
        fn label(&self) -> &str {
            &self.name
        }
    }

    #[test]
    fn flush_all_iterates_registrations() {
        let rt1 = CounterRuntime::new("a");
        let rt2 = CounterRuntime::new("b");

        let mut runtimes = ProtocolRuntimes::new();
        runtimes.register(Arc::clone(&rt1) as Arc<dyn ProtocolRuntime>);
        runtimes.register(Arc::clone(&rt2) as Arc<dyn ProtocolRuntime>);

        assert_eq!(runtimes.len(), 2);
        runtimes.flush_all();
        runtimes.flush_all();

        assert_eq!(rt1.flushes.load(Ordering::SeqCst), 2);
        assert_eq!(rt2.flushes.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn empty_runtimes_flush_is_noop() {
        let runtimes = ProtocolRuntimes::new();
        runtimes.flush_all(); // shouldn't panic
        assert!(runtimes.is_empty());
    }

    #[test]
    fn close_all_calls_close() {
        let rt = CounterRuntime::new("x");
        let mut runtimes = ProtocolRuntimes::new();
        runtimes.register(Arc::clone(&rt) as Arc<dyn ProtocolRuntime>);
        runtimes.close_all();
        assert_eq!(rt.closes.load(Ordering::SeqCst), 1);
    }
}

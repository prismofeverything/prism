//! Deferred computation results.
//!
//! Upstream `process-bigraph` uses a "defer" pattern for the Composite
//! lifecycle: `process.invoke(state, interval)` returns a `Defer` that
//! resolves later via `.get()`. The pattern lets batching protocols
//! (Ray, Pool) enqueue per-process calls during the *invoke pass*, run
//! one batched RPC during the *flush phase*, then return per-process
//! results during the *collect phase*.
//!
//! For synchronous protocols (the default, local in-process execution)
//! `invoke()` computes immediately and the `Defer` is just a thin
//! wrapper around the value. Either way, the Composite's lifecycle
//! looks identical from the orchestrator's perspective.
//!
//! ## Lifecycle
//!
//! ```text
//! for each process:                            // invoke pass
//!     defers.push(process.invoke(state, dt))
//!
//! for each protocol runtime:                   // flush phase
//!     runtime.flush_pending()                  // batches resolve here
//!
//! for each defer:                              // collect phase
//!     updates.push(defer.get())
//!
//! reconcile_and_apply(updates)
//! ```
//!
//! Synchronous protocols see no observable difference — `invoke()`
//! captures the result; `get()` returns it; flush_pending() is a
//! no-op.

use std::sync::Mutex;

/// A deferred computation that resolves to a `T`. For synchronous
/// protocols, the value is captured at `invoke()` time and `.get()`
/// returns it immediately. For batching protocols (Ray, Pool),
/// `.get()` blocks until the protocol's runtime has flushed pending
/// work — typically called between the orchestrator's invoke pass and
/// apply_updates phase.
///
/// `Defer<T>` is `Send` so it can move between threads if needed, but
/// is single-use: `.get()` consumes it.
pub struct Defer<T: Send + 'static> {
    inner: DeferInner<T>,
}

enum DeferInner<T: Send + 'static> {
    /// Result is already known. Sync protocols + tests use this.
    Immediate(T),
    /// Result is fetched by calling a closure. Async protocols use this
    /// to capture a reference to their runtime's result table; the
    /// closure pulls the result by id once flush has populated it.
    Lazy(Box<dyn FnOnce() -> T + Send>),
    /// A slot whose value is written externally (by flush_pending);
    /// `.get()` reads it under the Mutex. Used when the runtime
    /// holds the Defer and needs to write into it.
    Slot(Mutex<Option<T>>),
}

impl<T: Send + 'static> Defer<T> {
    /// Wrap an already-computed value.
    pub fn immediate(value: T) -> Self {
        Self {
            inner: DeferInner::Immediate(value),
        }
    }

    /// Build a defer whose value is produced by `f` when `.get()` is
    /// called. The closure runs at most once.
    pub fn lazy<F>(f: F) -> Self
    where
        F: FnOnce() -> T + Send + 'static,
    {
        Self {
            inner: DeferInner::Lazy(Box::new(f)),
        }
    }

    /// Build a defer paired with a fill handle. The handle's
    /// `[DeferSlot::fill]` method writes the value into the slot;
    /// `.get()` blocks until then (it doesn't actually block — if no
    /// value has been written and the slot is read, returns the
    /// default-or-panic via the caller's choice). For batching
    /// protocols.
    pub fn slot() -> (Self, DeferSlot<T>) {
        use std::sync::Arc;
        let cell = Arc::new(Mutex::new(None));
        let cell_for_slot = Arc::clone(&cell);
        // The Defer takes ownership of the Mutex through an inner
        // closure-stored handle. Use Lazy to consume the cell.
        let defer = Self {
            inner: DeferInner::Lazy(Box::new(move || {
                let mut guard = cell.lock().unwrap();
                guard.take().expect(
                    "Defer::get() called before the slot was filled \
                     — did you forget to call flush_pending() on the \
                     owning protocol runtime?",
                )
            })),
        };
        (defer, DeferSlot { cell: cell_for_slot })
    }

    /// Resolve the deferred value. Consumes `self`.
    pub fn get(self) -> T {
        match self.inner {
            DeferInner::Immediate(v) => v,
            DeferInner::Lazy(f) => f(),
            DeferInner::Slot(m) => m
                .lock()
                .unwrap()
                .take()
                .expect("Defer::Slot read with no value written"),
        }
    }
}

impl<T: Send + 'static + std::fmt::Debug> std::fmt::Debug for Defer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Defer").finish_non_exhaustive()
    }
}

/// Companion to `Defer::slot()` — the handle a protocol runtime uses
/// to write the result of a batched call back into the Defer.
pub struct DeferSlot<T: Send + 'static> {
    cell: std::sync::Arc<Mutex<Option<T>>>,
}

impl<T: Send + 'static> std::fmt::Debug for DeferSlot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeferSlot").finish_non_exhaustive()
    }
}

impl<T: Send + 'static> DeferSlot<T> {
    /// Fill the slot with `value`. Called once per Defer by the
    /// protocol's flush_pending after the batched call returns.
    pub fn fill(self, value: T) {
        *self.cell.lock().unwrap() = Some(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_round_trip() {
        let d: Defer<i32> = Defer::immediate(42);
        assert_eq!(d.get(), 42);
    }

    #[test]
    fn lazy_runs_on_get() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter = std::sync::Arc::new(AtomicUsize::new(0));
        let c2 = std::sync::Arc::clone(&counter);
        let d: Defer<i32> = Defer::lazy(move || {
            c2.fetch_add(1, Ordering::SeqCst);
            7
        });
        assert_eq!(counter.load(Ordering::SeqCst), 0, "lazy must not run early");
        assert_eq!(d.get(), 7);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn slot_fill_then_get() {
        let (defer, slot): (Defer<String>, DeferSlot<String>) = Defer::slot();
        slot.fill("hello".to_string());
        assert_eq!(defer.get(), "hello");
    }

    #[test]
    #[should_panic(expected = "Defer::get() called before the slot was filled")]
    fn slot_get_before_fill_panics() {
        let (defer, _slot): (Defer<i32>, _) = Defer::slot();
        let _ = defer.get();
    }
}

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

use std::sync::mpsc::{SyncSender, sync_channel};

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
    /// Result is produced by a closure on `.get()`. Async transports use
    /// this to await a worker — the closure receives on a one-shot
    /// channel the worker fills (see [`Defer::slot`]); [`Defer::lazy`]
    /// uses it for a plain deferred computation. Runs at most once.
    Lazy(Box<dyn FnOnce() -> T + Send>),
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

    /// Build a defer paired with a one-shot fill handle. The handle's
    /// [`DeferSlot::fill`] writes the value; `.get()` **blocks** until
    /// then. This is the primitive async transports build on: a pool /
    /// REST / stream worker runs off-thread and fills the slot when it
    /// finishes, while the orchestrator's collect phase awaits via
    /// `.get()`. (A batching runtime may instead fill every slot eagerly
    /// in `flush_pending`, in which case `.get()` returns immediately.)
    pub fn slot() -> (Self, DeferSlot<T>) {
        // A one-shot channel is exactly a fillable, awaitable cell:
        // `fill` sends (buffer of 1 → never blocks); `.get()` receives.
        let (tx, rx) = sync_channel::<T>(1);
        let defer = Self {
            inner: DeferInner::Lazy(Box::new(move || {
                rx.recv().expect(
                    "Defer::get(): slot sender was dropped without fill \
                     — the worker/runtime never produced a result",
                )
            })),
        };
        (defer, DeferSlot { tx })
    }

    /// Resolve the deferred value. Consumes `self`. Blocks if backed by
    /// an unfilled [`Defer::slot`] (until its `DeferSlot::fill`).
    pub fn get(self) -> T {
        match self.inner {
            DeferInner::Immediate(v) => v,
            DeferInner::Lazy(f) => f(),
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
    tx: SyncSender<T>,
}

impl<T: Send + 'static> std::fmt::Debug for DeferSlot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeferSlot").finish_non_exhaustive()
    }
}

impl<T: Send + 'static> DeferSlot<T> {
    /// Fill the slot with `value`, unblocking the paired `Defer::get()`.
    /// Called once, from whatever thread produced the result (a pool
    /// worker, a REST/stream reader, or a batching runtime's flush).
    pub fn fill(self, value: T) {
        // Buffer of 1 → never blocks; ignore error if the Defer (and its
        // receiver) was already dropped — the result is no longer wanted.
        let _ = self.tx.send(value);
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
    fn slot_get_blocks_until_filled_from_another_thread() {
        let (defer, slot): (Defer<i32>, DeferSlot<i32>) = Defer::slot();
        let h = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            slot.fill(99);
        });
        // .get() must wait for the worker thread to fill — no panic.
        assert_eq!(defer.get(), 99);
        h.join().unwrap();
    }

    #[test]
    #[should_panic(expected = "slot sender was dropped without fill")]
    fn slot_get_panics_if_sender_dropped_without_fill() {
        let (defer, slot): (Defer<i32>, _) = Defer::slot();
        drop(slot); // dropped without fill → get() can't receive
        let _ = defer.get();
    }
}

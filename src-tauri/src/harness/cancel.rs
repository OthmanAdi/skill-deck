// @agent-context: CancelRegistry — cancellation + concurrency control for turns.
// Carried over from the deleted ai/cancel.rs, upgraded in two ways:
//
// 1. `CancelHandle` pairs the AtomicBool with a `tokio::sync::Notify`, so a
//    cancel can WAKE a turn that is parked on a hung provider stream. The old
//    flag-only design was only observed between stream items — a provider
//    that stopped sending items could never be cancelled.
// 2. `TurnGate` enforces one active turn per session. A second
//    `harness_chat_send` for the same session fails fast with a `busy:` error
//    (kind `busy` on the IPC boundary) instead of racing the first turn's
//    memory + session file. Queueing was rejected as the more complex option:
//    the frontend disables the composer while streaming, so a concurrent send
//    for the same session is always a bug or a double-click.
//
// Both registries evict their per-session entry when the turn ends (RAII via
// `TurnGuard`), so hundreds of sessions do not accumulate dead entries.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::Notify;

/// One session's cancel signal. Cheap to clone via `Arc`.
#[derive(Default)]
pub struct CancelHandle {
    flag: AtomicBool,
    notify: Notify,
}

impl CancelHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the flag and wake every future currently parked in `cancelled()`.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Resolves when the handle is cancelled. Safe against the classic
    /// notify race: interest is registered via `enable()` BEFORE the flag is
    /// re-checked, so a cancel that lands between check and await still wakes.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let mut notified = std::pin::pin!(self.notify.notified());
        notified.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

pub struct CancelRegistry {
    handles: Mutex<HashMap<String, Arc<CancelHandle>>>,
}

static INSTANCE: OnceLock<CancelRegistry> = OnceLock::new();

impl CancelRegistry {
    pub fn instance() -> &'static CancelRegistry {
        INSTANCE.get_or_init(|| CancelRegistry {
            handles: Mutex::new(HashMap::new()),
        })
    }

    /// Reset (or insert) a fresh handle for this session, ensuring a clean slate.
    pub fn reset(&self, session_id: &str) -> Arc<CancelHandle> {
        let handle = Arc::new(CancelHandle::new());
        let mut guard = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        guard.insert(session_id.to_string(), handle.clone());
        handle
    }

    pub fn cancel(&self, session_id: &str) {
        let guard = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(handle) = guard.get(session_id) {
            handle.cancel();
        }
    }

    pub fn is_cancelled(&self, session_id: &str) -> bool {
        let guard = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        guard
            .get(session_id)
            .map(|h| h.is_cancelled())
            .unwrap_or(false)
    }

    /// Drop the session's handle once its turn is over so the map stays
    /// bounded and a late cancel cannot poison the NEXT turn of the session.
    pub fn remove(&self, session_id: &str) {
        let mut guard = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        guard.remove(session_id);
    }
}

/// One-active-turn-per-session gate. `try_acquire` returns `None` while a
/// turn is running for that session; the returned `TurnGuard` releases the
/// slot (and the session's cancel handle) on drop — including on panic and
/// on cancellation, so a failed turn can never wedge the session.
pub struct TurnGate {
    active: Mutex<HashSet<String>>,
}

static GATE: OnceLock<TurnGate> = OnceLock::new();

impl TurnGate {
    pub fn instance() -> &'static TurnGate {
        GATE.get_or_init(|| TurnGate {
            active: Mutex::new(HashSet::new()),
        })
    }

    pub fn try_acquire(&'static self, session_id: &str) -> Option<TurnGuard> {
        let mut guard = self.active.lock().unwrap_or_else(|p| p.into_inner());
        if guard.contains(session_id) {
            return None;
        }
        guard.insert(session_id.to_string());
        Some(TurnGuard {
            gate: self,
            session_id: session_id.to_string(),
        })
    }

    pub fn is_active(&self, session_id: &str) -> bool {
        let guard = self.active.lock().unwrap_or_else(|p| p.into_inner());
        guard.contains(session_id)
    }

    fn release(&self, session_id: &str) {
        let mut guard = self.active.lock().unwrap_or_else(|p| p.into_inner());
        guard.remove(session_id);
    }
}

pub struct TurnGuard {
    gate: &'static TurnGate,
    session_id: String,
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        self.gate.release(&self.session_id);
        CancelRegistry::instance().remove(&self.session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn cancel_sets_flag_and_reset_clears_it() {
        let reg = CancelRegistry::instance();
        let handle = reg.reset("cancel-test-a");
        assert!(!handle.is_cancelled());
        reg.cancel("cancel-test-a");
        assert!(handle.is_cancelled());
        assert!(reg.is_cancelled("cancel-test-a"));
        // A fresh turn starts clean.
        let handle2 = reg.reset("cancel-test-a");
        assert!(!handle2.is_cancelled());
        // The first handle still remembers its own cancellation.
        assert!(handle.is_cancelled());
        reg.remove("cancel-test-a");
        assert!(!reg.is_cancelled("cancel-test-a"));
    }

    #[test]
    fn cancel_unknown_session_is_noop() {
        CancelRegistry::instance().cancel("cancel-test-never-registered");
        assert!(!CancelRegistry::instance().is_cancelled("cancel-test-never-registered"));
    }

    #[tokio::test]
    async fn cancelled_future_wakes_a_parked_waiter() {
        let handle = Arc::new(CancelHandle::new());
        let waiter = handle.clone();
        let task = tokio::spawn(async move {
            waiter.cancelled().await;
            true
        });
        // Give the task a beat to park on the notify.
        tokio::time::sleep(Duration::from_millis(10)).await;
        handle.cancel();
        let woke = tokio::time::timeout(Duration::from_millis(500), task)
            .await
            .expect("waiter should wake after cancel")
            .expect("task should not panic");
        assert!(woke);
    }

    #[tokio::test]
    async fn cancelled_future_returns_immediately_when_already_cancelled() {
        let handle = CancelHandle::new();
        handle.cancel();
        tokio::time::timeout(Duration::from_millis(100), handle.cancelled())
            .await
            .expect("already-cancelled handle must resolve instantly");
    }

    #[test]
    fn turn_gate_rejects_second_acquire_until_guard_drops() {
        let gate = TurnGate::instance();
        let guard = gate.try_acquire("gate-test-a").expect("first acquire");
        assert!(gate.is_active("gate-test-a"));
        assert!(
            gate.try_acquire("gate-test-a").is_none(),
            "second acquire must fail"
        );
        // A different session is unaffected.
        let other = gate
            .try_acquire("gate-test-b")
            .expect("other session acquires");
        drop(other);
        drop(guard);
        assert!(!gate.is_active("gate-test-a"));
        let again = gate.try_acquire("gate-test-a").expect("free after drop");
        drop(again);
    }

    #[test]
    fn turn_guard_drop_also_clears_cancel_handle() {
        let reg = CancelRegistry::instance();
        let gate = TurnGate::instance();
        let _handle = reg.reset("gate-test-c");
        let guard = gate.try_acquire("gate-test-c").expect("acquire");
        reg.cancel("gate-test-c");
        assert!(reg.is_cancelled("gate-test-c"));
        drop(guard);
        // Handle evicted: a late cancel query no longer reports true.
        assert!(!reg.is_cancelled("gate-test-c"));
    }
}

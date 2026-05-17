// @agent-context: Cooperative cancellation registry.
//
// `register(session_id)` returns an AtomicBool the running agent loop
// checks between iterations and inside its stream-chunk drain. The Stop
// button in the UI calls `cancel(session_id)` which sets the flag.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Default)]
pub struct CancelRegistry {
    inner: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl CancelRegistry {
    pub fn instance() -> &'static CancelRegistry {
        static INSTANCE: OnceLock<CancelRegistry> = OnceLock::new();
        INSTANCE.get_or_init(CancelRegistry::default)
    }

    /// Get (or create) a cancellation flag for `session_id`.
    pub fn flag_for(&self, session_id: &str) -> Arc<AtomicBool> {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    /// Mark the session as cancelled. Any subsequent `is_cancelled` returns true
    /// until the entry is reset by the next turn.
    pub fn cancel(&self, session_id: &str) {
        let guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(flag) = guard.get(session_id) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// Reset the flag at the start of a fresh turn so a previous cancel doesn't
    /// kill the new request.
    pub fn reset(&self, session_id: &str) -> Arc<AtomicBool> {
        let flag = self.flag_for(session_id);
        flag.store(false, Ordering::SeqCst);
        flag
    }

    /// Drop the entry when the session is removed.
    #[allow(dead_code)]
    pub fn forget(&self, session_id: &str) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.remove(session_id);
    }
}

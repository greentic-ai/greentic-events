use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Simple pluggable store for idempotency keys.
pub trait IdempotencyStore: Send + Sync {
    /// Returns true if the key was newly recorded. Returns false when a duplicate is detected.
    fn record_key(&self, key: &str) -> bool;
}

/// In-memory implementation used by default and in tests.
#[derive(Debug, Default)]
pub struct InMemoryIdempotencyStore {
    seen: Arc<Mutex<HashSet<String>>>,
}

impl InMemoryIdempotencyStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl IdempotencyStore for InMemoryIdempotencyStore {
    fn record_key(&self, key: &str) -> bool {
        let mut guard = self.seen.lock().expect("poisoned idempotency store");
        guard.insert(key.to_string())
    }
}

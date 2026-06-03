use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// A single readiness probe: returns `Ok(())` when the dependency it guards is
/// healthy, or `Err(reason)` when it is not.
pub type CheckFn = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

/// Tracks liveness and readiness. Liveness ("am I running?") is a flat 200 once
/// the process is up. Readiness ("can I serve traffic?") runs every registered
/// check and also honours an explicit ready flag the server flips during
/// startup and graceful shutdown. Cheap to [`Clone`] — all state is shared.
#[derive(Clone, Default)]
pub struct Health {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    ready: AtomicBool,
    checks: RwLock<BTreeMap<String, CheckFn>>,
}

impl Health {
    /// An empty registry with the readiness gate closed (reports 503 until
    /// [`Health::set_ready`] opens it).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or replace) a named readiness check.
    ///
    /// # Panics
    /// Panics only if the internal lock is poisoned (another thread panicked
    /// while holding it) — not reachable in normal operation.
    pub fn register<F>(&self, name: &str, check: F)
    where
        F: Fn() -> Result<(), String> + Send + Sync + 'static,
    {
        self.inner
            .checks
            .write()
            .expect("health lock not poisoned")
            .insert(name.to_owned(), Arc::new(check));
    }

    /// Flip the readiness gate. The server sets it `true` once listening and
    /// `false` at the start of shutdown so load balancers drain cleanly.
    pub fn set_ready(&self, ready: bool) {
        self.inner.ready.store(ready, Ordering::SeqCst);
    }

    /// Whether the readiness gate is currently open.
    pub fn is_ready(&self) -> bool {
        self.inner.ready.load(Ordering::SeqCst)
    }

    /// Run every registered check. Returns `(all_ok, per_check_results)` with a
    /// deterministic (sorted) result map.
    ///
    /// # Panics
    /// Panics only if the internal lock is poisoned (another thread panicked
    /// while holding it) — not reachable in normal operation.
    pub fn run_checks(&self) -> (bool, BTreeMap<String, String>) {
        let checks = self.inner.checks.read().expect("health lock not poisoned");
        let mut all_ok = true;
        let mut results = BTreeMap::new();
        for (name, check) in checks.iter() {
            match check() {
                Ok(()) => {
                    results.insert(name.clone(), "ok".to_owned());
                }
                Err(reason) => {
                    all_ok = false;
                    results.insert(name.clone(), reason);
                }
            }
        }
        (all_ok, results)
    }
}

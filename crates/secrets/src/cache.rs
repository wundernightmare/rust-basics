use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use secrecy::SecretString;

/// A lock-free, hot-swappable cache of named secrets.
///
/// Reads are wait-free (an `Arc` load); writes publish a new snapshot via an
/// atomic swap (the same copy-on-write pattern a config handle uses). Cheap to
/// share behind an `Arc`.
#[derive(Default)]
pub struct SecretCache {
    inner: ArcSwap<HashMap<String, Arc<SecretString>>>,
}

impl SecretCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a secret by name. The returned `Arc` is a cheap clone of the
    /// cached value; the underlying string stays redacted in logs.
    pub fn get(&self, name: &str) -> Option<Arc<SecretString>> {
        self.inner.load().get(name).cloned()
    }

    /// Number of cached secrets.
    pub fn len(&self) -> usize {
        self.inner.load().len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.load().is_empty()
    }

    /// Replace the entire cache atomically (e.g. after reloading a snapshot).
    pub fn replace(&self, secrets: HashMap<String, Arc<SecretString>>) {
        self.inner.store(Arc::new(secrets));
    }

    /// Insert (or overwrite) one secret via copy-on-write swap.
    pub fn insert(&self, name: impl Into<String>, secret: SecretString) {
        let mut next = HashMap::clone(&self.inner.load());
        next.insert(name.into(), Arc::new(secret));
        self.inner.store(Arc::new(next));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn insert_get_and_replace() {
        let cache = SecretCache::new();
        assert!(cache.is_empty());

        cache.insert("api_key", SecretString::from("abc123".to_owned()));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("api_key").unwrap().expose_secret(), "abc123");
        assert!(cache.get("missing").is_none());

        let mut fresh = HashMap::new();
        fresh.insert(
            "other".to_owned(),
            Arc::new(SecretString::from("z".to_owned())),
        );
        cache.replace(fresh);
        assert!(
            cache.get("api_key").is_none(),
            "replace swapped the whole map"
        );
        assert_eq!(cache.get("other").unwrap().expose_secret(), "z");
    }
}

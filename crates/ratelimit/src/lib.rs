//! `ratelimit` — a keyed GCRA rate limiter built on
//! [`governor`](https://docs.rs/governor).
//!
//! GCRA (the Generic Cell Rate Algorithm) smooths bursts to a sustained rate
//! using lock-free atomics — no background sweep, no mutex. In tracehub-edge
//! this lives inside `resilient-http-client`; here it's a standalone, generic
//! crate keyed by anything `Hash + Eq + Clone` (an IP, an API token, a user id).
//!
//! ```
//! use ratelimit::Limiter;
//!
//! let limiter: Limiter<&str> = Limiter::per_second(2);
//! assert!(limiter.check(&"alice").is_ok());   // 1st — allowed
//! assert!(limiter.check(&"alice").is_ok());   // 2nd — allowed (burst of 2)
//! assert!(limiter.check(&"alice").is_err());  // 3rd — limited
//! assert!(limiter.check(&"bob").is_ok());     // separate key, own budget
//! ```

#![allow(clippy::must_use_candidate, clippy::missing_errors_doc)]

use std::hash::Hash;
use std::num::NonZeroU32;

use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use thiserror::Error;

pub use governor::Quota as GovernorQuota;

/// Returned by [`Limiter::check`] when the key has exhausted its budget.
#[derive(Debug, Clone, Copy, Error)]
#[error("rate limited")]
pub struct RateLimited;

/// A keyed GCRA rate limiter. Cheap to share behind an `Arc`; all state is
/// lock-free.
pub struct Limiter<K>
where
    K: Hash + Eq + Clone,
{
    inner: DefaultKeyedRateLimiter<K>,
}

impl<K> Limiter<K>
where
    K: Hash + Eq + Clone,
{
    /// A limiter allowing `n` requests per second per key (burst up to `n`).
    /// `n` is clamped to ≥ 1.
    pub fn per_second(n: u32) -> Self {
        Self::with_quota(Quota::per_second(nonzero(n)))
    }

    /// A limiter allowing `n` requests per minute per key. `n` is clamped to ≥ 1.
    pub fn per_minute(n: u32) -> Self {
        Self::with_quota(Quota::per_minute(nonzero(n)))
    }

    /// Build from a fully-specified [`governor::Quota`] (e.g. with a custom
    /// burst via [`governor::Quota::allow_burst`]).
    pub fn with_quota(quota: Quota) -> Self {
        Self {
            inner: RateLimiter::keyed(quota),
        }
    }

    /// Non-blocking check: `Ok(())` if a cell is available for `key` right now,
    /// otherwise [`RateLimited`].
    pub fn check(&self, key: &K) -> Result<(), RateLimited> {
        self.inner.check_key(key).map_err(|_| RateLimited)
    }

    /// Async: wait until `key` has an available cell, then return. Use this to
    /// shape (throttle) rather than reject.
    pub async fn until_ready(&self, key: &K) {
        self.inner.until_key_ready(key).await;
    }

    /// Drop rate-limiting state for keys that have fully recovered their
    /// budget. Call periodically for unbounded key spaces to reclaim memory.
    pub fn retain_recent(&self) {
        self.inner.retain_recent();
    }
}

fn nonzero(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n.max(1)).expect("max(1) is non-zero")
}

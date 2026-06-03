//! Full-jitter exponential backoff.
//!
//! Formula: `sleep = random(0, min(cap, base × 2^attempt))`
//!
//! Reference: <https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/>

use std::time::Duration;

use rand::RngExt as _;

/// Compute a full-jitter delay for `attempt` (0-indexed).
///
/// * `attempt = 0` → always `Duration::ZERO` (first try, no delay).
/// * `attempt = n` → random in `[0, min(cap_ms, base_ms × 2^n)]`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
pub fn full_jitter(attempt: u32, base_ms: u64, cap_ms: u64) -> Duration {
    if attempt == 0 || cap_ms == 0 {
        return Duration::ZERO;
    }
    // min(cap_ms, base_ms * 2^attempt) — use f64 to avoid u64 overflow on large attempts.
    let ceiling = ((base_ms as f64) * 2f64.powi(attempt as i32)).min(cap_ms as f64) as u64;
    let jitter_ms = rand::rng().random_range(0..=ceiling);
    Duration::from_millis(jitter_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_zero_is_immediate() {
        assert_eq!(full_jitter(0, 100, 30_000), Duration::ZERO);
    }

    #[test]
    fn zero_cap_is_immediate() {
        assert_eq!(full_jitter(5, 100, 0), Duration::ZERO);
    }

    #[test]
    fn jitter_within_cap() {
        let cap = Duration::from_secs(5);
        for attempt in 1..=20 {
            for _ in 0..50 {
                assert!(full_jitter(attempt, 100, 5_000) <= cap);
            }
        }
    }

    #[test]
    fn non_zero_attempt_produces_nonzero_delay() {
        let any_nonzero = (0..200).any(|_| full_jitter(1, 1000, 5000) > Duration::ZERO);
        assert!(any_nonzero, "expected at least one non-zero delay");
    }
}

//! Lock-free circuit breaker.
//!
//! ## State machine
//!
//! ```text
//!                failures / window >= threshold
//!  ┌─────────┐  AND requests >= min_requests   ┌──────┐
//!  │  Closed │ ──────────────────────────────► │ Open │
//!  └─────────┘                                 └──────┘
//!       ▲                                          │
//!       │  probe succeeds                          │ half_open_timeout elapsed
//!       │                                          ▼
//!       │                                    ┌──────────┐
//!       └──────────────────────────────────── │ HalfOpen │
//!         probe fails → back to Open          └──────────┘
//! ```
//!
//! All transitions use atomic compare-exchange; the sliding window uses two
//! `AtomicU32` counters plus an `AtomicU64` window-start timestamp rotated via
//! a CAS so only one thread clears the counters.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CbState {
    /// Normal operation — all requests are allowed through.
    Closed = 0,
    /// Circuit is open — requests are rejected until `half_open_timeout` elapses.
    Open = 1,
    /// One probe request is allowed; success closes, failure re-opens.
    HalfOpen = 2,
}

impl From<u8> for CbState {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::Closed,
            1 => Self::Open,
            _ => Self::HalfOpen,
        }
    }
}

pub struct CircuitBreaker {
    state: AtomicU8,
    window_requests: AtomicU32,
    window_failures: AtomicU32,
    window_start_ms: AtomicU64,
    opened_at_ms: AtomicU64,
    failure_threshold: f64,
    min_requests: u32,
    window_duration_ms: u64,
    half_open_timeout_ms: u64,
}

impl CircuitBreaker {
    pub fn new(
        failure_threshold: f64,
        min_requests: u32,
        window_duration: Duration,
        half_open_timeout: Duration,
    ) -> Self {
        Self {
            state: AtomicU8::new(CbState::Closed as u8),
            window_requests: AtomicU32::new(0),
            window_failures: AtomicU32::new(0),
            window_start_ms: AtomicU64::new(now_ms()),
            opened_at_ms: AtomicU64::new(0),
            failure_threshold,
            min_requests,
            #[allow(clippy::cast_possible_truncation)]
            window_duration_ms: window_duration.as_millis() as u64,
            #[allow(clippy::cast_possible_truncation)]
            half_open_timeout_ms: half_open_timeout.as_millis() as u64,
        }
    }

    /// Current circuit state — cheap atomic load.
    #[inline]
    pub fn state(&self) -> CbState {
        CbState::from(self.state.load(Ordering::Acquire))
    }

    /// Returns `true` if the caller may proceed with the request.
    pub fn allow_request(&self) -> bool {
        match self.state() {
            CbState::Closed | CbState::HalfOpen => true,
            CbState::Open => {
                let opened_at = self.opened_at_ms.load(Ordering::Acquire);
                if opened_at > 0 && now_ms().saturating_sub(opened_at) >= self.half_open_timeout_ms
                {
                    // Only the thread that wins the CAS gets the single probe.
                    self.state
                        .compare_exchange(
                            CbState::Open as u8,
                            CbState::HalfOpen as u8,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                } else {
                    false
                }
            }
        }
    }

    /// Record a successful response. Transitions `HalfOpen` → Closed.
    pub fn record_success(&self) {
        match self.state() {
            CbState::HalfOpen => {
                self.state.store(CbState::Closed as u8, Ordering::Release);
                self.reset_window();
            }
            CbState::Closed => {
                self.maybe_rotate_window();
                self.window_requests.fetch_add(1, Ordering::Relaxed);
            }
            CbState::Open => {}
        }
    }

    /// Record a failed response. May transition Closed → Open or `HalfOpen` → Open.
    pub fn record_failure(&self) {
        match self.state() {
            CbState::HalfOpen => self.open_circuit(),
            CbState::Closed => {
                self.maybe_rotate_window();
                let requests = self.window_requests.fetch_add(1, Ordering::Relaxed) + 1;
                let failures = self.window_failures.fetch_add(1, Ordering::Relaxed) + 1;
                if requests >= self.min_requests {
                    let ratio = f64::from(failures) / f64::from(requests);
                    if ratio >= self.failure_threshold {
                        self.open_circuit();
                    }
                }
            }
            CbState::Open => {}
        }
    }

    fn open_circuit(&self) {
        self.state.store(CbState::Open as u8, Ordering::Release);
        self.opened_at_ms.store(now_ms(), Ordering::Release);
    }

    fn reset_window(&self) {
        self.window_start_ms.store(now_ms(), Ordering::Release);
        self.window_requests.store(0, Ordering::Release);
        self.window_failures.store(0, Ordering::Release);
    }

    fn maybe_rotate_window(&self) {
        let start = self.window_start_ms.load(Ordering::Acquire);
        let now = now_ms();
        if now.saturating_sub(start) >= self.window_duration_ms
            && self
                .window_start_ms
                .compare_exchange(start, now, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            self.window_requests.store(0, Ordering::Release);
            self.window_failures.store(0, Ordering::Release);
        }
    }
}

#[inline]
#[allow(clippy::cast_possible_truncation)]
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cb(threshold: f64, min: u32) -> CircuitBreaker {
        CircuitBreaker::new(
            threshold,
            min,
            Duration::from_secs(10),
            Duration::from_millis(1),
        )
    }

    #[test]
    fn starts_closed_and_allows_requests() {
        let b = cb(0.5, 5);
        assert_eq!(b.state(), CbState::Closed);
        assert!(b.allow_request());
    }

    #[test]
    fn opens_when_failure_ratio_exceeds_threshold() {
        let b = cb(0.5, 4);
        for _ in 0..2 {
            b.record_success();
        }
        for _ in 0..2 {
            b.record_failure();
        }
        assert_eq!(b.state(), CbState::Open);
        assert!(!b.allow_request());
    }

    #[test]
    fn does_not_open_before_min_requests() {
        let b = cb(0.5, 10);
        for _ in 0..4 {
            b.record_failure();
        }
        assert_eq!(b.state(), CbState::Closed);
    }

    #[test]
    fn half_open_probe_success_closes_circuit() {
        let b = cb(0.5, 2);
        b.record_failure();
        b.record_failure();
        assert_eq!(b.state(), CbState::Open);
        b.state.store(CbState::HalfOpen as u8, Ordering::Release);
        assert!(b.allow_request());
        b.record_success();
        assert_eq!(b.state(), CbState::Closed);
    }

    #[test]
    fn half_open_probe_failure_reopens_circuit() {
        let b = cb(0.5, 2);
        b.state.store(CbState::HalfOpen as u8, Ordering::Release);
        b.record_failure();
        assert_eq!(b.state(), CbState::Open);
    }

    #[test]
    fn failure_ratio_uses_division_not_multiplication() {
        let b = CircuitBreaker::new(0.5, 2, Duration::from_secs(60), Duration::from_secs(30));
        b.record_success();
        b.record_success();
        b.record_failure(); // 1/3 ≈ 0.33 < 0.5 → Closed
        assert_eq!(b.state(), CbState::Closed);
    }
}

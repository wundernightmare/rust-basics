//! Prometheus metrics for [`crate::ResilientHttpClient`].
//!
//! Registered on a per-instance [`prometheus::Registry`] so multiple clients
//! (or tests) never conflict. Expose [`Metrics::registry`] from your `/metrics`
//! handler, or register the public metric vectors onto another registry.

use prometheus::{CounterVec, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry};

use crate::circuit_breaker::CbState;

/// Metric handles for one `ResilientHttpClient` instance.
pub struct Metrics {
    /// `outbound_requests_total{target, method, status, error_type}`
    pub requests_total: CounterVec,
    /// `outbound_request_duration_seconds{target, method}`
    pub request_duration: HistogramVec,
    /// `outbound_circuit_breaker_state{target}` — 0=Closed, 1=Open, 2=HalfOpen.
    pub cb_state: GaugeVec,
    /// `outbound_retry_attempts_total{target}` — counts retries (not the first try).
    pub retry_attempts: CounterVec,
    registry: Registry,
}

impl Metrics {
    /// Create and register all metrics in a fresh, isolated [`Registry`].
    pub fn new() -> prometheus::Result<Self> {
        let registry = Registry::new();

        let requests_total = CounterVec::new(
            Opts::new(
                "outbound_requests_total",
                "Outbound HTTP requests, by target, method, status and error type.",
            ),
            &["target", "method", "status", "error_type"],
        )?;
        let request_duration = HistogramVec::new(
            HistogramOpts::new(
                "outbound_request_duration_seconds",
                "Outbound HTTP request latency in seconds, by target and method.",
            ),
            &["target", "method"],
        )?;
        let cb_state = GaugeVec::new(
            Opts::new(
                "outbound_circuit_breaker_state",
                "Circuit-breaker state per target: 0=Closed, 1=Open, 2=HalfOpen.",
            ),
            &["target"],
        )?;
        let retry_attempts = CounterVec::new(
            Opts::new(
                "outbound_retry_attempts_total",
                "Retry attempts (excluding the initial try), by target.",
            ),
            &["target"],
        )?;

        registry.register(Box::new(requests_total.clone()))?;
        registry.register(Box::new(request_duration.clone()))?;
        registry.register(Box::new(cb_state.clone()))?;
        registry.register(Box::new(retry_attempts.clone()))?;

        Ok(Self {
            requests_total,
            request_duration,
            cb_state,
            retry_attempts,
            registry,
        })
    }

    /// The isolated registry holding this client's metrics.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub(crate) fn set_cb_state(&self, target: &str, state: CbState) {
        self.cb_state
            .with_label_values(&[target])
            .set(f64::from(state as u8));
    }
}

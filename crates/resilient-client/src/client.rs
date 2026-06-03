//! Core [`ResilientHttpClient`] implementation.

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use dashmap::DashMap;
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use reqwest::{header::HeaderMap, Client, Method, Response};
use tokio::sync::Notify;

use crate::backoff;
use crate::circuit_breaker::CircuitBreaker;
use crate::config::{ClientConfig, OutboundTargetConfig};
use crate::error::{OutboundError, ShutdownError};
use crate::metrics::Metrics;

// ── ResourceGroup ─────────────────────────────────────────────────────────────

/// Identifies the outbound target for a request. The inner string matches an
/// `outbound_targets[].name` in [`ClientConfig`]; unknown names get a fallback
/// policy with default settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceGroup(String);

impl ResourceGroup {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_key(&self) -> &str {
        &self.0
    }
}

// ── OutboundRequest ───────────────────────────────────────────────────────────

/// A request to send through the resilient client.
#[derive(Debug, Clone)]
pub struct OutboundRequest {
    pub target: ResourceGroup,
    pub method: Method,
    pub url: String,
    pub headers: HeaderMap,
    pub body: Option<Bytes>,
}

impl OutboundRequest {
    pub fn new(target: ResourceGroup, method: Method, url: impl Into<String>) -> Self {
        Self {
            target,
            method,
            url: url.into(),
            headers: HeaderMap::new(),
            body: None,
        }
    }

    /// Convenience constructor for a `GET`.
    pub fn get(target: ResourceGroup, url: impl Into<String>) -> Self {
        Self::new(target, Method::GET, url)
    }

    #[must_use]
    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }

    #[must_use]
    pub fn with_body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = Some(body.into());
        self
    }
}

// ── PolicySet ─────────────────────────────────────────────────────────────────

struct PolicySet {
    rate_limiter: DefaultDirectRateLimiter,
    circuit_breaker: CircuitBreaker,
    config: OutboundTargetConfig,
}

impl PolicySet {
    fn from_config(cfg: OutboundTargetConfig) -> Self {
        let rps = NonZeroU32::new(cfg.rate_limit.max(1)).expect("max(1) is non-zero");
        Self {
            rate_limiter: RateLimiter::direct(Quota::per_second(rps)),
            circuit_breaker: CircuitBreaker::new(
                cfg.cb_threshold,
                cfg.cb_min_requests,
                Duration::from_secs(cfg.cb_window_secs),
                Duration::from_secs(cfg.cb_half_open_timeout_secs),
            ),
            config: cfg,
        }
    }
}

enum ExecErr {
    Timeout,
    Connect(String),
    Other(String),
}

// ── ResilientHttpClient ───────────────────────────────────────────────────────

/// A shared reqwest connection pool fronted by per-target resilience policy.
pub struct ResilientHttpClient {
    client: Client,
    policies: DashMap<String, Arc<PolicySet>>,
    default_timeout: Duration,
    metrics: Arc<Metrics>,
    is_shutting_down: AtomicBool,
    in_flight: AtomicUsize,
    all_done: Notify,
}

impl ResilientHttpClient {
    /// Build a client from `config`, recording into `metrics`.
    pub fn new(config: ClientConfig, metrics: Arc<Metrics>) -> Result<Self, OutboundError> {
        let mut builder = Client::builder()
            .pool_max_idle_per_host(config.pool_max_idle_per_host)
            .pool_idle_timeout(Duration::from_secs(config.pool_idle_timeout_secs));
        if config.tcp_keepalive_secs > 0 {
            builder = builder.tcp_keepalive(Duration::from_secs(config.tcp_keepalive_secs));
        }
        if let Some(ua) = &config.user_agent {
            builder = builder.user_agent(ua);
        }
        let client = builder
            .build()
            .map_err(|e| OutboundError::Fatal(format!("build http client: {e}")))?;

        let policies = DashMap::new();
        for target in config.outbound_targets {
            policies.insert(
                target.name.clone(),
                Arc::new(PolicySet::from_config(target)),
            );
        }

        Ok(Self {
            client,
            policies,
            default_timeout: Duration::from_millis(config.default_timeout_ms),
            metrics,
            is_shutting_down: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
            all_done: Notify::new(),
        })
    }

    /// The metrics handle (e.g. to expose its registry on `/metrics`).
    pub fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }

    fn policy_for(&self, key: &str) -> Arc<PolicySet> {
        if let Some(p) = self.policies.get(key) {
            return Arc::clone(p.value());
        }
        let entry = self.policies.entry(key.to_owned()).or_insert_with(|| {
            Arc::new(PolicySet::from_config(OutboundTargetConfig::fallback(key)))
        });
        Arc::clone(entry.value())
    }

    /// Send one request through the policy stack (rate limit → circuit breaker
    /// → timed request → classify). Does **not** retry.
    pub async fn send(&self, req: OutboundRequest) -> Result<Response, OutboundError> {
        let key = req.target.as_key().to_owned();
        let method = req.method.as_str().to_owned();

        if self.is_shutting_down.load(Ordering::Acquire) {
            return Err(OutboundError::Transient(
                "client is shutting down".to_owned(),
            ));
        }

        let policy = self.policy_for(&key);

        if !policy.circuit_breaker.allow_request() {
            self.metrics
                .set_cb_state(&key, policy.circuit_breaker.state());
            self.count(&key, &method, "0", "circuit_breaker_open");
            return Err(OutboundError::Transient(format!(
                "circuit breaker open for {key}"
            )));
        }
        if policy.rate_limiter.check().is_err() {
            self.count(&key, &method, "0", "rate_limited");
            return Err(OutboundError::Transient(format!(
                "rate limited (local) for {key}"
            )));
        }

        self.in_flight.fetch_add(1, Ordering::AcqRel);
        let timeout = policy.config.timeout(self.default_timeout);
        let started = Instant::now();
        let outcome = self.execute(&req, timeout).await;
        self.metrics
            .request_duration
            .with_label_values(&[&key, &method])
            .observe(started.elapsed().as_secs_f64());

        let result = self.classify(&policy.circuit_breaker, &key, &method, timeout, outcome);
        self.metrics
            .set_cb_state(&key, policy.circuit_breaker.state());
        self.finish_in_flight();
        result
    }

    /// Send with automatic full-jitter retry of transient failures, up to the
    /// target's `retry_max_attempts`. Fatal errors return immediately.
    pub async fn send_with_retry(&self, req: OutboundRequest) -> Result<Response, OutboundError> {
        let key = req.target.as_key().to_owned();
        let policy = self.policy_for(&key);
        let (max, base, cap) = (
            policy.config.retry_max_attempts,
            policy.config.retry_base_ms,
            policy.config.retry_cap_ms,
        );

        let mut attempt = 0u32;
        loop {
            match self.send(req.clone()).await {
                Ok(resp) => return Ok(resp),
                Err(e) if e.is_fatal() || attempt >= max => return Err(e),
                Err(_) => {
                    attempt += 1;
                    self.metrics.retry_attempts.with_label_values(&[&key]).inc();
                    tokio::time::sleep(backoff::full_jitter(attempt, base, cap)).await;
                }
            }
        }
    }

    /// Stop accepting new work and wait for in-flight requests to drain, up to
    /// `timeout`.
    pub async fn shutdown(&self, timeout: Duration) -> Result<(), ShutdownError> {
        self.is_shutting_down.store(true, Ordering::Release);
        if self.in_flight.load(Ordering::Acquire) == 0 {
            return Ok(());
        }
        let drained = self.all_done.notified();
        // Re-check after registering the waiter to avoid a lost wakeup.
        if self.in_flight.load(Ordering::Acquire) == 0 {
            return Ok(());
        }
        match tokio::time::timeout(timeout, drained).await {
            Ok(()) => Ok(()),
            Err(_) => Err(ShutdownError::Timeout {
                in_flight: self.in_flight.load(Ordering::Acquire),
            }),
        }
    }

    async fn execute(&self, req: &OutboundRequest, timeout: Duration) -> Result<Response, ExecErr> {
        let mut rb = self
            .client
            .request(req.method.clone(), &req.url)
            .headers(req.headers.clone());
        if let Some(body) = &req.body {
            rb = rb.body(body.clone());
        }
        match tokio::time::timeout(timeout, rb.send()).await {
            Err(_elapsed) => Err(ExecErr::Timeout),
            Ok(Err(e)) if e.is_timeout() => Err(ExecErr::Timeout),
            Ok(Err(e)) if e.is_connect() => Err(ExecErr::Connect(e.to_string())),
            Ok(Err(e)) => Err(ExecErr::Other(e.to_string())),
            Ok(Ok(resp)) => Ok(resp),
        }
    }

    fn classify(
        &self,
        cb: &CircuitBreaker,
        key: &str,
        method: &str,
        timeout: Duration,
        outcome: Result<Response, ExecErr>,
    ) -> Result<Response, OutboundError> {
        match outcome {
            Ok(resp) => {
                let status = resp.status();
                let code = status.as_u16().to_string();
                if status.is_success() || status.is_redirection() {
                    cb.record_success();
                    self.count(key, method, &code, "ok");
                    Ok(resp)
                } else if status.as_u16() == 429 {
                    cb.record_failure();
                    self.count(key, method, &code, "rate_limited_upstream");
                    Err(OutboundError::Transient(format!("429 from {key}")))
                } else if status.is_server_error() {
                    cb.record_failure();
                    self.count(key, method, &code, "server_error");
                    Err(OutboundError::Transient(format!(
                        "{code} server error from {key}"
                    )))
                } else {
                    // 4xx (except 429): the server is healthy, the request is bad
                    // → success for the breaker, fatal for the caller.
                    cb.record_success();
                    self.count(key, method, &code, "client_error");
                    Err(OutboundError::Fatal(format!(
                        "{code} client error from {key}"
                    )))
                }
            }
            Err(ExecErr::Timeout) => {
                cb.record_failure();
                self.count(key, method, "0", "timeout");
                Err(OutboundError::Transient(format!(
                    "request to {key} timed out after {timeout:?}"
                )))
            }
            Err(ExecErr::Connect(msg)) => {
                cb.record_failure();
                self.count(key, method, "0", "connect_error");
                Err(OutboundError::Transient(msg))
            }
            Err(ExecErr::Other(msg)) => {
                cb.record_failure();
                self.count(key, method, "0", "error");
                Err(OutboundError::Transient(msg))
            }
        }
    }

    fn count(&self, target: &str, method: &str, status: &str, error_type: &str) {
        self.metrics
            .requests_total
            .with_label_values(&[target, method, status, error_type])
            .inc();
    }

    fn finish_in_flight(&self) {
        if self.in_flight.fetch_sub(1, Ordering::AcqRel) == 1
            && self.is_shutting_down.load(Ordering::Acquire)
        {
            self.all_done.notify_waiters();
        }
    }
}

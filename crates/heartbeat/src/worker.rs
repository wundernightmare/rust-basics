//! heartbeat — a background ticker worker (the "worker" shape of this
//! workspace, the analogue of a Kafka-consumer crate in tracehub-edge, minus
//! the broker). On each tick it bumps `heartbeat_beats_total`; if an upstream
//! URL is configured it also polls it through [`resilient_client`] and records
//! the outcome in `heartbeat_upstream_checks_total{result}`. It still reuses the
//! shared `httpx` server for its `/healthz`, `/readyz` and `/metrics` surface.

use std::sync::Arc;
use std::time::Duration;

use prometheus::{IntCounter, IntCounterVec, Opts, Registry};
use resilient_client::{
    ClientConfig, Metrics, OutboundRequest, OutboundTargetConfig, ResilientHttpClient,
    ResourceGroup,
};

/// Periodically "beats"; optionally polls an upstream via the resilient client.
pub struct Worker {
    interval: Duration,
    beats: IntCounter,
    upstream: Option<Upstream>,
}

struct Upstream {
    client: Arc<ResilientHttpClient>,
    url: String,
    checks: IntCounterVec,
}

impl Worker {
    /// Build a worker ticking every `interval`, registering its counters on
    /// `registry`. If `upstream_url` is set, each tick polls it via a resilient
    /// client (timeout + retry + circuit breaker) and records the result.
    pub fn new(
        interval: Duration,
        registry: &Registry,
        upstream_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let beats = IntCounter::new(
            "heartbeat_beats_total",
            "Total number of heartbeats emitted since process start.",
        )?;
        registry.register(Box::new(beats.clone()))?;

        let upstream = match upstream_url {
            None => None,
            Some(url) => {
                let checks = IntCounterVec::new(
                    Opts::new(
                        "heartbeat_upstream_checks_total",
                        "Upstream health checks performed each tick, by result.",
                    ),
                    &["result"],
                )?;
                registry.register(Box::new(checks.clone()))?;

                let cfg = ClientConfig {
                    default_timeout_ms: 1_000,
                    outbound_targets: vec![OutboundTargetConfig {
                        name: "upstream".to_owned(),
                        rate_limit: 1_000,
                        retry_max_attempts: 2,
                        retry_base_ms: 50,
                        retry_cap_ms: 500,
                        ..OutboundTargetConfig::default()
                    }],
                    ..ClientConfig::default()
                };
                let client = ResilientHttpClient::new(cfg, Arc::new(Metrics::new()?))?;
                Some(Upstream {
                    client: Arc::new(client),
                    url,
                    checks,
                })
            }
        };

        Ok(Self {
            interval,
            beats,
            upstream,
        })
    }

    /// Tick until cancelled (its task dropped) — the normal graceful-shutdown
    /// path when the server stops first.
    pub async fn run(self) {
        let mut ticker = tokio::time::interval(self.interval);
        // `interval` fires immediately; consume it so the first beat lands one
        // full interval in.
        ticker.tick().await;

        let mut count: u64 = 0;
        loop {
            ticker.tick().await;
            count += 1;
            self.beats.inc();
            tracing::info!(count, "heartbeat");

            if let Some(up) = &self.upstream {
                let req = OutboundRequest::get(ResourceGroup::new("upstream"), up.url.clone());
                let result = match up.client.send_with_retry(req).await {
                    Ok(_) => "ok",
                    Err(e) => {
                        tracing::warn!(error = %e, url = %up.url, "upstream check failed");
                        "err"
                    }
                };
                up.checks.with_label_values(&[result]).inc();
            }
        }
    }
}

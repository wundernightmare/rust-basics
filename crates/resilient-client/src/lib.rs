//! `resilient-client` — a policy-per-target HTTP client.
//!
//! A focused port of tracehub-edge's `resilient-http-client`: a shared
//! [`reqwest::Client`] connection pool fronted by per-target resilience policy:
//!
//! - **GCRA rate limiting** (lock-free, via `governor`) — smooths bursts;
//! - **circuit breaker** (lock-free atomics) — sheds load to a failing upstream;
//! - **full-jitter exponential backoff retry** — only for transient errors;
//! - **per-target timeouts**;
//! - **Prometheus metrics** on a private registry;
//! - **graceful shutdown** — drains in-flight requests.
//!
//! Errors are split [`OutboundError::Transient`] (retry later) vs
//! [`OutboundError::Fatal`] (discard) so callers can route failures sensibly.
//!
//! ```no_run
//! use resilient_client::{ClientConfig, Metrics, OutboundRequest, ResilientHttpClient, ResourceGroup};
//! use std::sync::Arc;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let cfg: ClientConfig = serde_yaml_ng::from_str(r#"
//! default_timeout_ms: 2000
//! outbound_targets:
//!   - name: "upstream"
//!     rate_limit: 500
//!     cb_threshold: 0.5
//!     retry_max_attempts: 3
//! "#)?;
//! let client = ResilientHttpClient::new(cfg, Arc::new(Metrics::new()?))?;
//! let resp = client
//!     .send_with_retry(OutboundRequest::get(ResourceGroup::new("upstream"), "https://example.com/health"))
//!     .await?;
//! # let _ = resp; Ok(())
//! # }
//! ```

#![allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions
)]

mod backoff;
mod circuit_breaker;
mod client;
mod config;
mod error;
mod metrics;

pub use circuit_breaker::CbState;
pub use client::{OutboundRequest, ResilientHttpClient, ResourceGroup};
pub use config::{ClientConfig, OutboundTargetConfig};
pub use error::{OutboundError, ShutdownError};
pub use metrics::Metrics;

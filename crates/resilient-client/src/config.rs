//! Declarative configuration for [`crate::ResilientHttpClient`].
//!
//! Load from YAML:
//! ```ignore
//! let cfg: ClientConfig = serde_yaml_ng::from_str(yaml_str)?;
//! ```

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Complete configuration for [`crate::ResilientHttpClient`]. Every field has a
/// `#[serde(default)]`, so only what differs from the default need be specified.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    /// Maximum idle connections kept per host in the connection pool.
    #[serde(default = "defaults::pool_max_idle_per_host")]
    pub pool_max_idle_per_host: usize,
    /// How long an idle connection is kept before being closed (seconds).
    #[serde(default = "defaults::pool_idle_timeout_secs")]
    pub pool_idle_timeout_secs: u64,
    /// TCP keepalive interval (seconds). `0` disables.
    #[serde(default = "defaults::tcp_keepalive_secs")]
    pub tcp_keepalive_secs: u64,
    /// Default per-request timeout (ms), used when a target doesn't override it.
    #[serde(default = "defaults::default_timeout_ms")]
    pub default_timeout_ms: u64,
    /// Custom `User-Agent` header sent with every request (reqwest default if `None`).
    #[serde(default)]
    pub user_agent: Option<String>,
    /// One entry per logical outbound target; `name` is the lookup key.
    #[serde(default)]
    pub outbound_targets: Vec<OutboundTargetConfig>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            pool_max_idle_per_host: defaults::pool_max_idle_per_host(),
            pool_idle_timeout_secs: defaults::pool_idle_timeout_secs(),
            tcp_keepalive_secs: defaults::tcp_keepalive_secs(),
            default_timeout_ms: defaults::default_timeout_ms(),
            user_agent: None,
            outbound_targets: Vec::new(),
        }
    }
}

/// Policy settings for one logical outbound target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboundTargetConfig {
    /// Logical identifier and metric label, e.g. `"meta_events"`.
    pub name: String,
    /// Free-form selector / template URL, used only as a metric label.
    #[serde(default)]
    pub selector: String,
    /// Sustained requests/second (GCRA smooths short bursts). Clamped to ≥ 1.
    #[serde(default = "defaults::rate_limit")]
    pub rate_limit: u32,
    /// Per-request timeout (ms); overrides the client default. `None` → default.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Failure ratio in the window that trips the breaker (`0.0..=1.0`).
    #[serde(default = "defaults::cb_threshold")]
    pub cb_threshold: f64,
    /// Minimum requests in the window before the ratio is evaluated.
    #[serde(default = "defaults::cb_min_requests")]
    pub cb_min_requests: u32,
    /// Sliding measurement window (seconds).
    #[serde(default = "defaults::cb_window_secs")]
    pub cb_window_secs: u64,
    /// How long the circuit stays Open before a probe is allowed (seconds).
    #[serde(default = "defaults::cb_half_open_timeout_secs")]
    pub cb_half_open_timeout_secs: u64,
    /// Max attempts for [`crate::ResilientHttpClient::send_with_retry`]
    /// (`0` = no automatic retries). Only transient errors are retried.
    #[serde(default)]
    pub retry_max_attempts: u32,
    /// Base delay (ms) for full-jitter backoff.
    #[serde(default = "defaults::retry_base_ms")]
    pub retry_base_ms: u64,
    /// Delay ceiling (ms) for full-jitter backoff.
    #[serde(default = "defaults::retry_cap_ms")]
    pub retry_cap_ms: u64,
}

impl Default for OutboundTargetConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            selector: String::new(),
            rate_limit: defaults::rate_limit(),
            timeout_ms: None,
            cb_threshold: defaults::cb_threshold(),
            cb_min_requests: defaults::cb_min_requests(),
            cb_window_secs: defaults::cb_window_secs(),
            cb_half_open_timeout_secs: defaults::cb_half_open_timeout_secs(),
            retry_max_attempts: 0,
            retry_base_ms: defaults::retry_base_ms(),
            retry_cap_ms: defaults::retry_cap_ms(),
        }
    }
}

impl OutboundTargetConfig {
    pub(crate) fn fallback(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }

    pub(crate) fn timeout(&self, default: Duration) -> Duration {
        self.timeout_ms.map_or(default, Duration::from_millis)
    }
}

mod defaults {
    pub fn pool_max_idle_per_host() -> usize {
        100
    }
    pub fn pool_idle_timeout_secs() -> u64 {
        90
    }
    pub fn tcp_keepalive_secs() -> u64 {
        30
    }
    pub fn default_timeout_ms() -> u64 {
        5_000
    }
    pub fn rate_limit() -> u32 {
        1_000
    }
    pub fn cb_threshold() -> f64 {
        0.5
    }
    pub fn cb_min_requests() -> u32 {
        10
    }
    pub fn cb_window_secs() -> u64 {
        10
    }
    pub fn cb_half_open_timeout_secs() -> u64 {
        30
    }
    pub fn retry_base_ms() -> u64 {
        100
    }
    pub fn retry_cap_ms() -> u64 {
        30_000
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn fallback_sets_name_and_uses_defaults() {
        let fb = OutboundTargetConfig::fallback("test_target");
        assert_eq!(fb.name, "test_target");
        assert_eq!(fb.rate_limit, defaults::rate_limit());
    }

    #[test]
    fn timeout_uses_override_or_default() {
        let mut cfg = OutboundTargetConfig::default();
        let default = Duration::from_secs(5);
        assert_eq!(cfg.timeout(default), default);
        cfg.timeout_ms = Some(1234);
        assert_eq!(cfg.timeout(default), Duration::from_millis(1234));
    }

    #[test]
    fn parses_yaml() {
        let cfg: ClientConfig = serde_yaml_ng::from_str(
            "default_timeout_ms: 2000\noutbound_targets:\n  - name: api\n    rate_limit: 50\n",
        )
        .unwrap();
        assert_eq!(cfg.default_timeout_ms, 2000);
        assert_eq!(cfg.outbound_targets.len(), 1);
        assert_eq!(cfg.outbound_targets[0].rate_limit, 50);
    }
}

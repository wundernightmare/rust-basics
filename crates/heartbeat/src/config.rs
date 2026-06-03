use std::time::Duration;

use serde::Deserialize;

/// Heartbeat worker configuration. Carries the shared HTTP fields (so the
/// health/metrics server can be built from `httpx`) plus the worker's own tick
/// interval. All keys are prefixed `HEARTBEAT_`:
///
/// | Variable                          | Default        | Meaning                       |
/// | --------------------------------- | -------------- | ----------------------------- |
/// | `HEARTBEAT_ADDR`                  | `0.0.0.0:8081` | health/metrics listen address |
/// | `HEARTBEAT_SHUTDOWN_TIMEOUT_SECS` | `10`           | graceful-shutdown budget      |
/// | `HEARTBEAT_LOG_LEVEL`             | `info`         | log level                     |
/// | `HEARTBEAT_LOG_FORMAT`            | `json`         | `json` or `text`              |
/// | `HEARTBEAT_INTERVAL_MS`           | `5000`         | tick period in milliseconds   |
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_addr")]
    pub addr: String,
    #[serde(default = "default_shutdown_secs")]
    pub shutdown_timeout_secs: u64,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
    /// Optional upstream URL polled each tick via the resilient client
    /// (`HEARTBEAT_UPSTREAM_URL`). Unset ⇒ no upstream check.
    #[serde(default)]
    pub upstream_url: Option<String>,
}

fn default_addr() -> String {
    "0.0.0.0:8081".to_owned()
}
fn default_shutdown_secs() -> u64 {
    10
}
fn default_log_level() -> String {
    "info".to_owned()
}
fn default_log_format() -> String {
    "json".to_owned()
}
fn default_interval_ms() -> u64 {
    5000
}

impl Config {
    /// Parse from `HEARTBEAT_`-prefixed environment variables.
    pub fn from_env() -> Result<Self, envy::Error> {
        envy::prefixed("HEARTBEAT_").from_env::<Self>()
    }

    /// Project the shared HTTP fields into an [`httpx::Config`].
    pub fn httpx(&self) -> httpx::Config {
        httpx::Config {
            addr: self.addr.clone(),
            shutdown_timeout_secs: self.shutdown_timeout_secs,
            log_level: self.log_level.clone(),
            log_format: self.log_format.clone(),
        }
    }

    /// The tick interval as a [`Duration`].
    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms)
    }
}

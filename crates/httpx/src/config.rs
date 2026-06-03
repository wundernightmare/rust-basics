use std::time::Duration;

use serde::Deserialize;

/// Shared HTTP-server configuration, populated from the environment.
///
/// Services load it with a prefix (e.g. `PING_`) so several binaries can
/// coexist without key collisions. With the `PING_` prefix:
///
/// | Variable                      | Default          | Meaning                    |
/// | ----------------------------- | ---------------- | -------------------------- |
/// | `PING_ADDR`                   | `0.0.0.0:8080`   | listen address             |
/// | `PING_SHUTDOWN_TIMEOUT_SECS`  | `10`             | graceful-shutdown budget   |
/// | `PING_LOG_LEVEL`              | `info`           | `debug`/`info`/`warn`/`error` |
/// | `PING_LOG_FORMAT`             | `json`           | `json` or `text`           |
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
}

fn default_addr() -> String {
    "0.0.0.0:8080".to_owned()
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

impl Config {
    /// Parse a [`Config`] from `PREFIX`-prefixed environment variables (use
    /// `""` for no prefix). Every field is defaulted, so the result is always
    /// fully populated.
    pub fn from_env(prefix: &str) -> Result<Self, envy::Error> {
        envy::prefixed(prefix).from_env::<Self>()
    }

    /// The graceful-shutdown budget as a [`Duration`].
    pub fn shutdown_timeout(&self) -> Duration {
        Duration::from_secs(self.shutdown_timeout_secs)
    }
}

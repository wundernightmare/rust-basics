use std::path::Path;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_yaml_ng::{Mapping, Value};

/// Load a config of type `T` from an optional YAML file, overlaid with
/// `PREFIX`-prefixed environment variables — the config-file story alongside
/// the pure-env [`Config::from_env`]. Precedence, highest first:
///
/// ```text
/// environment variable  >  YAML file value  >  serde default
/// ```
///
/// A missing file at `path` is not an error (env-only config still works), so
/// the same binary runs from a mounted `config.yaml` in production and from bare
/// env vars in a test. `T` should give its fields `serde` defaults so a sparse
/// file/env still deserializes. Each env value is parsed as a YAML scalar before
/// being merged, so `WORKERS=4` becomes the integer `4` and `FLAGS=[a,b]` a
/// sequence — matching how the field is typed.
///
/// # Errors
/// Returns an error if the YAML file is present but unreadable / malformed, or
/// if the merged document does not deserialize into `T`.
pub fn load_yaml<T: DeserializeOwned>(path: Option<&str>, env_prefix: &str) -> anyhow::Result<T> {
    let mut root: Value = match path {
        Some(p) if Path::new(p).exists() => serde_yaml_ng::from_str(&std::fs::read_to_string(p)?)?,
        _ => Value::Mapping(Mapping::new()),
    };
    let map = root
        .as_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("httpx: YAML config root must be a mapping"))?;

    for (key, val) in std::env::vars() {
        let Some(rest) = key.strip_prefix(env_prefix) else {
            continue;
        };
        // Parse the env string as a YAML scalar so numbers/bools/sequences land
        // with the right type; fall back to a plain string.
        let parsed: Value = serde_yaml_ng::from_str(&val).unwrap_or(Value::String(val.clone()));
        map.insert(Value::String(rest.to_lowercase()), parsed);
    }

    Ok(serde_yaml_ng::from_value(root)?)
}

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

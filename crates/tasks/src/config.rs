//! Unified tasks configuration: loaded from an optional YAML file overlaid with
//! `TASKS_`-prefixed environment variables (see [`httpx::load_yaml`]) and
//! projected into the per-dependency configs. The field names match the
//! sub-configs', so a single source of truth feeds every lib.

use serde::Deserialize;

/// The full tasks-service configuration.
///
/// Load from a file with `TASKS_CONFIG=/path/config.yaml`; every key is also an
/// env var under the `TASKS_` prefix (e.g. `TASKS_VALKEY_URL`,
/// `TASKS_KAFKA_BROKERS`). Env wins over the file; both win over these defaults.
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

    #[serde(default = "default_valkey_url")]
    pub valkey_url: String,
    #[serde(default = "default_probe_secs")]
    pub valkey_probe_secs: u64,

    #[serde(default = "default_brokers")]
    pub kafka_brokers: String,
    #[serde(default = "default_topic")]
    pub kafka_topic: String,
    #[serde(default = "default_client_id")]
    pub kafka_client_id: String,
    #[serde(default = "default_probe_secs")]
    pub kafka_probe_secs: u64,

    #[serde(default)]
    pub otel_enabled: bool,
    #[serde(default = "default_service")]
    pub otel_service_name: String,
    #[serde(default = "default_endpoint")]
    pub otel_endpoint: String,
    #[serde(default = "default_ratio")]
    pub otel_sampler_ratio: f64,
}

fn default_addr() -> String {
    "0.0.0.0:8082".to_owned()
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
fn default_valkey_url() -> String {
    "redis://127.0.0.1:6379".to_owned()
}
fn default_probe_secs() -> u64 {
    5
}
fn default_brokers() -> String {
    "localhost:9092".to_owned()
}
fn default_topic() -> String {
    "tasks.events".to_owned()
}
fn default_client_id() -> String {
    "tasks".to_owned()
}
fn default_service() -> String {
    "tasks".to_owned()
}
fn default_endpoint() -> String {
    "http://localhost:4317".to_owned()
}
fn default_ratio() -> f64 {
    1.0
}

impl Config {
    /// Load from the file named by `TASKS_CONFIG` (when set and present) overlaid
    /// with `TASKS_`-prefixed env vars.
    ///
    /// # Errors
    /// Propagates file/parse errors from [`httpx::load_yaml`].
    pub fn load() -> anyhow::Result<Self> {
        let path = std::env::var("TASKS_CONFIG").ok();
        httpx::load_yaml(path.as_deref(), "TASKS_")
    }

    #[must_use]
    pub fn httpx(&self) -> httpx::Config {
        httpx::Config {
            addr: self.addr.clone(),
            shutdown_timeout_secs: self.shutdown_timeout_secs,
            log_level: self.log_level.clone(),
            log_format: self.log_format.clone(),
        }
    }

    #[must_use]
    pub fn valkey(&self) -> valkey::Config {
        valkey::Config {
            valkey_url: self.valkey_url.clone(),
            valkey_probe_secs: self.valkey_probe_secs,
        }
    }

    #[must_use]
    pub fn kafka(&self) -> kafka::Config {
        kafka::Config {
            kafka_brokers: self.kafka_brokers.clone(),
            kafka_topic: self.kafka_topic.clone(),
            kafka_group: "tasks-producer".to_owned(),
            kafka_client_id: self.kafka_client_id.clone(),
            kafka_probe_secs: self.kafka_probe_secs,
        }
    }

    #[must_use]
    pub fn otelx(&self) -> otelx::Config {
        otelx::Config {
            otel_enabled: self.otel_enabled,
            otel_service_name: self.otel_service_name.clone(),
            otel_endpoint: self.otel_endpoint.clone(),
            otel_sampler_ratio: self.otel_sampler_ratio,
        }
    }
}

//! Consumer worker configuration, populated from `CONSUMER_`-prefixed
//! environment variables (env-only, mirroring `heartbeat` — the YAML config-file
//! story is shown in `tasks`).

use serde::Deserialize;

/// Carries the shared HTTP fields (for the health/metrics server) plus the
/// broker subscription and tracing settings. All keys are prefixed `CONSUMER_`:
///
/// | Variable                   | Default        | Meaning                       |
/// | -------------------------- | -------------- | ----------------------------- |
/// | `CONSUMER_ADDR`            | `0.0.0.0:8083` | health/metrics listen address |
/// | `CONSUMER_LOG_LEVEL`       | `info`         | log level                     |
/// | `CONSUMER_LOG_FORMAT`      | `json`         | `json` or `text`              |
/// | `CONSUMER_KAFKA_BROKERS`   | `localhost:9092` | comma-separated seeds       |
/// | `CONSUMER_KAFKA_TOPIC`     | `tasks.events` | topic to drain                |
/// | `CONSUMER_KAFKA_GROUP`     | `tasks-consumer` | consumer group id           |
/// | `CONSUMER_OTEL_ENABLED`    | `false`        | export traces                 |
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

    #[serde(default = "default_brokers")]
    pub kafka_brokers: String,
    #[serde(default = "default_topic")]
    pub kafka_topic: String,
    #[serde(default = "default_group")]
    pub kafka_group: String,
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
    "0.0.0.0:8083".to_owned()
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
fn default_brokers() -> String {
    "localhost:9092".to_owned()
}
fn default_topic() -> String {
    "tasks.events".to_owned()
}
fn default_group() -> String {
    "tasks-consumer".to_owned()
}
fn default_probe_secs() -> u64 {
    5
}
fn default_service() -> String {
    "consumer".to_owned()
}
fn default_endpoint() -> String {
    "http://localhost:4317".to_owned()
}
fn default_ratio() -> f64 {
    1.0
}

impl Config {
    /// Parse from `CONSUMER_`-prefixed environment variables.
    pub fn from_env() -> Result<Self, envy::Error> {
        envy::prefixed("CONSUMER_").from_env::<Self>()
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
    pub fn kafka(&self) -> kafka::Config {
        kafka::Config {
            kafka_brokers: self.kafka_brokers.clone(),
            kafka_topic: self.kafka_topic.clone(),
            kafka_group: self.kafka_group.clone(),
            kafka_client_id: "consumer".to_owned(),
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

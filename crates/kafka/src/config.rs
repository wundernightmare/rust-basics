use serde::Deserialize;

/// Kafka client configuration, populated from `PREFIX`-prefixed environment
/// variables. The same struct serves both shapes; a producer ignores `group`
/// and a consumer subscribes to `topic`.
///
/// With the `TASKS_` prefix:
///
/// | Variable                  | Default          | Meaning                          |
/// | ------------------------- | ---------------- | -------------------------------- |
/// | `TASKS_KAFKA_BROKERS`     | `localhost:9092` | comma-separated bootstrap seeds  |
/// | `TASKS_KAFKA_TOPIC`       | `tasks.events`   | produce / subscribe topic        |
/// | `TASKS_KAFKA_GROUP`       | `tasks-consumer` | consumer group id                |
/// | `TASKS_KAFKA_CLIENT_ID`   | `rust-basics`    | client id advertised to brokers  |
/// | `TASKS_KAFKA_PROBE_SECS`  | `5`              | readiness-probe period (seconds) |
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_brokers")]
    pub kafka_brokers: String,
    #[serde(default = "default_topic")]
    pub kafka_topic: String,
    #[serde(default = "default_group")]
    pub kafka_group: String,
    #[serde(default = "default_client_id")]
    pub kafka_client_id: String,
    #[serde(default = "default_probe_secs")]
    pub kafka_probe_secs: u64,
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
fn default_client_id() -> String {
    "rust-basics".to_owned()
}
fn default_probe_secs() -> u64 {
    5
}

impl Config {
    /// Parse from `PREFIX`-prefixed environment variables (use `""` for none).
    pub fn from_env(prefix: &str) -> Result<Self, envy::Error> {
        envy::prefixed(prefix).from_env::<Self>()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            kafka_brokers: default_brokers(),
            kafka_topic: default_topic(),
            kafka_group: default_group(),
            kafka_client_id: default_client_id(),
            kafka_probe_secs: default_probe_secs(),
        }
    }
}

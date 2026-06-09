use serde::Deserialize;

/// Tracing configuration, populated from `PREFIX`-prefixed environment
/// variables. With the `TASKS_` prefix:
///
/// | Variable                  | Default                 | Meaning                       |
/// | ------------------------- | ----------------------- | ----------------------------- |
/// | `TASKS_OTEL_ENABLED`      | `false`                 | export traces on/off          |
/// | `TASKS_OTEL_SERVICE_NAME` | `service`               | resource `service.name`       |
/// | `TASKS_OTEL_ENDPOINT`     | `http://localhost:4317` | OTLP/gRPC collector endpoint  |
/// | `TASKS_OTEL_SAMPLER_RATIO`| `1.0`                   | head sampling ratio [0.0–1.0] |
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub otel_enabled: bool,
    #[serde(default = "default_service")]
    pub otel_service_name: String,
    #[serde(default = "default_endpoint")]
    pub otel_endpoint: String,
    #[serde(default = "default_ratio")]
    pub otel_sampler_ratio: f64,
}

fn default_service() -> String {
    "service".to_owned()
}
fn default_endpoint() -> String {
    "http://localhost:4317".to_owned()
}
fn default_ratio() -> f64 {
    1.0
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
            otel_enabled: false,
            otel_service_name: default_service(),
            otel_endpoint: default_endpoint(),
            otel_sampler_ratio: default_ratio(),
        }
    }
}

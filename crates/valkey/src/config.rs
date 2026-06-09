use serde::Deserialize;

/// Valkey client configuration, populated from `PREFIX`-prefixed environment
/// variables. With the `TASKS_` prefix:
///
/// | Variable                   | Default                  | Meaning                          |
/// | -------------------------- | ------------------------ | -------------------------------- |
/// | `TASKS_VALKEY_URL`         | `redis://127.0.0.1:6379` | connection URL (RESP)            |
/// | `TASKS_VALKEY_PROBE_SECS`  | `5`                      | readiness-probe period (seconds) |
///
/// The `redis://` scheme is the RESP wire protocol that Valkey speaks; point it
/// at a Valkey server.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_url")]
    pub valkey_url: String,
    #[serde(default = "default_probe_secs")]
    pub valkey_probe_secs: u64,
}

fn default_url() -> String {
    "redis://127.0.0.1:6379".to_owned()
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
            valkey_url: default_url(),
            valkey_probe_secs: default_probe_secs(),
        }
    }
}

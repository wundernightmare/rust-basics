//! `valkey` — shared Valkey (Redis-compatible) cache for rust-basics services.
//!
//! The cache analogue of the `httpx` HTTP scaffolding and the Rust counterpart
//! of the Go sibling's `libs/valkey`. It wraps the maintained, widely-used
//! `redis` (redis-rs) client — which speaks RESP to a Valkey server — behind a
//! small, cloneable [`Cache`] handle with:
//!
//! - typed key/value + JSON helpers and a small set-of-ids API (enough to back
//!   `crates/tasks`' store on Valkey alone — no separate database);
//! - a **synchronous** readiness check ([`Cache::readiness_check`]) that plugs
//!   straight into [`httpx::Health`]. Because that registry runs checks
//!   synchronously, a background task probes the server every few seconds and
//!   the check just reads the cached health flag — no blocking in the readiness
//!   handler.
//!
//! ```no_run
//! # async fn run() -> anyhow::Result<()> {
//! let cfg = valkey::Config::from_env("TASKS_")?;
//! let cache = valkey::Cache::connect(&cfg).await?;
//! cache.set_json("task:1", &"hello", None).await?;
//! # Ok(()) }
//! ```

#![allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions
)]

mod config;

pub use config::Config;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use redis::AsyncCommands;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// A cloneable handle to a Valkey server. Cloning shares the underlying
/// connection manager (which multiplexes and transparently reconnects) and the
/// health flag, so pass it around freely.
#[derive(Clone)]
pub struct Cache {
    conn: redis::aio::ConnectionManager,
    healthy: Arc<AtomicBool>,
}

impl Cache {
    /// Connect to the server in `cfg`, verify it with a `PING` (so a
    /// misconfigured cache fails fast at boot) and spawn the background
    /// readiness probe.
    pub async fn connect(cfg: &Config) -> anyhow::Result<Self> {
        let client = redis::Client::open(cfg.valkey_url.as_str())?;
        let mut conn = client.get_connection_manager().await?;
        let _: String = redis::cmd("PING").query_async(&mut conn).await?;

        let cache = Self {
            conn,
            healthy: Arc::new(AtomicBool::new(true)),
        };
        cache.spawn_health_probe(Duration::from_secs(cfg.valkey_probe_secs.max(1)));
        tracing::info!(url = %cfg.valkey_url, "valkey cache ready");
        Ok(cache)
    }

    /// Get the string value at `key`, or `None` on a miss.
    pub async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let mut conn = self.conn.clone();
        Ok(conn.get(key).await?)
    }

    /// Set `key` to `value`, optionally with a time-to-live.
    pub async fn set(&self, key: &str, value: &str, ttl: Option<Duration>) -> anyhow::Result<()> {
        let mut conn = self.conn.clone();
        match ttl {
            Some(ttl) => conn.set_ex(key, value, ttl.as_secs().max(1)).await?,
            None => conn.set(key, value).await?,
        }
        Ok(())
    }

    /// Delete `key` (a no-op if it does not exist).
    pub async fn del(&self, key: &str) -> anyhow::Result<()> {
        let mut conn = self.conn.clone();
        let _: i64 = conn.del(key).await?;
        Ok(())
    }

    /// JSON-serialize `value` and store it at `key` (optionally with a TTL).
    pub async fn set_json<T: Serialize + Sync>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> anyhow::Result<()> {
        let payload = serde_json::to_string(value)?;
        self.set(key, &payload, ttl).await
    }

    /// Fetch and JSON-deserialize the value at `key`, or `None` on a miss.
    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> anyhow::Result<Option<T>> {
        match self.get(key).await? {
            Some(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            None => Ok(None),
        }
    }

    /// Add `member` to the set at `key` (used as an index of ids).
    pub async fn set_add(&self, key: &str, member: &str) -> anyhow::Result<()> {
        let mut conn = self.conn.clone();
        let _: i64 = conn.sadd(key, member).await?;
        Ok(())
    }

    /// Remove `member` from the set at `key`.
    pub async fn set_remove(&self, key: &str, member: &str) -> anyhow::Result<()> {
        let mut conn = self.conn.clone();
        let _: i64 = conn.srem(key, member).await?;
        Ok(())
    }

    /// Return every member of the set at `key`.
    pub async fn set_members(&self, key: &str) -> anyhow::Result<Vec<String>> {
        let mut conn = self.conn.clone();
        Ok(conn.smembers(key).await?)
    }

    /// A synchronous readiness probe for [`httpx::Health`]: returns `Ok(())`
    /// while the background probe last saw the server reachable.
    pub fn readiness_check(&self) -> impl Fn() -> Result<(), String> + Send + Sync + Clone {
        let healthy = Arc::clone(&self.healthy);
        move || {
            if healthy.load(Ordering::Relaxed) {
                Ok(())
            } else {
                Err("valkey unreachable".to_owned())
            }
        }
    }

    fn spawn_health_probe(&self, period: Duration) {
        let mut conn = self.conn.clone();
        let healthy = Arc::clone(&self.healthy);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(period);
            loop {
                ticker.tick().await;
                let ok = redis::cmd("PING")
                    .query_async::<String>(&mut conn)
                    .await
                    .is_ok();
                healthy.store(ok, Ordering::Relaxed);
            }
        });
    }
}

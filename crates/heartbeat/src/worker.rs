use std::time::Duration;

use prometheus::{IntCounter, Registry};

/// Periodically "beats": on every tick it bumps the `heartbeat_beats_total`
/// counter and emits a structured log line, until its task is dropped.
pub struct Worker {
    interval: Duration,
    beats: IntCounter,
}

impl Worker {
    /// Build a worker ticking every `interval` and register its
    /// `heartbeat_beats_total` counter on `registry` (typically the server's
    /// metrics registry, so the count is exported on `/metrics`).
    pub fn new(interval: Duration, registry: &Registry) -> anyhow::Result<Self> {
        let beats = IntCounter::new(
            "heartbeat_beats_total",
            "Total number of heartbeats emitted since process start.",
        )?;
        registry.register(Box::new(beats.clone()))?;
        Ok(Self { interval, beats })
    }

    /// Tick forever. Returns only if cancelled (its task dropped) — that is the
    /// normal graceful-shutdown path when the server stops first.
    pub async fn run(self) {
        let mut ticker = tokio::time::interval(self.interval);
        // `interval` fires immediately on the first tick; consume it so the
        // first beat lands one full interval in.
        ticker.tick().await;

        let mut count: u64 = 0;
        loop {
            ticker.tick().await;
            count += 1;
            self.beats.inc();
            tracing::info!(count, "heartbeat");
        }
    }
}

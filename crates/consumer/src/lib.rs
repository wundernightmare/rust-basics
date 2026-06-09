//! consumer — a Kafka consumer worker that drains the `tasks.events` topic
//! produced by `crates/tasks`. The broker-fed counterpart to `heartbeat`'s
//! ticker: the worker loop and the shared `httpx` health/metrics server run
//! concurrently under one `tokio::select!`. For each `task.created` event it
//! bumps a Prometheus counter; an undecodable message is counted as skipped and
//! acknowledged (a poison record must not wedge the loop).
//!
//! Split into a lib so the worker is unit-testable without spawning the binary.

#![allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions
)]

mod config;

pub use config::Config;

use prometheus::{IntCounter, Registry};
use serde::Deserialize;

use kafka::Consumer;

/// The consumer's local copy of the event contract published by `crates/tasks`.
/// Intentionally duplicated rather than imported: the two services are decoupled
/// and only share the JSON shape on the wire.
#[derive(Debug, Deserialize)]
struct TaskCreatedEvent {
    id: String,
    #[serde(default)]
    title: String,
}

/// Consumes task events until cancelled (its future dropped on shutdown).
pub struct Worker {
    consumer: Consumer,
    consumed: IntCounter,
    skipped: IntCounter,
}

impl Worker {
    /// Build a worker over `consumer`, registering its counters on `registry`
    /// (typically the server's, so they appear on `/metrics`):
    ///
    /// ```text
    /// consumer_tasks_consumed_total  events handled successfully
    /// consumer_tasks_skipped_total   events dropped as undecodable
    /// ```
    pub fn new(kafka_consumer: Consumer, registry: &Registry) -> anyhow::Result<Self> {
        let consumed = IntCounter::new(
            "consumer_tasks_consumed_total",
            "Total number of task.created events consumed successfully.",
        )?;
        let skipped = IntCounter::new(
            "consumer_tasks_skipped_total",
            "Total number of events skipped because they could not be decoded.",
        )?;
        registry.register(Box::new(consumed.clone()))?;
        registry.register(Box::new(skipped.clone()))?;
        Ok(Self {
            consumer: kafka_consumer,
            consumed,
            skipped,
        })
    }

    /// A synchronous readiness probe for [`httpx::Health`].
    pub fn readiness_check(&self) -> impl Fn() -> Result<(), String> + Send + Sync + Clone {
        self.consumer.readiness_check()
    }

    /// Drain the topic until cancelled. Each successfully-decoded event bumps the
    /// consumed counter; an undecodable one bumps the skipped counter and is
    /// acknowledged so the loop keeps moving.
    pub async fn run(self) -> anyhow::Result<()> {
        let consumed = self.consumed.clone();
        let skipped = self.skipped.clone();
        self.consumer
            .run(move |msg| {
                let consumed = consumed.clone();
                let skipped = skipped.clone();
                async move {
                    match serde_json::from_slice::<TaskCreatedEvent>(&msg.payload) {
                        Ok(event) => {
                            consumed.inc();
                            tracing::info!(
                                id = %event.id,
                                title = %event.title,
                                partition = msg.partition,
                                offset = msg.offset,
                                "task.created consumed"
                            );
                        }
                        Err(error) => {
                            skipped.inc();
                            tracing::warn!(
                                %error,
                                partition = msg.partition,
                                offset = msg.offset,
                                "skipping undecodable event"
                            );
                        }
                    }
                    Ok(())
                }
            })
            .await
    }
}

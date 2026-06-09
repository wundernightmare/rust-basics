//! `kafka` — shared Kafka producer/consumer for rust-basics services.
//!
//! The broker analogue of `crates/valkey`, and the Rust counterpart of the Go
//! sibling's `libs/kafka`. It wraps `rdkafka` (the maintained librdkafka
//! binding — vendored and statically built, so the service binary stays
//! self-contained) behind two thin shapes:
//!
//! - [`Producer`] — awaited publishing ([`Producer::publish`]);
//! - [`Consumer`] — a consumer-group loop ([`Consumer::run`]) that hands each
//!   record to an async handler and commits only what the handler accepts, so a
//!   failing handler leaves the record for redelivery (at-least-once).
//!
//! Both expose a **synchronous** [`httpx::Health`]-compatible readiness check
//! backed by a background metadata probe (the readiness registry runs checks
//! synchronously, so a periodic probe updates a flag the check just reads).

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

use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer as _, StreamConsumer};
use rdkafka::message::Message as _;
use rdkafka::producer::{FutureProducer, FutureRecord, Producer as _};

const METADATA_TIMEOUT: Duration = Duration::from_secs(3);

/// One consumed record handed to a handler in [`Consumer::run`].
#[derive(Debug, Clone)]
pub struct Message {
    pub topic: String,
    pub key: Option<Vec<u8>>,
    pub payload: Vec<u8>,
    pub partition: i32,
    pub offset: i64,
}

/// Spawn a background task that runs `probe` every `period` and records the
/// outcome in `healthy`. `probe` runs on the blocking pool because rdkafka's
/// `fetch_metadata` is synchronous.
fn spawn_probe<F>(healthy: &Arc<AtomicBool>, period: Duration, probe: F)
where
    F: Fn() -> bool + Clone + Send + 'static,
{
    let healthy = Arc::clone(healthy);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(period);
        loop {
            ticker.tick().await;
            let probe = probe.clone();
            let ok = tokio::task::spawn_blocking(probe).await.unwrap_or(false);
            healthy.store(ok, Ordering::Relaxed);
        }
    });
}

fn readiness(healthy: &Arc<AtomicBool>) -> impl Fn() -> Result<(), String> + Send + Sync + Clone {
    let healthy = Arc::clone(healthy);
    move || {
        if healthy.load(Ordering::Relaxed) {
            Ok(())
        } else {
            Err("kafka brokers unreachable".to_owned())
        }
    }
}

// ── Producer ───────────────────────────────────────────────────────────────────

/// Publishes records to Kafka, awaiting the broker ack per send. Cheap to
/// [`Clone`] (the underlying producer is reference-counted).
#[derive(Clone)]
pub struct Producer {
    inner: FutureProducer,
    default_topic: String,
    healthy: Arc<AtomicBool>,
}

impl Producer {
    /// Build a producer from `cfg` and verify broker connectivity once (so a
    /// misconfigured broker fails fast at boot), then start the readiness probe.
    pub async fn connect(cfg: &Config) -> anyhow::Result<Self> {
        let inner: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &cfg.kafka_brokers)
            .set("client.id", &cfg.kafka_client_id)
            .set("message.timeout.ms", "5000")
            .create()?;

        let probe = inner.clone();
        tokio::task::spawn_blocking(move || probe.client().fetch_metadata(None, METADATA_TIMEOUT))
            .await??;

        let healthy = Arc::new(AtomicBool::new(true));
        let probe = inner.clone();
        spawn_probe(
            &healthy,
            Duration::from_secs(cfg.kafka_probe_secs.max(1)),
            move || {
                probe
                    .client()
                    .fetch_metadata(None, METADATA_TIMEOUT)
                    .is_ok()
            },
        );

        tracing::info!(brokers = %cfg.kafka_brokers, topic = %cfg.kafka_topic, "kafka producer ready");
        Ok(Self {
            inner,
            default_topic: cfg.kafka_topic.clone(),
            healthy,
        })
    }

    /// Publish one record and await the broker acknowledgement. An empty `topic`
    /// falls back to the configured default. `key` determines partitioning.
    pub async fn publish(&self, topic: &str, key: &[u8], payload: &[u8]) -> anyhow::Result<()> {
        let topic = if topic.is_empty() {
            self.default_topic.as_str()
        } else {
            topic
        };
        let record = FutureRecord::to(topic).key(key).payload(payload);
        self.inner
            .send(record, Duration::from_secs(5))
            .await
            .map_err(|(e, _)| anyhow::anyhow!("kafka publish to {topic}: {e}"))?;
        Ok(())
    }

    /// A synchronous readiness probe for [`httpx::Health`].
    pub fn readiness_check(&self) -> impl Fn() -> Result<(), String> + Send + Sync + Clone {
        readiness(&self.healthy)
    }
}

// ── Consumer ───────────────────────────────────────────────────────────────────

/// Drains a consumer group, handing each record to an async handler.
pub struct Consumer {
    inner: Arc<StreamConsumer>,
    healthy: Arc<AtomicBool>,
}

impl Consumer {
    /// Build a consumer-group client subscribed to `cfg.kafka_topic` and verify
    /// broker connectivity. Auto-commit is off; offsets advance only past
    /// records a handler accepts. A brand-new group starts from the earliest
    /// retained record, so a freshly-deployed worker does not skip a backlog.
    pub async fn connect(cfg: &Config) -> anyhow::Result<Self> {
        let inner: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &cfg.kafka_brokers)
            .set("group.id", &cfg.kafka_group)
            .set("client.id", &cfg.kafka_client_id)
            .set("enable.auto.commit", "false")
            .set("auto.offset.reset", "earliest")
            .create()?;
        inner.subscribe(&[cfg.kafka_topic.as_str()])?;
        let inner = Arc::new(inner);

        let probe = Arc::clone(&inner);
        tokio::task::spawn_blocking(move || probe.fetch_metadata(None, METADATA_TIMEOUT)).await??;

        let healthy = Arc::new(AtomicBool::new(true));
        let probe = Arc::clone(&inner);
        spawn_probe(
            &healthy,
            Duration::from_secs(cfg.kafka_probe_secs.max(1)),
            move || probe.fetch_metadata(None, METADATA_TIMEOUT).is_ok(),
        );

        tracing::info!(brokers = %cfg.kafka_brokers, group = %cfg.kafka_group, topic = %cfg.kafka_topic, "kafka consumer ready");
        Ok(Self { inner, healthy })
    }

    /// Poll and dispatch records to `handler` forever (until the future is
    /// dropped on shutdown). A record's offset is committed only after its
    /// handler returns `Ok`; a handler `Err` stops the loop and leaves the
    /// record uncommitted for redelivery.
    pub async fn run<F, Fut>(&self, mut handler: F) -> anyhow::Result<()>
    where
        F: FnMut(Message) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<()>>,
    {
        loop {
            match self.inner.recv().await {
                Err(e) => tracing::error!(error = %e, "kafka recv error"),
                Ok(borrowed) => {
                    let msg = Message {
                        topic: borrowed.topic().to_owned(),
                        key: borrowed.key().map(<[u8]>::to_vec),
                        payload: borrowed.payload().unwrap_or_default().to_vec(),
                        partition: borrowed.partition(),
                        offset: borrowed.offset(),
                    };
                    handler(msg).await?;
                    self.inner.commit_message(&borrowed, CommitMode::Async)?;
                }
            }
        }
    }

    /// A synchronous readiness probe for [`httpx::Health`].
    pub fn readiness_check(&self) -> impl Fn() -> Result<(), String> + Send + Sync + Clone {
        readiness(&self.healthy)
    }
}

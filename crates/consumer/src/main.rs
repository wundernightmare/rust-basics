//! Command `consumer` — the Kafka consumer worker. Connects to the broker
//! (failing fast at boot), runs the consume loop and the shared `httpx`
//! health/metrics server concurrently under one `tokio::select!`, so either half
//! failing tears the other down — the `heartbeat` pattern, fed by a broker.

use consumer::{Config, Worker};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::from_env()?;

    let _tracing = otelx::init(&cfg.otelx(), &cfg.log_level, &cfg.log_format)?;
    tracing::info!(
        addr = %cfg.addr,
        topic = %cfg.kafka_topic,
        group = %cfg.kafka_group,
        "consumer starting"
    );

    let kafka_consumer = kafka::Consumer::connect(&cfg.kafka()).await?;

    let server = httpx::Server::new(cfg.httpx());
    // The worker registers its counters on the server's registry, so consumed
    // counts show up on /metrics alongside the HTTP metrics.
    let worker = Worker::new(kafka_consumer, &server.metrics.registry)?;
    server.health.register("kafka", worker.readiness_check());

    tokio::select! {
        result = server.run() => result?,
        result = worker.run() => result?,
    }

    tracing::info!("consumer stopped cleanly");
    Ok(())
}

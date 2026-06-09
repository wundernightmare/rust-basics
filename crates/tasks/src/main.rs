//! Command `tasks` — the Valkey + Kafka + OpenTelemetry CRUD service. Loads
//! config (YAML + env), initialises tracing, connects its dependencies (failing
//! fast at boot if any is unreachable), registers their readiness checks and
//! serves the routes on the shared `httpx` scaffolding.

use tasks::{routes, AppState, Config, TaskStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::load()?;

    // Tracing first, so spans from dependency setup are captured. The guard
    // flushes buffered spans on drop at the end of `main`.
    let _tracing = otelx::init(&cfg.otelx(), &cfg.log_level, &cfg.log_format)?;

    tracing::info!(addr = %cfg.addr, topic = %cfg.kafka_topic, "tasks starting");

    let cache = valkey::Cache::connect(&cfg.valkey()).await?;
    let producer = kafka::Producer::connect(&cfg.kafka()).await?;
    let store = TaskStore::new(cache.clone());

    let server = httpx::Server::new(cfg.httpx());
    server.health.register("valkey", cache.readiness_check());
    server.health.register("kafka", producer.readiness_check());

    let state = AppState {
        store,
        producer,
        topic: cfg.kafka_topic.clone(),
    };

    server.with_routes(routes(state)).run().await
}

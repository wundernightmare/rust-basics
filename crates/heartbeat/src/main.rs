//! Command `heartbeat` — a minimal background worker built on `httpx`. It runs
//! a ticker loop (the "worker" shape of this workspace) while serving
//! `/healthz`, `/readyz` and `/metrics` from the shared scaffolding, so it is
//! observable like any other service. The HTTP server and the worker run
//! concurrently under one `tokio::select!`; when the server stops on a signal,
//! the worker is cancelled with it.

use heartbeat::{Config, Worker};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::from_env()?;
    httpx::init_tracing(&cfg.log_level, &cfg.log_format);
    tracing::info!(addr = %cfg.addr, interval_ms = cfg.interval_ms, "heartbeat starting");

    let server = httpx::Server::new(cfg.httpx());
    // The worker registers its counter on the server's registry, so beats show
    // up on /metrics alongside the HTTP metrics.
    let worker = Worker::new(cfg.interval(), &server.metrics.registry)?;

    tokio::select! {
        result = server.run() => result?,
        () = worker.run() => {}
    }

    tracing::info!("heartbeat stopped cleanly");
    Ok(())
}

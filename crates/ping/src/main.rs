//! Command `ping` — a minimal ping/pong HTTP service built on the shared
//! `httpx` scaffolding. Load config, build the server from the common crate,
//! merge ping's routes, and serve with graceful shutdown.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = httpx::Config::from_env("PING_")?;
    httpx::init_tracing(&cfg.log_level, &cfg.log_format);
    tracing::info!(addr = %cfg.addr, "ping starting");

    httpx::Server::new(cfg)
        .with_routes(ping::routes())
        .run()
        .await
}

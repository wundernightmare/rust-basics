//! Command `ping` — a minimal ping/pong HTTP service built on the shared
//! `httpx` scaffolding, demonstrating the `ratelimit` and `secrets` crates.

use std::sync::Arc;

use ping::Options;
use secrets::SecretString;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = httpx::Config::from_env("PING_")?;
    httpx::init_tracing(&cfg.log_level, &cfg.log_format);

    let opts = Options {
        rate_limit_rps: std::env::var("PING_RATE_LIMIT_RPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        api_key: std::env::var("PING_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|k| Arc::new(SecretString::from(k))),
    };

    tracing::info!(
        addr = %cfg.addr,
        rate_limit_rps = opts.rate_limit_rps,
        secure = opts.api_key.is_some(),
        "ping starting"
    );

    httpx::Server::new(cfg)
        .with_routes(ping::routes(opts))
        .run()
        .await
}

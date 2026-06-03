use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialise the global `tracing` subscriber writing to stdout.
///
/// `level` is one of `debug|info|warn|error` (falls back to `info`); `format`
/// is `text` for human-readable output or anything else for JSON (the default).
/// The `RUST_LOG` env var, if set, overrides `level`. Calling this more than
/// once is a no-op (the second init is ignored), which keeps tests safe.
pub fn init_tracing(level: &str, format: &str) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let registry = tracing_subscriber::registry().with(filter);

    if format.eq_ignore_ascii_case("text") {
        let _ = registry.with(tracing_subscriber::fmt::layer()).try_init();
    } else {
        let _ = registry
            .with(tracing_subscriber::fmt::layer().json())
            .try_init();
    }
}

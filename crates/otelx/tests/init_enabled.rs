//! Export-enabled path of `otelx::init`. Kept in its own test binary so the
//! process-global subscriber/tracer provider it installs is not pre-empted by
//! the disabled-path tests in `init.rs` (`try_init` keeps the first one).

use std::net::TcpListener;
use std::time::{Duration, Instant};

use otelx::Config;

/// A loopback endpoint nothing listens on: bind an ephemeral port, then free it.
fn dead_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

#[tokio::test(flavor = "multi_thread")]
async fn init_enabled_exports_and_shuts_down_without_collector() {
    let cfg = Config {
        otel_enabled: true,
        otel_service_name: "otelx-test".to_owned(),
        otel_endpoint: dead_endpoint(),
        otel_sampler_ratio: 1.0,
    };
    let guard = otelx::init(&cfg, "info", "json").expect("init with OTLP exporter");

    tracing::info_span!("otelx-test-span").in_scope(|| tracing::info!("inside span"));

    // Dropping the guard flushes the batch exporter against an unreachable
    // collector. The OTLP exporter retries failed exports by default since
    // 0.33, so make sure process shutdown still stays bounded.
    let started = Instant::now();
    tokio::task::spawn_blocking(move || drop(guard))
        .await
        .expect("guard drop must not panic");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(15),
        "shutdown without a collector took {elapsed:?}"
    );
    eprintln!("shutdown without collector: {elapsed:?}");
}

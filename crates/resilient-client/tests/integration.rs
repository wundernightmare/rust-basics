//! Integration tests for `ResilientHttpClient`, driven against a local axum
//! mock server whose routes simulate success, 4xx, 5xx, flakiness and slowness.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::{http::StatusCode, routing::get, Router};
use resilient_client::{
    ClientConfig, Metrics, OutboundRequest, OutboundTargetConfig, ResilientHttpClient,
    ResourceGroup,
};

/// Spawn a mock upstream on an ephemeral port. Returns its base URL and a hit
/// counter for the `/flaky` route.
async fn spawn_server() -> (String, Arc<AtomicU32>) {
    let flaky_hits = Arc::new(AtomicU32::new(0));
    let hits = flaky_hits.clone();

    let app = Router::new()
        .route("/ok", get(|| async { "ok" }))
        .route("/fail", get(|| async { StatusCode::INTERNAL_SERVER_ERROR }))
        .route("/notfound", get(|| async { StatusCode::NOT_FOUND }))
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(300)).await;
                "slow"
            }),
        )
        .route(
            "/flaky",
            get(move || {
                let hits = hits.clone();
                async move {
                    // 503 for the first two hits, then 200.
                    if hits.fetch_add(1, Ordering::SeqCst) < 2 {
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        StatusCode::OK
                    }
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), flaky_hits)
}

fn client(target: OutboundTargetConfig) -> ResilientHttpClient {
    let cfg = ClientConfig {
        outbound_targets: vec![target],
        ..ClientConfig::default()
    };
    ResilientHttpClient::new(cfg, Arc::new(Metrics::new().unwrap())).unwrap()
}

fn target(name: &str) -> OutboundTargetConfig {
    OutboundTargetConfig {
        name: name.to_owned(),
        rate_limit: 1000,
        ..OutboundTargetConfig::default()
    }
}

#[tokio::test]
async fn ok_request_succeeds() {
    let (base, _) = spawn_server().await;
    let c = client(target("t"));
    let resp = c
        .send(OutboundRequest::get(
            ResourceGroup::new("t"),
            format!("{base}/ok"),
        ))
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn server_error_is_transient() {
    let (base, _) = spawn_server().await;
    let c = client(target("t"));
    let err = c
        .send(OutboundRequest::get(
            ResourceGroup::new("t"),
            format!("{base}/fail"),
        ))
        .await
        .unwrap_err();
    assert!(err.is_transient(), "{err}");
}

#[tokio::test]
async fn client_error_is_fatal() {
    let (base, _) = spawn_server().await;
    let c = client(target("t"));
    let err = c
        .send(OutboundRequest::get(
            ResourceGroup::new("t"),
            format!("{base}/notfound"),
        ))
        .await
        .unwrap_err();
    assert!(err.is_fatal(), "{err}");
}

#[tokio::test]
async fn timeout_is_transient() {
    let (base, _) = spawn_server().await;
    let c = client(OutboundTargetConfig {
        timeout_ms: Some(50),
        ..target("t")
    });
    let err = c
        .send(OutboundRequest::get(
            ResourceGroup::new("t"),
            format!("{base}/slow"),
        ))
        .await
        .unwrap_err();
    assert!(err.is_transient(), "{err}");
    assert!(err.to_string().contains("timed out"), "{err}");
}

#[tokio::test]
async fn retry_recovers_from_transient_failures() {
    let (base, hits) = spawn_server().await;
    let c = client(OutboundTargetConfig {
        retry_max_attempts: 5,
        retry_base_ms: 1,
        retry_cap_ms: 5,
        ..target("t")
    });
    let resp = c
        .send_with_retry(OutboundRequest::get(
            ResourceGroup::new("t"),
            format!("{base}/flaky"),
        ))
        .await
        .unwrap();
    assert!(resp.status().is_success());
    assert!(hits.load(Ordering::SeqCst) >= 3, "expected ≥3 attempts");
}

#[tokio::test]
async fn local_rate_limit_rejects_excess() {
    let (base, _) = spawn_server().await;
    let c = client(OutboundTargetConfig {
        rate_limit: 1,
        ..target("t")
    });
    let mut rate_limited = false;
    for _ in 0..5 {
        if let Err(e) = c
            .send(OutboundRequest::get(
                ResourceGroup::new("t"),
                format!("{base}/ok"),
            ))
            .await
        {
            if e.to_string().contains("rate limited") {
                rate_limited = true;
            }
        }
    }
    assert!(
        rate_limited,
        "expected at least one local rate-limit rejection"
    );
}

#[tokio::test]
async fn shutdown_drains_then_rejects() {
    let (base, _) = spawn_server().await;
    let c = Arc::new(client(target("t")));

    // A clean shutdown with nothing in flight returns immediately.
    c.shutdown(Duration::from_secs(1)).await.unwrap();

    // After shutdown, new requests are rejected as transient.
    let err = c
        .send(OutboundRequest::get(
            ResourceGroup::new("t"),
            format!("{base}/ok"),
        ))
        .await
        .unwrap_err();
    assert!(err.is_transient(), "{err}");
    assert!(err.to_string().contains("shutting down"), "{err}");
}

//! Integration tests for the shared httpx server, driven through the assembled
//! router via `tower::ServiceExt::oneshot` (no socket bind needed).

use axum::{
    body::{self, Body},
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use httpx::{Config, Server};
use tower::ServiceExt;

fn test_config() -> Config {
    // Defaults are fine; the router never reads `addr` (only `run` binds).
    Config {
        addr: "127.0.0.1:0".to_owned(),
        shutdown_timeout_secs: 1,
        log_level: "error".to_owned(),
        log_format: "text".to_owned(),
    }
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn healthz_is_always_ok() {
    let server = Server::new(test_config());
    let resp = server
        .router()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, r#"{"status":"ok"}"#);
}

#[tokio::test]
async fn readyz_gate_closed_then_open() {
    let server = Server::new(test_config());

    // Gate closed → 503.
    let resp = server
        .router()
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    // Open the gate → 200.
    server.health.set_ready(true);
    let resp = server
        .router()
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_string(resp).await.contains(r#""status":"ready""#));
}

#[tokio::test]
async fn readyz_failing_check_is_degraded() {
    let server = Server::new(test_config());
    server.health.set_ready(true);
    server
        .health
        .register("db", || Err("connection refused".to_owned()));
    server.health.register("cache", || Ok(()));

    let resp = server
        .router()
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_string(resp).await;
    assert!(body.contains(r#""status":"degraded""#), "{body}");
    assert!(body.contains("connection refused"), "{body}");
    assert!(body.contains(r#""cache":"ok""#), "{body}");
}

#[tokio::test]
async fn metrics_records_requests() {
    let user = Router::new().route("/ping", get(|| async { "pong" }));
    let server = Server::new(test_config()).with_routes(user);

    // Drive one request so the counter is non-zero.
    let resp = server
        .router()
        .oneshot(Request::get("/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Scrape /metrics (shares the same Metrics registry).
    let resp = server
        .router()
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("http_requests_total"), "{body}");
    assert!(body.contains(r#"path="/ping""#), "{body}");
    assert!(body.contains("http_request_duration_seconds"), "{body}");
}

#[test]
fn config_from_env_defaults_and_prefix() {
    // Defaults (no vars set for this prefix).
    let cfg = Config::from_env("HTTPX_TEST_UNSET_").unwrap();
    assert_eq!(cfg.addr, "0.0.0.0:8080");
    assert_eq!(cfg.shutdown_timeout_secs, 10);
    assert_eq!(cfg.log_format, "json");

    // Prefixed override.
    std::env::set_var("HTTPX_TEST_ADDR", "127.0.0.1:9999");
    std::env::set_var("HTTPX_TEST_LOG_LEVEL", "debug");
    let cfg = Config::from_env("HTTPX_TEST_").unwrap();
    assert_eq!(cfg.addr, "127.0.0.1:9999");
    assert_eq!(cfg.log_level, "debug");
    std::env::remove_var("HTTPX_TEST_ADDR");
    std::env::remove_var("HTTPX_TEST_LOG_LEVEL");
}

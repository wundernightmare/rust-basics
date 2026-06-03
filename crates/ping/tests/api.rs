use std::sync::Arc;

use axum::{
    body::{self, Body},
    http::{Request, StatusCode},
};
use ping::{routes, Options, PongResponse, VersionResponse};
use secrets::SecretString;
use tower::ServiceExt;

fn app() -> axum::Router {
    routes(Options::default())
}

async fn json<T: serde::de::DeserializeOwned>(resp: axum::response::Response) -> T {
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn ping_returns_pong() {
    let resp = app()
        .oneshot(Request::get("/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: PongResponse = json(resp).await;
    assert_eq!(body.message, "pong");
    assert!(body.echo.is_none());
}

#[tokio::test]
async fn ping_echoes_msg() {
    let resp = app()
        .oneshot(Request::get("/ping?msg=hello").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: PongResponse = json(resp).await;
    assert_eq!(body.echo.as_deref(), Some("hello"));
}

#[tokio::test]
async fn version_reports_service() {
    let resp = app()
        .oneshot(Request::get("/version").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: VersionResponse = json(resp).await;
    assert_eq!(body.service, "ping");
    assert!(!body.version.is_empty());
}

#[tokio::test]
async fn unknown_route_is_404() {
    let resp = app()
        .oneshot(Request::get("/nope").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rate_limit_returns_429_after_burst() {
    // 2/s burst; the same client key ("global", no XFF header) is throttled.
    let app = routes(Options {
        rate_limit_rps: 2,
        ..Options::default()
    });
    let mut statuses = Vec::new();
    for _ in 0..4 {
        let resp = app
            .clone()
            .oneshot(Request::get("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();
        statuses.push(resp.status());
    }
    assert!(
        statuses.contains(&StatusCode::TOO_MANY_REQUESTS),
        "expected a 429 within the burst, got {statuses:?}"
    );
}

#[tokio::test]
async fn secure_route_checks_bearer_token() {
    let app = routes(Options {
        api_key: Some(Arc::new(SecretString::from("s3cr3t".to_owned()))),
        ..Options::default()
    });

    let ok = app
        .clone()
        .oneshot(
            Request::get("/secure")
                .header("authorization", "Bearer s3cr3t")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);

    let wrong = app
        .clone()
        .oneshot(
            Request::get("/secure")
                .header("authorization", "Bearer nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    let missing = app
        .oneshot(Request::get("/secure").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn secure_route_absent_without_key() {
    let resp = app()
        .oneshot(Request::get("/secure").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

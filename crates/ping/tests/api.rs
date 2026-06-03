use axum::{
    body::{self, Body},
    http::{Request, StatusCode},
};
use ping::{routes, PongResponse, VersionResponse};
use tower::ServiceExt;

async fn json<T: serde::de::DeserializeOwned>(resp: axum::response::Response) -> T {
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn ping_returns_pong() {
    let resp = routes()
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
    let resp = routes()
        .oneshot(Request::get("/ping?msg=hello").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: PongResponse = json(resp).await;
    assert_eq!(body.message, "pong");
    assert_eq!(body.echo.as_deref(), Some("hello"));
}

#[tokio::test]
async fn version_reports_service() {
    let resp = routes()
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
    let resp = routes()
        .oneshot(Request::get("/nope").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

//! ping — the service's HTTP surface, kept thin: cross-cutting concerns
//! (logging, metrics, health, shutdown) live in `httpx`. This crate also shows
//! how the shared library crates plug in:
//!
//! - [`ratelimit`] guards `/ping` (and `/version`) with a per-client GCRA limit;
//! - [`secrets`] backs an optional Bearer-protected `/secure` route, holding the
//!   key as a redacted [`secrets::SecretString`] and comparing in constant time.
//!
//! Split into a lib so the routes are testable without spawning the binary.

#![allow(clippy::must_use_candidate)]

use std::sync::Arc;

use axum::{
    extract::{Query, Request, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Json, Response},
    routing::get,
    Extension, Router,
};
use ratelimit::Limiter;
use secrets::{constant_time_eq, ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// How the ping routes are wired. Build from the environment in `main`.
#[derive(Clone, Default)]
pub struct Options {
    /// Per-client request/second limit on `/ping`. `0` disables rate limiting.
    pub rate_limit_rps: u32,
    /// Optional Bearer key enabling `/secure`. Held redacted; never logged.
    pub api_key: Option<Arc<SecretString>>,
}

/// Query string for `GET /ping` (`?msg=…`).
#[derive(Debug, Deserialize)]
pub struct PingQuery {
    pub msg: Option<String>,
}

/// Body returned by `GET /ping`.
#[derive(Debug, Serialize, Deserialize)]
pub struct PongResponse {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub echo: Option<String>,
}

/// Body returned by `GET /version`.
#[derive(Debug, Serialize, Deserialize)]
pub struct VersionResponse {
    pub service: String,
    pub version: String,
    pub build: String,
}

#[derive(Clone)]
struct ApiKey(Arc<SecretString>);

/// Build the ping service's routes, to be merged onto an [`httpx::Server`].
pub fn routes(opts: Options) -> Router {
    let mut router = Router::new()
        .route("/ping", get(pong))
        .route("/version", get(version));

    if let Some(key) = opts.api_key {
        router = router
            .route("/secure", get(secure))
            .layer(Extension(ApiKey(key)));
    }

    if opts.rate_limit_rps > 0 {
        let limiter = Arc::new(Limiter::<String>::per_second(opts.rate_limit_rps));
        router = router.route_layer(from_fn_with_state(limiter, rate_limit));
    }

    router
}

async fn pong(Query(q): Query<PingQuery>) -> Json<PongResponse> {
    Json(PongResponse {
        message: "pong".to_owned(),
        echo: q.msg.filter(|s| !s.is_empty()),
    })
}

async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        service: "ping".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        build: option_env!("GIT_SHA").unwrap_or("unknown").to_owned(),
    })
}

/// Bearer-token gate backed by [`secrets`]. Compares the supplied token against
/// the configured key in constant time.
async fn secure(Extension(key): Extension<ApiKey>, headers: HeaderMap) -> Response {
    let provided = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    match provided {
        Some(token) if constant_time_eq(token.as_bytes(), key.0.expose_secret().as_bytes()) => {
            (StatusCode::OK, Json(json!({ "status": "authorized" }))).into_response()
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "status": "unauthorized" })),
        )
            .into_response(),
    }
}

/// Per-client GCRA rate limit. The key is the first `X-Forwarded-For` /
/// `X-Real-IP` value (so it works behind a proxy), falling back to a shared
/// bucket when neither header is present.
async fn rate_limit(
    State(limiter): State<Arc<Limiter<String>>>,
    req: Request,
    next: Next,
) -> Response {
    let key = client_key(req.headers());
    if limiter.check(&key).is_err() {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "rate limited" })),
        )
            .into_response();
    }
    next.run(req).await
}

fn client_key(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_owned())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "global".to_owned())
}

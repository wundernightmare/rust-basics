//! ping — the service's HTTP surface, kept thin: all cross-cutting concerns
//! (logging, metrics, health, shutdown) live in the shared `httpx` crate, so
//! this is only ping's own routes. Split into a lib so the routes are testable
//! without spawning the binary.

#![allow(clippy::must_use_candidate)]

use axum::{extract::Query, response::Json, routing::get, Router};
use serde::{Deserialize, Serialize};

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

/// The ping service's routes, to be merged onto an [`httpx::Server`].
pub fn routes() -> Router {
    Router::new()
        .route("/ping", get(pong))
        .route("/version", get(version))
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

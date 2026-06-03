use std::net::SocketAddr;
use std::time::Instant;

use axum::{
    extract::{Extension, MatchedPath, Request},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde_json::json;
use tower_http::trace::TraceLayer;

use crate::{Config, Health, Metrics};

/// Wires an [`axum::Router`] together with the cross-cutting concerns every
/// service shares: request tracing, Prometheus metrics, and health endpoints.
/// Services call [`Server::with_routes`] to add their own routes, then
/// [`Server::run`] to serve with graceful shutdown.
pub struct Server {
    config: Config,
    routes: Router,
    /// The server's metrics — register service-specific collectors on
    /// `metrics.registry` before calling [`Server::run`].
    pub metrics: Metrics,
    /// The readiness registry — add checks before [`Server::run`].
    pub health: Health,
}

impl Server {
    /// Construct a server from `config`. The final router (with `/healthz`,
    /// `/readyz`, `/metrics`, the metrics middleware and a trace layer) is
    /// assembled in [`Server::run`].
    pub fn new(config: Config) -> Self {
        Self {
            config,
            routes: Router::new(),
            metrics: Metrics::new(),
            health: Health::new(),
        }
    }

    /// Merge service routes onto the server.
    #[must_use]
    pub fn with_routes(mut self, routes: Router) -> Self {
        self.routes = self.routes.merge(routes);
        self
    }

    /// Build the final axum router: service routes + `/healthz` `/readyz`
    /// `/metrics`, the metrics middleware, the metrics/health extensions and a
    /// trace layer. [`Server::run`] uses this internally; it is public so tests
    /// can drive the assembled app without binding a socket.
    pub fn router(&self) -> Router {
        self.routes
            .clone()
            .route("/healthz", get(live))
            .route("/readyz", get(ready))
            .route("/metrics", get(metrics_handler))
            .route_layer(middleware::from_fn(track_metrics))
            .layer(Extension(self.metrics.clone()))
            .layer(Extension(self.health.clone()))
            .layer(TraceLayer::new_for_http())
    }

    /// Bind the listener and serve until SIGINT/SIGTERM, then drain in-flight
    /// requests. A clean shutdown returns `Ok(())`.
    pub async fn run(self) -> anyhow::Result<()> {
        let app = self.router();

        let addr: SocketAddr = self
            .config
            .addr
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid listen addr {:?}: {e}", self.config.addr))?;

        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(%addr, "http server listening");
        self.health.set_ready(true);

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal(self.health.clone()))
            .await?;

        tracing::info!("http server stopped cleanly");
        Ok(())
    }
}

// ── built-in handlers ─────────────────────────────────────────────────────────

async fn live() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn ready(Extension(health): Extension<Health>) -> Response {
    if !health.is_ready() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "not_ready" })),
        )
            .into_response();
    }

    let (ok, checks) = health.run_checks();
    let (status, label) = if ok {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "degraded")
    };
    (status, Json(json!({ "status": label, "checks": checks }))).into_response()
}

async fn metrics_handler(Extension(metrics): Extension<Metrics>) -> impl IntoResponse {
    metrics.render()
}

// ── metrics middleware ────────────────────────────────────────────────────────

async fn track_metrics(
    Extension(metrics): Extension<Metrics>,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().as_str().to_owned();
    // Label by the matched route template (not the raw URI) so path params
    // never explode the metric cardinality.
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |m| m.as_str().to_owned());

    let start = Instant::now();
    let resp = next.run(req).await;
    metrics.record(
        &method,
        &path,
        resp.status().as_u16(),
        start.elapsed().as_secs_f64(),
    );
    resp
}

// ── graceful shutdown ─────────────────────────────────────────────────────────

async fn shutdown_signal(health: Health) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install ctrl_c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("shutdown signal received, draining");
    health.set_ready(false);
}

//! `httpx` — shared axum HTTP scaffolding for rust-basics services.
//!
//! The example "common crate" of this workspace (the Rust analogue of the
//! `libs/httpx` Go package in the sibling repo, and modelled on tracehub-edge's
//! shared telemetry/worker crates). It bundles the boilerplate every service
//! repeats:
//!
//! - a configured [`axum::Router`] with request tracing and a metrics
//!   middleware (see [`Server`]);
//! - Prometheus metrics on `/metrics` plus per-request count/latency
//!   recording (see [`Metrics`]);
//! - liveness (`/healthz`) and readiness (`/readyz`) endpoints backed by a
//!   pluggable check registry (see [`Health`]);
//! - environment-driven configuration with a per-service prefix (see
//!   [`Config`]);
//! - structured logging via `tracing` (see [`init_tracing`]);
//! - graceful shutdown wired to SIGINT/SIGTERM (see [`Server::run`]).
//!
//! ```no_run
//! # async fn run() -> anyhow::Result<()> {
//! use axum::{routing::get, Router};
//!
//! let cfg = httpx::Config::from_env("PING_")?;
//! httpx::init_tracing(&cfg.log_level, &cfg.log_format);
//! let routes = Router::new().route("/ping", get(|| async { "pong" }));
//! httpx::Server::new(cfg).with_routes(routes).run().await
//! # }
//! ```

// Pedantic doc-noise lints: keep the rest of `clippy::pedantic` denied (see the
// justfile), but silence the three that just demand boilerplate docs/attrs on a
// small internal crate.
#![allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions
)]

mod config;
mod health;
mod logging;
mod metrics;
mod problem;
mod server;

pub use config::{load_yaml, Config};
pub use health::Health;
pub use logging::init_tracing;
pub use metrics::Metrics;
pub use problem::{Problem, PROBLEM_CONTENT_TYPE};
pub use server::Server;

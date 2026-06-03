//! heartbeat — a background ticker worker (the "worker" shape of this
//! workspace, the analogue of a Kafka-consumer crate in tracehub-edge, minus
//! the broker). It still reuses the shared `httpx` server for its `/healthz`,
//! `/readyz` and `/metrics` surface. Split into a lib so the worker is testable.

#![allow(clippy::must_use_candidate, clippy::missing_errors_doc)]

mod config;
mod worker;

pub use config::Config;
pub use worker::Worker;

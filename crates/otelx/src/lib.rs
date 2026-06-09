//! `otelx` — shared OpenTelemetry tracing setup for rust-basics services.
//!
//! The tracing analogue of `httpx`'s metrics, and the Rust counterpart of the
//! Go sibling's `libs/otelx`. A service calls [`init`] once at boot to wire the
//! `tracing` subscriber (env-filter + JSON/text fmt layer) together with an
//! OTLP/gRPC OpenTelemetry exporter and the W3C trace-context propagator; the
//! `TraceLayer` that `httpx::Server` already installs then turns every request
//! into an exported span.
//!
//! It is kept out of `httpx` on purpose: `ping`/`heartbeat` stay free of the
//! `OpenTelemetry` dependency tree, while services that span process boundaries
//! (`tasks` → Kafka → `consumer`) opt in by calling [`init`] instead of
//! [`httpx::init_tracing`]. Export is also opt-in at runtime — with
//! `OTEL_ENABLED=false` (the default) only the fmt layer and propagator are
//! installed, so the same binary runs with or without a collector.

#![allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions
)]

mod config;

pub use config::Config;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace::{Sampler, TracerProvider};
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Holds the tracer provider so its batch exporter is flushed and shut down on
/// drop. Keep it alive for the lifetime of the process (bind it in `main`); when
/// tracing export is disabled it carries nothing.
pub struct TracingGuard {
    provider: Option<TracerProvider>,
}

impl Drop for TracingGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take() {
            // Flush buffered spans before exit.
            let _ = provider.shutdown();
        }
    }
}

/// Install the global `tracing` subscriber and the W3C trace-context
/// propagator. `log_level`/`log_format` mirror [`httpx::init_tracing`]
/// (`debug|info|warn|error`, `json|text`); when `cfg.otel_enabled` is set, spans
/// are additionally exported to the OTLP endpoint. Calling this more than once
/// is a no-op (the second init is ignored), keeping tests safe.
pub fn init(cfg: &Config, log_level: &str, log_format: &str) -> anyhow::Result<TracingGuard> {
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = if log_format.eq_ignore_ascii_case("text") {
        tracing_subscriber::fmt::layer().boxed()
    } else {
        tracing_subscriber::fmt::layer().json().boxed()
    };

    let registry = tracing_subscriber::registry().with(filter).with(fmt_layer);

    if !cfg.otel_enabled {
        let _ = registry.try_init();
        tracing::info!(service = %cfg.otel_service_name, "tracing disabled (propagation only)");
        return Ok(TracingGuard { provider: None });
    }

    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&cfg.otel_endpoint)
        .build()?;

    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, runtime::Tokio)
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            cfg.otel_sampler_ratio,
        ))))
        .with_resource(Resource::new(vec![
            KeyValue::new(SERVICE_NAME, cfg.otel_service_name.clone()),
            KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
        ]))
        .build();

    let tracer = provider.tracer("rust-basics");
    opentelemetry::global::set_tracer_provider(provider.clone());
    let _ = registry
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .try_init();

    tracing::info!(
        service = %cfg.otel_service_name,
        endpoint = %cfg.otel_endpoint,
        sampler_ratio = cfg.otel_sampler_ratio,
        "tracing enabled"
    );
    Ok(TracingGuard {
        provider: Some(provider),
    })
}

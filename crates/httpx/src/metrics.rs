use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
};

/// Owns a private Prometheus registry plus the standard HTTP request metrics.
/// Each [`crate::Server`] gets its own `Metrics`, so multiple servers in one
/// process never fight over the global default registry. Cloning shares the
/// underlying registry and metric vectors (they are `Arc`-backed).
#[derive(Clone)]
pub struct Metrics {
    pub registry: Registry,
    req_total: IntCounterVec,
    req_duration: HistogramVec,
}

impl Metrics {
    /// Build a registry pre-populated with process metrics and the two
    /// canonical HTTP metrics:
    ///
    /// ```text
    /// http_requests_total{method,path,status}
    /// http_request_duration_seconds{method,path,status}
    /// ```
    ///
    /// # Panics
    /// Panics only if the metric descriptors are invalid — a programmer error
    /// caught immediately in tests, never at runtime with these constants.
    pub fn new() -> Self {
        let registry = Registry::new();

        let req_total = IntCounterVec::new(
            Opts::new(
                "http_requests_total",
                "Total number of HTTP requests handled, by method, route and status.",
            ),
            &["method", "path", "status"],
        )
        .expect("valid counter opts");

        let req_duration = HistogramVec::new(
            HistogramOpts::new(
                "http_request_duration_seconds",
                "HTTP request latency in seconds, by method, route and status.",
            ),
            &["method", "path", "status"],
        )
        .expect("valid histogram opts");

        registry
            .register(Box::new(req_total.clone()))
            .expect("register http_requests_total");
        registry
            .register(Box::new(req_duration.clone()))
            .expect("register http_request_duration_seconds");

        // Process-level metrics (process_cpu_seconds_total, _resident_memory_…).
        // Best-effort: on platforms without /proc this registration is skipped.
        let _ = registry.register(Box::new(
            prometheus::process_collector::ProcessCollector::for_self(),
        ));

        Self {
            registry,
            req_total,
            req_duration,
        }
    }

    /// Record one finished request.
    pub fn record(&self, method: &str, path: &str, status: u16, elapsed_secs: f64) {
        let status = status.to_string();
        self.req_total
            .with_label_values(&[method, path, &status])
            .inc();
        self.req_duration
            .with_label_values(&[method, path, &status])
            .observe(elapsed_secs);
    }

    /// Render the registry in the Prometheus text exposition format.
    ///
    /// # Panics
    /// Panics only if the registered metrics produce non-UTF-8 output, which
    /// the prometheus text encoder never does.
    pub fn render(&self) -> String {
        let mut buf = Vec::new();
        TextEncoder::new()
            .encode(&self.registry.gather(), &mut buf)
            .expect("encode metrics");
        String::from_utf8(buf).expect("metrics are valid UTF-8")
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

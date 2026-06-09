use otelx::Config;

#[test]
fn config_defaults() {
    let cfg = Config::default();
    assert!(!cfg.otel_enabled);
    assert_eq!(cfg.otel_endpoint, "http://localhost:4317");
    assert!((cfg.otel_sampler_ratio - 1.0).abs() < f64::EPSILON);
}

#[test]
fn from_env_reads_prefix() {
    // SAFETY: single-threaded test; no other thread reads the env concurrently.
    unsafe {
        std::env::set_var("TASKS_OTEL_ENABLED", "true");
        std::env::set_var("TASKS_OTEL_SERVICE_NAME", "tasks");
    }
    let cfg = Config::from_env("TASKS_").unwrap();
    assert!(cfg.otel_enabled);
    assert_eq!(cfg.otel_service_name, "tasks");
    unsafe {
        std::env::remove_var("TASKS_OTEL_ENABLED");
        std::env::remove_var("TASKS_OTEL_SERVICE_NAME");
    }
}

#[test]
fn init_disabled_installs_propagator_and_returns_guard() {
    // Disabled path needs no tokio runtime and no collector.
    let guard = otelx::init(&Config::default(), "info", "json").expect("init");
    drop(guard); // no panic on shutdown
}

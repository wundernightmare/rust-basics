use std::time::Duration;

use heartbeat::Worker;
use prometheus::{Encoder, Registry, TextEncoder};

/// Read the `heartbeat_beats_total` value back through the registry, exactly as
/// the `/metrics` endpoint would — the Worker keeps the counter private.
fn beats(registry: &Registry) -> f64 {
    let mut buf = Vec::new();
    TextEncoder::new()
        .encode(&registry.gather(), &mut buf)
        .unwrap();
    String::from_utf8(buf)
        .unwrap()
        .lines()
        .find(|l| l.starts_with("heartbeat_beats_total "))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .expect("heartbeat_beats_total present")
}

#[tokio::test]
async fn worker_beats_and_increments() {
    let registry = Registry::new();
    let worker = Worker::new(Duration::from_millis(20), &registry, None).unwrap();

    let handle = tokio::spawn(worker.run());
    tokio::time::sleep(Duration::from_millis(130)).await;
    handle.abort();

    let value = beats(&registry);
    assert!(
        value >= 2.0,
        "expected at least a couple of beats, got {value}"
    );
}

#[test]
fn duplicate_registration_errors() {
    // Two workers on the same registry would double-register the counter; the
    // second must fail rather than panic.
    let registry = Registry::new();
    assert!(Worker::new(Duration::from_secs(1), &registry, None).is_ok());
    assert!(Worker::new(Duration::from_secs(1), &registry, None).is_err());
}

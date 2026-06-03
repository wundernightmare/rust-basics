use std::time::Duration;

use ratelimit::Limiter;

#[test]
fn allows_burst_then_limits_per_key() {
    let limiter: Limiter<&str> = Limiter::per_second(2);
    assert!(limiter.check(&"alice").is_ok());
    assert!(limiter.check(&"alice").is_ok());
    assert!(
        limiter.check(&"alice").is_err(),
        "3rd within the second is limited"
    );
}

#[test]
fn keys_have_independent_budgets() {
    let limiter: Limiter<u32> = Limiter::per_second(1);
    assert!(limiter.check(&1).is_ok());
    assert!(limiter.check(&1).is_err());
    assert!(
        limiter.check(&2).is_ok(),
        "a different key has its own budget"
    );
}

#[tokio::test]
async fn until_ready_resolves_after_throttle() {
    // 100/s ⇒ ~10ms between cells; until_ready should return quickly.
    let limiter: Limiter<&str> = Limiter::per_second(100);
    assert!(limiter.check(&"k").is_ok()); // consume the initial cell

    let waited = tokio::time::timeout(Duration::from_secs(2), limiter.until_ready(&"k")).await;
    assert!(waited.is_ok(), "until_ready should resolve well within 2s");
}

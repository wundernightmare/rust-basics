# ratelimit

A keyed GCRA rate limiter built on [`governor`](https://docs.rs/governor).
GCRA (the Generic Cell Rate Algorithm) smooths bursts to a sustained rate using
lock-free atomics — no background sweep, no mutex. In tracehub-edge this lives
inside `resilient-http-client`; here it's a standalone, generic crate keyed by
anything `Hash + Eq + Clone` (an IP, an API token, a user id).

## Usage

```rust
use ratelimit::Limiter;

let limiter: Limiter<&str> = Limiter::per_second(2);
assert!(limiter.check(&"alice").is_ok());   // 1st — allowed
assert!(limiter.check(&"alice").is_ok());   // 2nd — allowed (burst of 2)
assert!(limiter.check(&"alice").is_err());  // 3rd — limited
assert!(limiter.check(&"bob").is_ok());     // separate key, own budget
```

| Method                 | Behaviour                                               |
| ---------------------- | ------------------------------------------------------- |
| `per_second(n)` / `per_minute(n)` | constructors (n clamped to ≥ 1)              |
| `with_quota(quota)`    | from a full `governor::Quota` (custom burst, etc.)      |
| `check(&key)`          | non-blocking: `Ok` if a cell is free, else `RateLimited`|
| `until_ready(&key).await` | async: wait (throttle) instead of reject             |
| `retain_recent()`      | reclaim memory for recovered keys (unbounded key spaces)|

Used by [`ping`](../ping) to guard `/ping` per client IP (429 on exceed).

## Develop

```sh
just test
just lint
```

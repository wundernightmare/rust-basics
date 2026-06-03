# resilient-client

A policy-per-target HTTP client — a focused port of tracehub-edge's
`resilient-http-client` (core subset; the optional DNS-cache / adaptive-
concurrency / response-cache adapters are out of scope here).

A shared [`reqwest`](https://docs.rs/reqwest) connection pool is fronted, per
logical target, by:

| Concern         | Mechanism                                                        |
| --------------- | --------------------------------------------------------------- |
| Rate limiting   | lock-free GCRA via [`governor`] (smooths bursts to a sustained rate) |
| Circuit breaking| lock-free atomics; Closed → Open → HalfOpen with a sliding window |
| Retry           | full-jitter exponential backoff, **transient errors only**      |
| Timeout         | per-target, overrides the client default                        |
| Metrics         | Prometheus on a private registry                                |
| Shutdown        | drains in-flight requests within a deadline                     |

Errors split into [`OutboundError::Transient`] (retry later) and
[`OutboundError::Fatal`] (discard) so callers route failures sensibly.

## Usage

```rust,no_run
use resilient_client::{ClientConfig, Metrics, OutboundRequest, ResilientHttpClient, ResourceGroup};
use std::sync::Arc;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let cfg: ClientConfig = serde_yaml_ng::from_str(r#"
default_timeout_ms: 2000
outbound_targets:
  - name: "upstream"
    rate_limit: 500          # req/s (GCRA)
    cb_threshold: 0.5        # open at ≥50% failures
    retry_max_attempts: 3
"#)?;

let client = ResilientHttpClient::new(cfg, Arc::new(Metrics::new()?))?;
let resp = client
    .send_with_retry(OutboundRequest::get(ResourceGroup::new("upstream"), "https://example.com/health"))
    .await?;
# let _ = resp; Ok(())
# }
```

Unknown target names fall back to a default policy. Config is YAML
(`serde_yaml_ng`); every field has a default.

## Develop

```sh
just test     # unit + integration (against a local axum mock server)
just lint     # clippy -D pedantic/perf/suspicious
```

[`governor`]: https://docs.rs/governor

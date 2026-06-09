# rust-basics

A small, idiomatic **Rust monorepo** modelled directly on the `tracehub-edge`
workspace (and the Go-flavoured `golang-basics` sibling): a Cargo workspace of
service crates + a shared library crate, a `just`-driven build/test/lint/
security/CI surface inherited from tracehub-edge, per-crate justfiles,
multi-stage **cargo-chef** distroless Docker images, a Playwright e2e suite, and
k6 load tests. It's a learning / template scaffold, wired up the way the real
edge workspace is.

It comes in two layers: **dependency-free** building blocks (the `ping` service,
the `heartbeat` worker and the `httpx`/`resilient-client`/`ratelimit`/`secrets`
libs) and a **data-services** vertical that adds real infrastructure — the
`tasks` CRUD service (Valkey store + Kafka producer + OpenTelemetry + RFC 9457
errors; **no relational database** in this workspace) and the `consumer` worker
that drains the events `tasks` produces — backed by the `valkey`/`kafka`/`otelx`
crates.

It lives in a **bare-repo worktree container** with sccache as the shared build
cache (see [Worktrees](#worktrees-multi-branch-dev)), exactly like tracehub-edge.

---

## Crates

| Crate                         | Kind         | Port(s)                   | One-liner                                                                                         |
| ----------------------------- | ------------ | ------------------------- | ------------------------------------------------------------------------------------------------- |
| [`ping`](crates/ping)             | HTTP service | `:8080`                   | axum ping/pong service (`?msg=` echo, `/version`); rate-limited + optional `/secure`.             |
| [`heartbeat`](crates/heartbeat)   | Worker       | `:8081` (health/metrics)  | tokio ticker worker — bumps `heartbeat_beats_total` each interval; optional upstream health check. |
| [`httpx`](crates/httpx)           | Library      | —                         | Shared axum scaffolding: server + tracing, Prometheus metrics, health, graceful shutdown, config. |
| [`resilient-client`](crates/resilient-client) | Library | —              | Policy-per-target HTTP client: GCRA rate limiting, circuit breaking, jittered retry, timeouts.    |
| [`ratelimit`](crates/ratelimit)   | Library      | —                         | Keyed GCRA rate limiter (`governor`); `check` (reject) + `until_ready` (throttle).                |
| [`secrets`](crates/secrets)       | Library      | —                         | AES-256-GCM seal/open, auto-redacting `SecretString`, lock-free secret cache, constant-time compare. |
| [`tasks`](crates/tasks)           | HTTP service | `:8082`                   | Tasks CRUD over **Valkey + Kafka**, traced, with `problem+json` errors. Publishes `task.created`.  |
| [`consumer`](crates/consumer)     | Worker       | `:8083` (health/metrics)  | Kafka consumer draining `tasks.events`; bumps `consumer_tasks_consumed_total`.                     |
| [`valkey`](crates/valkey)         | Library      | —                         | Valkey cache (`redis`-rs against a Valkey server): typed/JSON get/set/del, set index, readiness probe. |
| [`kafka`](crates/kafka)           | Library      | —                         | Kafka producer + consumer (`rdkafka`): awaited publish, at-least-once consumer-group loop, readiness. |
| [`otelx`](crates/otelx)           | Library      | —                         | OpenTelemetry tracing: OTLP exporter + `tracing` subscriber, W3C propagation (opt-in).             |

The dependency graph: every service → `httpx`; `ping` → `ratelimit` + `secrets`;
`heartbeat` → `resilient-client`; `tasks` → `valkey` + `kafka` + `otelx`;
`consumer` → `kafka` + `otelx`. Every service reuses `httpx` for its `/healthz`,
`/readyz` and `/metrics` surface, so a worker is as observable as a server.
`ping`/`heartbeat` stay dependency-free; `tasks`/`consumer` need the backing
services from [`docker/deps.yml`](docker/deps.yml) (`just infra-up`).

### Library-crate integration demos

- **ratelimit** → `ping` guards `/ping` with a per-client (X-Forwarded-For /
  X-Real-IP) GCRA limit; set `PING_RATE_LIMIT_RPS` (0 disables). Excess → `429`.
- **secrets** → `ping` exposes `/secure` when `PING_API_KEY` is set: the key is
  held as a redacted `SecretString`; the Bearer token is compared in constant time.
- **resilient-client** → `heartbeat` polls `HEARTBEAT_UPSTREAM_URL` each tick
  (timeout + retry + circuit breaker), recording `heartbeat_upstream_checks_total{result}`.
- **valkey + kafka + otelx** → `tasks` stores tasks in Valkey, publishes a
  `task.created` event to Kafka on write, traces requests, and returns RFC 9457
  `problem+json` errors; **consumer** drains those events. Bring the deps up with
  `just infra-up`, then `just tasks run` / `just consumer run` (or `just stack-up`
  for the whole thing in containers).

---

## Layout

```
rust-basics/                ← bare-repo CONTAINER (.bare + .git + .cargo/config.toml + wt + CLAUDE.md)
└── master/                 ← canonical worktree (this tree)
    ├── Cargo.toml          ← workspace: members, profiles, shared deps
    ├── Cargo.lock          ← committed (this workspace ships binaries)
    ├── rust-toolchain.toml ← pins the toolchain (channel 1.95 + clippy/rustfmt)
    ├── justfile            ← workspace task runner (delegation + `each`)
    ├── mise.toml           ← pinned tools (rust, sccache, node, k6, AppSec)
    ├── deny.toml           ← cargo-deny config
    ├── crates/httpx/       ← shared HTTP scaffolding (+ problem+json, YAML config)
    ├── crates/ping/        ← HTTP service crate (+ Dockerfile)
    ├── crates/heartbeat/   ← worker crate (+ Dockerfile)
    ├── crates/valkey/      ← Valkey cache crate
    ├── crates/kafka/       ← Kafka producer/consumer crate
    ├── crates/otelx/       ← OpenTelemetry tracing crate
    ├── crates/tasks/       ← Valkey + Kafka CRUD service (+ Dockerfile)
    ├── crates/consumer/    ← Kafka consumer worker (+ Dockerfile)
    ├── docker/             ← deps.yml (Valkey + Kafka) + stack.yml (the app images)
    ├── e2e/                ← Playwright API tests (spawn the release binaries)
    ├── benchmarks/         ← k6 load tests
    └── scripts/            ← host-services-spawn.sh (just up / down)
```

---

## Quick start

```sh
mise trust && mise install     # pinned toolchain (rust, sccache, node, k6, …)
just setup                     # + rustup components + cargo-deny/audit/outdated

just ci                        # fmt-check → clippy → test

just up                        # ping :8080 + heartbeat :8081 on the host
curl -s localhost:8080/ping
curl -s localhost:8081/metrics | grep heartbeat_beats_total
just down

# data-services vertical (Valkey + Kafka):
just infra-up                  # docker compose deps (valkey + kafka)
just tasks run &               # tasks :8082
just consumer run &            # consumer :8083
curl -s -XPOST localhost:8082/tasks -d '{"title":"hello"}'
curl -s localhost:8083/metrics | grep consumer_tasks_consumed_total
#   …or run it all in containers:  just stack-up

just e2e                       # Playwright, dependency-free services
just e2e-deps                  # Playwright incl. tasks + consumer (needs `just infra-up`)
just bench-smoke               # k6, 50 VUs × 30s against ping
just bench-tasks smoke         # k6 against tasks (needs `just infra-up`)
```

---

## Common workspace commands

A Cargo workspace operates on every member at once, so these are plain
`cargo … --workspace`:

```sh
just check           # cargo check --workspace --all-targets
just build           # cargo build --workspace
just release         # cargo build --workspace --release
just test            # cargo test --workspace
just fmt             # cargo fmt --all
just fmt-check       # cargo fmt --all -- --check  (CI gate)
just lint            # clippy -D warnings -D pedantic -D perf -D suspicious
just deny            # cargo-deny: licences + advisories + bans
just audit           # cargo-audit (RustSec)
just doc-check       # cargo doc --workspace --no-deps -D warnings
just ci              # fmt-check → lint → test
just ci-full         # + doc-check + deny
just clean
```

### Per-crate commands

```sh
just httpx <recipe>      # also: resilient-client, ratelimit, secrets, valkey, kafka, otelx
just ping <recipe>       # also: heartbeat, tasks, consumer

# examples
just ping test
just tasks test          # full Valkey + Kafka stack via testcontainers
just httpx doc
just heartbeat lint

# run a recipe across every crate, in dependency order:
just each test
```

### Infra dependencies (Valkey + Kafka)

`tasks` and `consumer` need backing services. `docker/deps.yml` brings them up;
`docker/stack.yml` runs the app images on the same network.

```sh
just infra-up        # valkey :6379 + kafka :9092
just infra-down      # stop + drop volumes
just stack-up        # deps + build & run the tasks/consumer images
just stack-down      # tear the whole stack down
```

---

## E2E tests (Playwright)

API tests (no browser). The harness builds the release binaries, spawns them,
waits for `/healthz`, runs the specs, then stops them. See
[`e2e/README.md`](e2e/README.md).

```sh
just e2e-install     # pnpm install (once)
just e2e             # build + run the dependency-free suite
just e2e-deps        # + tasks & consumer specs (needs `just infra-up`)
just e2e-filter ping
just e2e-report
```

The `tasks`/`consumer` specs run only under `E2E_WITH_DEPS=1` (set by
`just e2e-deps`); otherwise they skip, so the default suite stays Docker-free.

## Benchmarks (k6)

Profiles `smoke` / `load` / `stress` / `soak` / `peak` (see
[`benchmarks/README.md`](benchmarks/README.md)).

```sh
just bench-smoke         # ping: 50 VUs × 30s
just bench-load          # ping: ramp 0→500 VUs
just bench-stress        # ping: ramp 0→2000 VUs
just bench-tasks smoke   # tasks (create+read): needs `just infra-up`
```

---

## Security tooling

AppSec tools are pinned in `mise.toml`, installed by `just setup-sec`.

| Recipe              | Tool        | Config              | Covers                                          |
| ------------------- | ----------- | ------------------- | ----------------------------------------------- |
| `just sec-secrets`  | gitleaks    | `.gitleaks.toml`    | secrets in tree + history                       |
| `just sec-sast`     | semgrep     | `.semgrepignore`    | `p/owasp-top-ten` + `p/rust` packs              |
| `just sec-deps`     | osv-scanner | `osv-scanner.toml`  | OSV.dev over `Cargo.lock` + `pnpm-lock`         |
| `just sec-iac`      | hadolint    | `.hadolint.yaml`    | every `crates/*/Dockerfile`                     |
| `just deny`         | cargo-deny  | `deny.toml`         | licences + advisories + bans                    |
| `just audit`        | cargo-audit | —                   | RustSec advisories                              |

Container side: `just docker-build SVC` → `just docker-scan-ci SVC` (syft SBOM +
grype, `--fail-on high`) → `just docker-sign/verify SVC TAG` (cosign key-mode).

---

## Docker

Each service has a multi-stage **cargo-chef** distroless Dockerfile (dep layer
cooked separately so source edits don't re-compile dependencies; debug info
split + stripped; `gcr.io/distroless/cc-debian12:nonroot`, uid 65532, no shell).
Build context is the **workspace root**:

```sh
docker build -f crates/ping/Dockerfile -t ping:dev .
docker run --rm -p 8080:8080 ping:dev

# the data-services stack (deps + app images), one command:
just stack-up        # docker compose deps.yml + stack.yml
curl -s -XPOST localhost:8082/tasks -d '{"title":"hi"}'
just stack-down
```

`tasks`/`consumer` build the same way; `rdkafka`'s librdkafka (and zlib, via the
`libz-static` feature) is vendored and statically linked, so they too run on the
`cc-debian12` base with nothing beyond glibc/libgcc. `docker/deps.yml` runs
Valkey + Redpanda (the Kafka API broker, dual listeners so both host processes and in-network
containers reach the broker); `docker/stack.yml` runs the app images against it.

---

## Worktrees (multi-branch dev)

A **bare-repo container** so each branch is a clean sibling checkout — inherited
from tracehub-edge.

```sh
# one-time container
git clone --bare git@github.com:wundernightmare/rust-basics.git rust-basics/.bare
cd rust-basics && echo 'gitdir: ./.bare' > .git
git --git-dir=.bare config remote.origin.fetch '+refs/heads/*:refs/remotes/origin/*'
git fetch origin && git worktree add master master

# per branch — the `wt` helper wraps the extra setup:
./wt add feat/x          # worktree + mise trust + pnpm install
./wt list
./wt rm  feat/x
```

What `git worktree add` does **not** do, and `wt` does: `mise trust` the new
worktree (else mise-shimmed tools — including the sccache `rustc-wrapper` — fail
with a misleading "error parsing config file") and `pnpm install` in `e2e/`.

**Build cache (sccache).** Each worktree keeps its own `target/`; cross-worktree
reuse comes from sccache caching dependency compilations, wired once in the
container-root `.cargo/config.toml` (machine-local, **not** committed — a
repo-level wrapper would also enable sccache in CI, where the cache is cold every
run). The binary is pinned in `mise.toml`. **Docker** `docker/deps.yml` is a
singleton (fixed project name `rust-basics-deps` + host ports) — run one deps
stack and every worktree reaches it at `localhost:<port>`.

---

## Toolchain notes

- **Rust** is pinned in `rust-toolchain.toml` (channel `1.95`, with `clippy` +
  `rustfmt`); `mise.toml` also pins it for the shims. That file is the single
  source of truth for the toolchain version in CI too.
- **just** drives everything (inherited from tracehub-edge).
- **axum** for HTTP, **tracing** for logging, **prometheus** for metrics,
  **envy** for config, **anyhow/thiserror** for errors.
- Data crates use the maintained drivers: **redis** (redis-rs, against a Valkey
  server), **rdkafka** (librdkafka, vendored + statically linked) and
  **opentelemetry**/`tracing-opentelemetry` for traces.
- **testcontainers** backs the integration suites — `just <crate> test` spins up
  real Valkey/Kafka, so those tests need a Docker daemon (they no-op when Docker
  is absent).

See [`CLAUDE.md`](CLAUDE.md) for high-signal notes and
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the dev workflow.

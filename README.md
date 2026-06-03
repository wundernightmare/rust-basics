# rust-basics

A small, idiomatic **Rust monorepo** modelled directly on the `tracehub-edge`
workspace (and the Go-flavoured `golang-basics` sibling): a Cargo workspace of
service crates + a shared library crate, a `just`-driven build/test/lint/
security/CI surface inherited from tracehub-edge, per-crate justfiles,
multi-stage **cargo-chef** distroless Docker images, a Playwright e2e suite, and
k6 load tests. It's a learning / template scaffold — two tiny services and one
shared library, wired up the way the real edge workspace is.

It lives in a **bare-repo worktree container** with sccache as the shared build
cache (see [Worktrees](#worktrees-multi-branch-dev)), exactly like tracehub-edge.

---

## Crates

| Crate                         | Kind         | Port(s)                   | One-liner                                                                                         |
| ----------------------------- | ------------ | ------------------------- | ------------------------------------------------------------------------------------------------- |
| [`ping`](crates/ping)             | HTTP service | `:8080`                   | axum ping/pong service. `GET /ping` → `pong`, with `?msg=` echo + `/version`.                     |
| [`heartbeat`](crates/heartbeat)   | Worker       | `:8081` (health/metrics)  | tokio ticker worker — emits a beat + bumps `heartbeat_beats_total` every interval.                |
| [`httpx`](crates/httpx)           | Library      | —                         | Shared axum scaffolding: server + tracing, Prometheus metrics, health, graceful shutdown, config. |

The dependency graph is `ping`, `heartbeat` → `httpx`. Both services — an
HTTP-first one and a worker — reuse the same crate for their `/healthz`,
`/readyz` and `/metrics` surface, so a worker is as observable as a server.

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
    ├── crates/httpx/       ← shared library crate (+ justfile + tests)
    ├── crates/ping/        ← HTTP service crate (+ Dockerfile)
    ├── crates/heartbeat/   ← worker crate (+ Dockerfile)
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

just e2e                       # Playwright (builds + spawns the binaries)
just bench-smoke               # k6, 50 VUs × 30s against ping
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
just httpx <recipe>
just ping <recipe>
just heartbeat <recipe>

# examples
just ping test
just httpx doc
just heartbeat lint

# run a recipe across every crate, in dependency order:
just each test
```

---

## E2E tests (Playwright)

API tests (no browser). The harness builds the release binaries, spawns them,
waits for `/healthz`, runs the specs, then stops them. See
[`e2e/README.md`](e2e/README.md).

```sh
just e2e-install     # pnpm install (once)
just e2e             # build + run the suite
just e2e-filter ping
just e2e-report
```

## Benchmarks (k6)

Profiles `smoke` / `load` / `stress` / `soak` / `peak` (see
[`benchmarks/README.md`](benchmarks/README.md)).

```sh
just bench-smoke     # 50 VUs × 30s
just bench-load      # ramp 0→500 VUs
just bench-stress    # ramp 0→2000 VUs
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
```

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
run). The binary is pinned in `mise.toml`. **Docker** is a singleton if/when a
local stack is added.

---

## Toolchain notes

- **Rust** is pinned in `rust-toolchain.toml` (channel `1.95`, with `clippy` +
  `rustfmt`); `mise.toml` also pins it for the shims. That file is the single
  source of truth for the toolchain version in CI too.
- **just** drives everything (inherited from tracehub-edge).
- **axum** for HTTP, **tracing** for logging, **prometheus** for metrics,
  **envy** for config, **anyhow/thiserror** for errors.

See [`CLAUDE.md`](CLAUDE.md) for high-signal notes and
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the dev workflow.

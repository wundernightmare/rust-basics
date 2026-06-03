# CLAUDE.md

Guidance for AI agents working in this repo. Deep docs live in
[README.md](README.md), [CONTRIBUTING.md](CONTRIBUTING.md), and each crate's
README; this file is only the high-signal, easy-to-miss bits.

## Build & test

- Workspace-wide: `just check` / `just test` / `just lint` / `just ci` (full
  list in README "Common workspace commands"). A Cargo workspace builds every
  member at once, so these are plain `cargo … --workspace` — no per-crate
  fan-out (unlike the Go sibling).
- Clippy gate (inherited from tracehub-edge): `-D warnings -D clippy::pedantic
  -D clippy::perf -D clippy::suspicious`. The crates carry a small
  `#![allow(...)]` for the three pedantic doc-noise lints
  (`must_use_candidate`, `missing_errors_doc`, `module_name_repetitions`); keep
  the rest denied.
- AppSec: `just sec` (source) + `just docker-scan-ci SVC` (image, `--fail-on
  high`). Waivers go in `deny.toml` / `osv-scanner.toml` / `.grype.yaml` with a
  documented removal trigger.

## Worktrees & the mise/sccache gotcha

- Multi-branch work uses a bare-repo container — see README "Worktrees". The
  repo root is a *bare container*, not a checkout: code and this file live one
  level down in `master/` (or a branch worktree), so `cd` in before running
  `cargo`/`just`.
- **The big footgun:** the container `.cargo/config.toml` sets
  `rustc-wrapper = <mise shim>/sccache`. If the worktree's `mise.toml` is not
  trusted, that shim aborts and **every `cargo build` fails** with a rustc
  "process didn't exit successfully" wrapping a `mise ERROR … not trusted`. Fix
  with `mise trust` (the `./wt` helper does this automatically on `wt add`).
- Each worktree has its own `target/`; sccache is the shared cross-worktree
  cache. The wiring is machine-local (not committed) so CI doesn't pay for a
  cold cache every run.

## Conventions

- All cross-cutting HTTP concerns live in `crates/httpx`; services stay thin
  (a `lib.rs` with the routes/worker + a tiny `main.rs`). Add shared behaviour
  to `httpx`, not per service.
- Services are split lib + bin so routes/worker are unit-testable via
  `tower::ServiceExt::oneshot` (httpx exposes `Server::router()` for the same
  reason — test the assembled app without binding a socket).
- Graceful shutdown: `httpx::Server::run` handles SIGINT/SIGTERM; the heartbeat
  worker rides the same signal via `tokio::select!` against the server future.

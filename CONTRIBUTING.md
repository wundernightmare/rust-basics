# Contributing

## Dev setup

```sh
mise trust && mise install     # pinned rust, sccache, node, k6, AppSec tools
just setup                     # + rustup components + cargo-deny/audit/outdated
just hooks-install             # optional: lefthook pre-commit/pre-push gates
```

> If `cargo build` fails with a rustc "process didn't exit successfully" error
> wrapping `mise ERROR … not trusted`, run `mise trust` — the sccache
> `rustc-wrapper` is a mise shim and needs the worktree trusted. (`./wt add`
> does this for you.)

## Workflow

1. Branch via the worktree helper: `./wt add feat/my-change` (from the container
   root). See README "Worktrees".
2. Make your change. Keep services thin — shared HTTP behaviour belongs in
   `crates/httpx`.
3. Before pushing:
   ```sh
   just ci          # fmt-check → clippy → test
   just e2e         # Playwright (optional but recommended)
   ```
4. Commit with a conventional-commit subject (`feat:`, `fix:`, `docs:`, …).

## Adding a crate

1. Create `crates/<name>/` with its own `Cargo.toml` (inherit `edition` /
   `rust-version` from the workspace).
2. Add it to `members` in the root `Cargo.toml`.
3. Copy a sibling's `justfile` (the recipes are crate-generic).
4. Add it to `CRATES` (and `SERVICES`, if it produces a binary) in the root
   `justfile`, and to the per-crate delegation block.

## Style & tests

- `cargo fmt` + the clippy gate (`-D pedantic -D perf -D suspicious`) — run
  `just lint` / `just fmt-check`.
- Unit/integration tests live in each crate's `tests/`; drive HTTP via
  `tower::ServiceExt::oneshot`.
- E2E lives in `e2e/` (Playwright, API-only); tag fast specs `@smoke`.
- Load tests live in `benchmarks/` (k6).

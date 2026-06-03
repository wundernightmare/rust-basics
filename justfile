# rust-basics — workspace task runner (inherited from tracehub-edge)
#
# Install: cargo install just | brew install just | mise use just
# Usage:   just <recipe>      | just --list
#
# A cargo workspace operates on every member at once, so the workspace-wide
# recipes are plain `cargo … --workspace`; per-crate work uses `-p` or the
# per-crate justfiles (delegated below).

# Service crates that produce a binary.
SERVICES := "ping heartbeat"
# Every crate, in dependency order (libs first).
CRATES := "httpx resilient-client ratelimit secrets ping heartbeat"

# Show all available recipes
default:
    @just --list --unsorted

# ── Workspace build ───────────────────────────────────────────────────────────

# Type-check every crate + target (fastest feedback)
check:
    cargo check --workspace --all-targets

# Debug build
build:
    cargo build --workspace

# Optimised release build (thin LTO + strip)
release:
    cargo build --workspace --release

# ── Workspace test ────────────────────────────────────────────────────────────

# Run all tests (no live services required)
test *args:
    cargo test --workspace {{args}}

# Run tests and print output even on success
test-verbose:
    cargo test --workspace -- --nocapture

# Run tests matching a name filter
test-filter FILTER:
    cargo test --workspace {{FILTER}}

# ── Lint & format ─────────────────────────────────────────────────────────────

# Clippy — deny warnings + pedantic/perf/suspicious (the tracehub-edge gate)
lint:
    cargo clippy --workspace --all-targets \
      -- -D warnings -D clippy::pedantic -D clippy::perf -D clippy::suspicious

# Clippy — fix automatically where possible
lint-fix:
    cargo clippy --workspace --all-targets --fix --allow-dirty

# Format all source in-place
fmt:
    cargo fmt --all

# Check formatting without modifying files (CI gate)
fmt-check:
    cargo fmt --all -- --check

# ── Security & docs ───────────────────────────────────────────────────────────

# cargo-deny: licences + advisories + bans + sources
deny:
    cargo deny check

# Security vulnerability audit (RustSec)
audit:
    cargo audit

# Build docs with broken intra-doc link checking (CI gate)
doc-check:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# ── AppSec (source-side) — tools pinned in mise.toml, installed by setup-sec ──

# One-time: install the AppSec toolchain via mise (idempotent)
setup-sec:
    mise install semgrep gitleaks osv-scanner hadolint syft grype cosign

# Run all source-side AppSec checks fail-fast
sec: sec-secrets sec-sast sec-deps sec-iac
    @echo "AppSec source checks passed"

# Secrets — gitleaks across the working tree + history
sec-secrets:
    mise exec -- gitleaks detect --source . --config .gitleaks.toml --verbose

# SAST — semgrep OWASP + Rust rule packs
sec-sast:
    mise exec -- semgrep --config p/owasp-top-ten --config p/rust --error

# Dependencies — osv-scanner over Cargo.lock + pnpm-lock.yaml
sec-deps:
    mise exec -- osv-scanner scan --config osv-scanner.toml --recursive .

# IaC — hadolint on every Dockerfile
sec-iac:
    #!/usr/bin/env bash
    set -euo pipefail
    find crates -name Dockerfile -print0 | xargs -0 -I{} mise exec -- hadolint --config .hadolint.yaml {}

# ── Container CVE / SBOM / signing ────────────────────────────────────────────

# Build a single service image locally (context = workspace root)
docker-build SVC:
    docker build -f crates/{{SVC}}/Dockerfile -t rust-basics-{{SVC}}:dev .

# Build all service images
docker-build-all:
    #!/usr/bin/env bash
    set -euo pipefail
    for s in {{SERVICES}}; do echo "── image $s"; docker build -f "crates/$s/Dockerfile" -t "rust-basics-$s:dev" .; done

# syft SBOM + grype CVE scan of a locally-built image (interactive)
docker-scan SVC:
    mise exec -- syft rust-basics-{{SVC}}:dev -o cyclonedx-json=sbom-{{SVC}}.json
    mise exec -- grype rust-basics-{{SVC}}:dev --config .grype.yaml

# Same scan but fail on HIGH+ — the CI variant
docker-scan-ci SVC:
    mise exec -- grype rust-basics-{{SVC}}:dev --config .grype.yaml --fail-on high

# Sign an image with cosign (key-mode, no Rekor); needs COSIGN_PRIVATE_KEY
docker-sign SVC TAG:
    mise exec -- cosign sign --key env://COSIGN_PRIVATE_KEY --tlog-upload=false rust-basics-{{SVC}}:{{TAG}}

# Offline-verify an image against cosign.pub
docker-verify SVC TAG:
    mise exec -- cosign verify --key cosign.pub --insecure-ignore-tlog=true rust-basics-{{SVC}}:{{TAG}}

# ── CI gates ──────────────────────────────────────────────────────────────────

# Standard pipeline: fmt-check → lint → test
ci: fmt-check lint test
    @echo "CI passed"

# Extended pipeline: + docs + supply-chain audit
ci-full: fmt-check lint test doc-check deny
    @echo "CI-full passed"

# ── Per-crate delegation ──────────────────────────────────────────────────────
# Forward any recipe to a single crate's justfile, e.g. `just ping test`.

httpx +args:
    just --justfile crates/httpx/justfile {{args}}

resilient-client +args:
    just --justfile crates/resilient-client/justfile {{args}}

ratelimit +args:
    just --justfile crates/ratelimit/justfile {{args}}

secrets +args:
    just --justfile crates/secrets/justfile {{args}}

ping +args:
    just --justfile crates/ping/justfile {{args}}

heartbeat +args:
    just --justfile crates/heartbeat/justfile {{args}}

# Run a recipe in every crate's justfile, in dependency order
each RECIPE:
    #!/usr/bin/env bash
    set -euo pipefail
    for c in {{CRATES}}; do echo "══ $c: {{RECIPE}}"; just --justfile "crates/$c/justfile" {{RECIPE}}; done

# ── Local bring-up (host services) ────────────────────────────────────────────

# Build + run every service on the host (logs+pids under .run/)
up *services:
    ./scripts/host-services-spawn.sh {{services}}

# Stop host services started by `just up`
down:
    ./scripts/host-services-spawn.sh --stop

# ── E2E (Playwright) ──────────────────────────────────────────────────────────

# Install the Node workspace (Playwright e2e deps; API tests, no browsers)
e2e-install:
    pnpm install

# Build the release binaries the e2e harness spawns
e2e-build: release

# Run the full Playwright e2e suite (spawns the binaries itself)
e2e: e2e-build
    cd e2e && pnpm test

# Playwright interactive UI
e2e-ui: e2e-build
    cd e2e && pnpm test:ui

# Run e2e tests matching a string
e2e-filter GREP: e2e-build
    cd e2e && pnpm test --grep "{{GREP}}"

# Open the last Playwright report
e2e-report:
    cd e2e && pnpm report

# ── k6 benchmarks ─────────────────────────────────────────────────────────────

bench-smoke: release
    ./benchmarks/run-k6.sh smoke

bench-load: release
    ./benchmarks/run-k6.sh load

bench-stress: release
    ./benchmarks/run-k6.sh stress

bench-soak: release
    ./benchmarks/run-k6.sh soak

bench-peak: release
    ./benchmarks/run-k6.sh peak

# ── Setup & housekeeping ──────────────────────────────────────────────────────

# Install dev tools: rustup components + cargo-deny/audit/outdated, via mise
setup:
    mise install
    rustup component add clippy rustfmt
    cargo install cargo-deny cargo-audit cargo-outdated --locked
    @echo "Dev tools installed — run 'just setup-sec' for the AppSec toolchain"

# Wire git hooks → lefthook (opt-in per clone; bypass with LEFTHOOK=0)
hooks-install:
    pnpm install
    pnpm exec lefthook install

# Remove lefthook-managed git hooks
hooks-uninstall:
    pnpm exec lefthook uninstall

# Show the workspace dependency tree
deps:
    cargo tree --workspace

# Check for newer dependency versions (requires cargo-outdated)
outdated:
    cargo outdated --workspace --root-deps-only

# Remove build/test artefacts
clean:
    cargo clean
    rm -rf e2e/playwright-report e2e/test-results e2e/.e2e-state.json benchmarks/results .run
    @echo "cleaned"

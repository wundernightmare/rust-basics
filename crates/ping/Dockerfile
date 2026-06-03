# syntax=docker/dockerfile:1.7
# ─────────────────────────────────────────────────────────────────────────────
# ping — multi-stage production image (cargo-chef, modelled on tracehub-edge)
#
# Build context — workspace ROOT (two levels up from this file), so the build
# sees the workspace Cargo.toml + Cargo.lock + every member crate:
#
#   docker build -f crates/ping/Dockerfile --tag ping:dev .   ← workspace root
#
# AppSec properties of the final image
# ─────────────────────────────────────
#   • Distroless cc runtime — no shell, no package manager, no build tools
#   • Non-root principal (uid 65532 / "nonroot")
#   • Minimal attack surface: binary + glibc/libgcc only
#   • No secrets, credentials or TLS material baked into any layer
#
# Runtime injection (never bake these into the image)
# ────────────────────────────────────────────────────
#   PING_ADDR        listen address (default 0.0.0.0:8080)
#   PING_LOG_LEVEL   log filter     (default info)
#   PING_LOG_FORMAT  json | text    (default json)
#
# Production image pinning: replace the version tags with @sha256:… digests in
# CI for byte-for-byte reproducibility and tag-mutation protection.
# ─────────────────────────────────────────────────────────────────────────────

ARG RUST_VERSION=1.95
ARG DEBIAN_CODENAME=bookworm

# ── Stage 1 – chef ─────────────────────────────────────────────────────────────
FROM rust:${RUST_VERSION}-${DEBIAN_CODENAME} AS chef
RUN cargo install cargo-chef --locked
WORKDIR /build

# ── Stage 2 – planner ──────────────────────────────────────────────────────────
# Resolve the full dependency graph into recipe.json (changes only when a
# Cargo.toml / Cargo.lock changes, so the cook layer below caches well).
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3 – builder ──────────────────────────────────────────────────────────
FROM chef AS builder
# Pass 1: compile every dependency at the `dist` profile. Cached until
# recipe.json changes.
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --profile dist --recipe-path recipe.json
# Pass 2: compile the application binary, then split debug info + strip.
COPY . .
RUN cargo build --profile dist --bin ping \
 && objcopy --only-keep-debug target/dist/ping target/dist/ping.debug \
 && objcopy --strip-debug --add-gnu-debuglink=target/dist/ping.debug target/dist/ping \
 && mkdir -p /out && cp target/dist/ping /out/ping

# ── Stage 4 – runtime ──────────────────────────────────────────────────────────
# distroless/cc carries glibc + libgcc for a dynamically-linked Rust binary —
# no shell, no package manager, nothing writable outside /tmp.
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

LABEL org.opencontainers.image.title="ping" \
      org.opencontainers.image.description="Minimal ping/pong HTTP service (rust-basics)" \
      org.opencontainers.image.vendor="tracehub" \
      org.opencontainers.image.source="https://github.com/wundernightmare/rust-basics" \
      org.opencontainers.image.licenses="UNLICENSED"

WORKDIR /app
COPY --from=builder --chown=65532:65532 /out/ping ./ping

# HTTP (and Prometheus /metrics on the same port)
EXPOSE 8080

USER 65532:65532
ENTRYPOINT ["/app/ping"]

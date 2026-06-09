# syntax=docker/dockerfile:1.7
# ─────────────────────────────────────────────────────────────────────────────
# tasks — multi-stage production image (cargo-chef, modelled on ping)
#
# Build context — workspace ROOT (two levels up from this file), so the build
# sees the workspace Cargo.toml + Cargo.lock + every member crate:
#
#   docker build -f crates/tasks/Dockerfile --tag tasks:dev .   ← workspace root
#
# AppSec properties of the final image
# ─────────────────────────────────────
#   • Distroless cc runtime — no shell, no package manager, no build tools
#   • Non-root principal (uid 65532 / "nonroot")
#   • Minimal attack surface: the binary + glibc/libgcc/libz only. rdkafka's
#     librdkafka is vendored and statically linked (no librdkafka.so to ship).
#   • No secrets, credentials or TLS material baked into any layer
#
# Runtime injection (never bake these into the image)
# ────────────────────────────────────────────────────
#   TASKS_CONFIG        optional path to a mounted config.yaml
#   TASKS_VALKEY_URL    redis://cache:6379
#   TASKS_KAFKA_BROKERS broker1:9092,broker2:9092
#   TASKS_OTEL_ENABLED  true to export traces to TASKS_OTEL_ENDPOINT
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
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3 – builder ──────────────────────────────────────────────────────────
FROM chef AS builder
# Pass 1: compile every dependency at the `dist` profile (cached until
# recipe.json changes). rdkafka-sys builds the vendored librdkafka here.
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --profile dist --recipe-path recipe.json
# Pass 2: compile the application binary, then split debug info + strip.
COPY . .
RUN cargo build --profile dist --bin tasks \
 && objcopy --only-keep-debug target/dist/tasks target/dist/tasks.debug \
 && objcopy --strip-debug --add-gnu-debuglink=target/dist/tasks.debug target/dist/tasks \
 && mkdir -p /out && cp target/dist/tasks /out/tasks

# ── Stage 4 – runtime ──────────────────────────────────────────────────────────
# distroless/cc carries glibc + libgcc + libz for a dynamically-linked Rust
# binary — no shell, no package manager, nothing writable outside /tmp.
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

LABEL org.opencontainers.image.title="tasks" \
      org.opencontainers.image.description="Tasks CRUD over Valkey + Kafka + OTel (rust-basics)" \
      org.opencontainers.image.vendor="tracehub" \
      org.opencontainers.image.source="https://github.com/wundernightmare/rust-basics" \
      org.opencontainers.image.licenses="UNLICENSED"

WORKDIR /app
COPY --from=builder --chown=65532:65532 /out/tasks ./tasks

# HTTP (and Prometheus /metrics on the same port)
EXPOSE 8082

USER 65532:65532
ENTRYPOINT ["/app/tasks"]

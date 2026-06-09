# syntax=docker/dockerfile:1.7
# ─────────────────────────────────────────────────────────────────────────────
# consumer — multi-stage production image (cargo-chef, modelled on ping)
#
# Build context — workspace ROOT (two levels up from this file):
#   docker build -f crates/consumer/Dockerfile --tag consumer:dev .   ← workspace root
#
# AppSec: distroless cc runtime, non-root (uid 65532), minimal surface. rdkafka's
# librdkafka is vendored and statically linked (zlib too, via libz-static), so
# nothing beyond glibc/libgcc ships.
#
# Runtime injection (never bake these into the image)
# ────────────────────────────────────────────────────
#   CONSUMER_KAFKA_BROKERS  broker1:9092,broker2:9092
#   CONSUMER_KAFKA_TOPIC    tasks.events
#   CONSUMER_KAFKA_GROUP    tasks-consumer
#   CONSUMER_OTEL_ENABLED   true to export traces
# ─────────────────────────────────────────────────────────────────────────────

ARG RUST_VERSION=1.95
ARG DEBIAN_CODENAME=bookworm

FROM rust:${RUST_VERSION}-${DEBIAN_CODENAME} AS chef
RUN cargo install cargo-chef --locked
WORKDIR /build

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --profile dist --recipe-path recipe.json
COPY . .
RUN cargo build --profile dist --bin consumer \
 && objcopy --only-keep-debug target/dist/consumer target/dist/consumer.debug \
 && objcopy --strip-debug --add-gnu-debuglink=target/dist/consumer.debug target/dist/consumer \
 && mkdir -p /out && cp target/dist/consumer /out/consumer

FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

LABEL org.opencontainers.image.title="consumer" \
      org.opencontainers.image.description="Kafka consumer worker for task.created events (rust-basics)" \
      org.opencontainers.image.vendor="tracehub" \
      org.opencontainers.image.source="https://github.com/wundernightmare/rust-basics" \
      org.opencontainers.image.licenses="UNLICENSED"

WORKDIR /app
COPY --from=builder --chown=65532:65532 /out/consumer ./consumer

# Health/metrics HTTP (Prometheus /metrics on the same port)
EXPOSE 8083

USER 65532:65532
ENTRYPOINT ["/app/consumer"]

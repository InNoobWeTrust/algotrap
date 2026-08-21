# syntax=docker/dockerfile:1.7

# Build from the repository root: docker build -f bins/telegrambot/deployment/Dockerfile .
FROM --platform=$TARGETPLATFORM lukemathwalker/cargo-chef:latest-rust-bookworm AS chef
# ─── Planner: generate dependency recipe ──────────────────────────────────────
FROM chef AS planner

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY bins/ bins/

RUN cargo chef prepare --recipe-path recipe.json

# ─── Cook: build ONLY dependencies (cached until Cargo.toml/lock change) ─────
FROM chef AS cook

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY --from=planner /build/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/cargo-target \
    CARGO_TARGET_DIR=/cargo-target cargo chef cook --release --recipe-path recipe.json -p telegrambot

# ─── Build: compile the application (fast — deps already built) ──────────────
FROM cook AS builder

COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY bins/ bins/

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/cargo-target \
    CARGO_TARGET_DIR=/cargo-target cargo build --release -p telegrambot --bin telegrambot && \
    cp /cargo-target/release/telegrambot /build/telegrambot && \
    strip /build/telegrambot

# ─── Pinned DuckDB shared library ─────────────────────────────────────────────
FROM --platform=$TARGETPLATFORM debian:bookworm AS duckdb-builder
ARG TARGETPLATFORM
ARG TARGETOS
ARG TARGETARCH
COPY docker/duckdb/ /usr/local/src/duckdb/
RUN TARGETPLATFORM="$TARGETPLATFORM" TARGETOS="$TARGETOS" TARGETARCH="$TARGETARCH" bash /usr/local/src/duckdb/build-libduckdb.sh

# ─── Runtime ─────────────────────────────────────────────────────────────────
FROM --platform=$TARGETPLATFORM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libgcc-s1 libssl3 libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/telegrambot .
COPY --from=duckdb-builder /opt/duckdb/lib/libduckdb.so /usr/local/lib/libduckdb.so
COPY --from=duckdb-builder /opt/duckdb/libexec/duckdb-smoke /usr/local/libexec/duckdb-smoke
COPY --from=duckdb-builder /opt/duckdb-artifacts/ /usr/local/share/algotrap/
RUN /usr/local/libexec/duckdb-smoke
# Default prompt config (overridden by ConfigMap mount in K8s)
COPY bins/telegrambot/config/prompts/ config/prompts/
ENV PROMPTS_DIR=config/prompts \
    DUCKDB_LIBRARY_PATH=/usr/local/lib/libduckdb.so

ENTRYPOINT ["./telegrambot"]

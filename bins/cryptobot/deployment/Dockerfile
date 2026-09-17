# syntax=docker/dockerfile:1.7

# Build from the repository root: docker build -f bins/cryptobot/deployment/Dockerfile .
FROM --platform=$TARGETPLATFORM lukemathwalker/cargo-chef:latest-rust-bookworm AS chef
# ─── Planner: generate dependency recipe ──────────────────────────────────────
FROM chef AS planner

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY bins/etf_dashboard/Cargo.toml ./bins/etf_dashboard/Cargo.toml
COPY bins/etf_dashboard/src ./bins/etf_dashboard/src
COPY bins/telegrambot/Cargo.toml ./bins/telegrambot/Cargo.toml
COPY bins/telegrambot/src ./bins/telegrambot/src
COPY bins/cryptobot/Cargo.toml ./bins/cryptobot/Cargo.toml
COPY bins/cryptobot/src ./bins/cryptobot/src

RUN cargo chef prepare --recipe-path recipe.json

# ─── Cook: build ONLY dependencies (cached until Cargo.toml/lock change) ─────
FROM chef AS cook

ARG TARGETPLATFORM
ENV DUCKDB_DOWNLOAD_LIB=1

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY --from=planner /build/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-target-${TARGETPLATFORM},target=/cargo-target \
    CARGO_TARGET_DIR=/cargo-target cargo chef cook --release --recipe-path recipe.json -p cryptobot

# ─── Build: compile the application (fast — deps already built) ──────────────
FROM cook AS builder

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY bins/etf_dashboard/Cargo.toml ./bins/etf_dashboard/Cargo.toml
COPY bins/etf_dashboard/src ./bins/etf_dashboard/src
COPY bins/telegrambot/Cargo.toml ./bins/telegrambot/Cargo.toml
COPY bins/telegrambot/src ./bins/telegrambot/src
COPY bins/cryptobot/Cargo.toml ./bins/cryptobot/Cargo.toml
COPY bins/cryptobot/src ./bins/cryptobot/src

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-target-${TARGETPLATFORM},target=/cargo-target \
    CARGO_TARGET_DIR=/cargo-target cargo build --release -p cryptobot --bin cryptobot && \
    cp /cargo-target/release/cryptobot /build/cryptobot && \
    strip /build/cryptobot && \
    mkdir -p /build-artifacts && \
    cp /cargo-target/release/deps/libduckdb.so /build-artifacts/libduckdb.so

# ─── Runtime ─────────────────────────────────────────────────────────────────
FROM --platform=$TARGETPLATFORM node:20-slim

# Install root certificates and wrangler
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libgcc-s1 libssl3 libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g wrangler

WORKDIR /app

COPY --from=builder /build/cryptobot .
COPY --from=builder /build-artifacts/libduckdb.so /usr/local/lib/libduckdb.so
RUN ldconfig && \
    ldd ./cryptobot | grep -Eq '[[:space:]]*libduckdb\.so[[:space:]]*=>[[:space:]]*/usr/local/lib/libduckdb\.so[[:space:]]+\('
# node:20-slim is Debian/glibc, matching the native Rust binary and built DuckDB library.
ENV DUCKDB_LIBRARY_PATH=/usr/local/lib/libduckdb.so
ENV LD_LIBRARY_PATH=/usr/local/lib

ENTRYPOINT ["./cryptobot"]

# syntax=docker/dockerfile:1.7

FROM lukemathwalker/cargo-chef:latest-rust-bookworm AS chef
# ─── Planner: generate dependency recipe ──────────────────────────────────────
FROM chef AS planner

WORKDIR /build
COPY --from=repo Cargo.toml Cargo.lock ./
COPY --from=repo src ./src
COPY --from=repo bins/etf_dashboard/Cargo.toml ./bins/etf_dashboard/Cargo.toml
COPY --from=repo bins/etf_dashboard/src ./bins/etf_dashboard/src
COPY --from=repo bins/telegrambot/Cargo.toml ./bins/telegrambot/Cargo.toml
COPY --from=repo bins/telegrambot/src ./bins/telegrambot/src
COPY Cargo.toml ./bins/cryptobot/Cargo.toml
COPY src ./bins/cryptobot/src

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
    CARGO_TARGET_DIR=/cargo-target cargo chef cook --release --recipe-path recipe.json -p cryptobot

# ─── Build: compile the application (fast — deps already built) ──────────────
FROM cook AS builder

COPY --from=repo Cargo.toml Cargo.lock ./
COPY --from=repo src ./src
COPY --from=repo bins/etf_dashboard/Cargo.toml ./bins/etf_dashboard/Cargo.toml
COPY --from=repo bins/etf_dashboard/src ./bins/etf_dashboard/src
COPY --from=repo bins/telegrambot/Cargo.toml ./bins/telegrambot/Cargo.toml
COPY --from=repo bins/telegrambot/src ./bins/telegrambot/src
COPY Cargo.toml ./bins/cryptobot/Cargo.toml
COPY src ./bins/cryptobot/src

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/cargo-target \
    CARGO_TARGET_DIR=/cargo-target cargo build --release -p cryptobot --bin cryptobot && \
    cp /cargo-target/release/cryptobot /build/cryptobot && \
    strip /build/cryptobot

# ─── Runtime ─────────────────────────────────────────────────────────────────
FROM node:20-slim

# Install root certificates and wrangler
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g wrangler

WORKDIR /app

COPY --from=builder /build/cryptobot .

ENTRYPOINT ["./cryptobot"]

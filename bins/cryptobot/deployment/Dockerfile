FROM lukemathwalker/cargo-chef:latest-rust-bookworm AS chef
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
RUN cargo chef cook --release --recipe-path recipe.json -p cryptobot

# ─── Build: compile the application (fast — deps already built) ──────────────
FROM cook AS builder

COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY bins/ bins/

RUN cargo build --release -p cryptobot --bin cryptobot && \
    strip /build/target/release/cryptobot

# ─── Runtime ─────────────────────────────────────────────────────────────────
FROM node:20-slim

# Install root certificates and wrangler
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g wrangler

WORKDIR /app

COPY --from=builder /build/target/release/cryptobot .

ENTRYPOINT ["./cryptobot"]

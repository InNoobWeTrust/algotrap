# ─── Build stage ──────────────────────────────────────────────────────────────
FROM rust:slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace root
COPY Cargo.toml Cargo.lock ./

# Copy root crate (algotrap lib)
COPY src/ src/

# Copy all workspace members (cargo needs them to resolve the workspace)
COPY bins/ bins/

# Build release binary
RUN cargo build --release -p telegrambot --bin telegrambot && \
    strip /build/target/release/telegrambot

# ─── Runtime stage ────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/telegrambot .
# Default prompt config (overridden by ConfigMap mount in K8s)
COPY bins/telegrambot/config/prompts/ config/prompts/
ENV PROMPTS_DIR=config/prompts

ENTRYPOINT ["./telegrambot"]

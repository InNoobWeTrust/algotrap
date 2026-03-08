# ─── Build stage ──────────────────────────────────────────────────────────────
FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconf perl make

WORKDIR /build

# Copy workspace root
COPY Cargo.toml Cargo.lock ./

# Copy root crate (algotrap lib)
COPY src/ src/

# Copy all workspace members (cargo needs them to resolve the workspace)
COPY bins/ bins/

# Build release binary (statically linked via musl)
RUN cargo build --release -p telegrambot --bin telegrambot && \
    strip /build/target/release/telegrambot

# ─── Runtime stage ────────────────────────────────────────────────────────────
FROM alpine:3.21

RUN apk add --no-cache ca-certificates

WORKDIR /app

COPY --from=builder /build/target/release/telegrambot .

# The bot reads config from env vars, no files needed
ENTRYPOINT ["./telegrambot"]

# Stage 1: Build the static Rust binaries
#FROM --platform=linux/amd64 rust:latest
FROM rust:latest AS builder

# Install musl target and build tools
#RUN apt-get update && apt-get install -y musl-tools libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*
#RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app

# Copy manifests from the project root
COPY Cargo.toml ./

# Copy the actual source code from the project root
COPY src ./src

# Copy the manifests from bins
COPY bins/cryptobot/Cargo.toml ./bins/cryptobot/
COPY bins/etf_dashboard/Cargo.toml ./bins/etf_dashboard/

# Copy the actual source code from bins
COPY bins/cryptobot/src ./bins/cryptobot/src
COPY bins/etf_dashboard/src ./bins/etf_dashboard/src

# Build the binaries and put to root under `/bin`
#RUN cargo install --target x86_64-unknown-linux-musl --path bins/cryptobot --root /
RUN cargo install --path bins/cryptobot --root /
RUN cargo install --path bins/etf_dashboard --root /

# Final layer only need to hold binaries
FROM scratch

WORKDIR /app

COPY --from=builder /bin/cryptobot .
COPY --from=builder /bin/etf_dashboard .

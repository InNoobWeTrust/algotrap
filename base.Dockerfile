# Stage 1: Build the static Rust binaries
#FROM --platform=linux/amd64 rust:latest
FROM rust:latest AS builder

# # Install musl target and build tools (for producing fully static binaries)
# # Add build deps required for static musl builds (perl, wget)
# RUN apt-get update && \
#     apt-get install -y --no-install-recommends \
#     build-essential musl-tools binutils pkg-config perl wget zlib1g-dev libbz2-dev liblz4-dev libzstd-dev ca-certificates && \
#     rm -rf /var/lib/apt/lists/*
# # Add the musl target and use musl-gcc as the linker for that target
# RUN rustup target add x86_64-unknown-linux-musl
# ENV CC_x86_64_unknown_linux_musl=musl-gcc
# # Ensure builds use the musl toolchain
# ENV CC=musl-gcc AR=ar

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

# Build the binaries and put to root under `/bin` (build for musl to avoid runtime dynamic deps)
# RUN cargo install --target x86_64-unknown-linux-musl --path bins/cryptobot --root /
# RUN cargo install --target x86_64-unknown-linux-musl --path bins/etf_dashboard --root /
RUN cargo install --path bins/cryptobot --root /
RUN cargo install --path bins/etf_dashboard --root /
# # Strip binaries to reduce size (optional; present because binutils was installed above)
# RUN strip /bin/cryptobot || true && strip /bin/etf_dashboard || true

# Final layer only need to hold binaries
FROM scratch

WORKDIR /app

COPY --from=builder /bin/cryptobot .
COPY --from=builder /bin/etf_dashboard .

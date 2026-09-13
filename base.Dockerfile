# Repository-root Docker build context is required.
# Every stage is built for the requested target, never the host's forced amd64 platform.
FROM --platform=$TARGETPLATFORM rust:1.97.1-bookworm@sha256:0e2bcaef56d041a486784e54104a81aebe0da44bd03019bd70bc0401e42e4a97 AS builder

WORKDIR /app

# Use the official DuckDB prebuilt shared library, downloaded at compile time.
ENV DUCKDB_DOWNLOAD_LIB=1
ENV CARGO_TARGET_DIR=/cargo-target

# Copy manifests from the project root
COPY Cargo.toml Cargo.lock ./

# Copy the actual source code from the project root
COPY src ./src

# Copy the manifests from bins
COPY bins/cryptobot/Cargo.toml ./bins/cryptobot/
COPY bins/etf_dashboard/Cargo.toml ./bins/etf_dashboard/
COPY bins/telegrambot/Cargo.toml ./bins/telegrambot/

# Copy the actual source code from bins
COPY bins/cryptobot/src ./bins/cryptobot/src
COPY bins/etf_dashboard/src ./bins/etf_dashboard/src
COPY bins/telegrambot/src ./bins/telegrambot/src

# Build glibc binaries because DuckDB is dynamically loaded in the Debian glibc runtime.
ARG RUST_TARGETS
RUN <<'EOF' bash
set -euxo pipefail

if [ -n "${RUST_TARGETS:-}" ]; then
  for TARGET in $(echo "$RUST_TARGETS" | tr ',' ' '); do
    case "$TARGET" in *-musl*) echo "musl targets are incompatible with the glibc DuckDB runtime" >&2; exit 1;; esac
    rustup target add "$TARGET"

    cargo install --locked --target "$TARGET" --path ./bins/cryptobot --root /
    # Capture the official release-profile DuckDB shared library for the runtime.
    mkdir -p /build-artifacts
    cp "/cargo-target/$TARGET/release/deps/libduckdb.so" /build-artifacts/libduckdb.so
    cargo install --locked --target "$TARGET" --path ./bins/etf_dashboard --root /
    cmp --silent "/cargo-target/$TARGET/release/deps/libduckdb.so" /build-artifacts/libduckdb.so

    # Rename output binaries for multi-arch
    case "$TARGET" in
      x86_64-unknown-linux-gnu)
        cp /bin/cryptobot /app/cryptobot-x86_64
        cp /bin/etf_dashboard /app/etf_dashboard-x86_64
        ;;
      aarch64-unknown-linux-gnu)
        cp /bin/cryptobot /app/cryptobot-aarch64
        cp /bin/etf_dashboard /app/etf_dashboard-aarch64
        ;;
    esac
  done
else
  cargo install --locked --path ./bins/cryptobot --root /
  # Capture the official release-profile DuckDB shared library for the runtime.
  mkdir -p /build-artifacts
  cp /cargo-target/release/deps/libduckdb.so /build-artifacts/libduckdb.so
  cargo install --locked --path ./bins/etf_dashboard --root /
  cmp --silent /cargo-target/release/deps/libduckdb.so /build-artifacts/libduckdb.so

  cp /bin/cryptobot /app/cryptobot &&
  cp /bin/etf_dashboard /app/etf_dashboard
fi
EOF
# # Strip binaries to reduce size (optional; present because binutils was installed above)
# RUN strip /bin/cryptobot || true && strip /bin/etf_dashboard || true

FROM --platform=$TARGETPLATFORM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    libgcc-s1 libstdc++6 libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/cryptobot* ./
COPY --from=builder /app/etf_dashboard* ./
COPY --from=builder /build-artifacts/libduckdb.so /usr/local/lib/libduckdb.so
RUN ldconfig
ENV DUCKDB_LIBRARY_PATH=/usr/local/lib/libduckdb.so

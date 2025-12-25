# Stage 1: Build the static Rust binaries
#FROM --platform=linux/amd64 rust:latest
FROM rust:latest AS builder

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
ARG RUST_TARGETS
RUN <<'EOF' bash
set -euxo pipefail

if [ -n "$RUST_TARGETS" ]; then
  # If any requested target is a musl target, install musl toolchain deps once
  case "$RUST_TARGETS" in
    *-musl*) HAS_MUSL=1 ;;
    *) HAS_MUSL=0 ;;
  esac

  if [ "$HAS_MUSL" -eq 1 ]; then
    apt-get update && \
      apt-get install -y --no-install-recommends \
        build-essential musl-tools binutils pkg-config perl wget \
        zlib1g-dev libbz2-dev liblz4-dev libzstd-dev ca-certificates && \
      rm -rf /var/lib/apt/lists/*

    # Prefer musl-gcc for musl builds; set generic CC/AR so cargo/rustc can pick it up
    export CC=musl-gcc
    export AR=ar
  fi

  for TARGET in $(echo "$RUST_TARGETS" | tr ',' ' '); do
    rustup target add "$TARGET"

    # Set target-specific CC vars for known musl targets (best-effort)
    case "$TARGET" in
      x86_64-unknown-linux-musl)
        export CC_x86_64_unknown_linux_musl=musl-gcc
        ;;
      aarch64-unknown-linux-musl)
        if command -v aarch64-linux-musl-gcc >/dev/null 2>&1; then
          export CC_aarch64_unknown_linux_musl=aarch64-linux-musl-gcc
        else
          export CC_aarch64_unknown_linux_musl=musl-gcc
        fi
        ;;
    esac

    cargo install --target "$TARGET" --path ./bins/cryptobot --root /
    cargo install --target "$TARGET" --path ./bins/etf_dashboard --root /

    # Rename output binaries for multi-arch
    case "$TARGET" in
      x86_64-unknown-linux-musl)
        cp /bin/cryptobot /app/cryptobot-x86_64
        cp /bin/etf_dashboard /app/etf_dashboard-x86_64
        ;;
      aarch64-unknown-linux-musl)
        cp /bin/cryptobot /app/cryptobot-aarch64
        cp /bin/etf_dashboard /app/etf_dashboard-aarch64
        ;;
    esac
  done
else
  cargo install --path ./bins/cryptobot --root / &&
  cargo install --path ./bins/etf_dashboard --root /
fi
EOF
# # Strip binaries to reduce size (optional; present because binutils was installed above)
# RUN strip /bin/cryptobot || true && strip /bin/etf_dashboard || true

# Final layer only need to hold binaries
FROM scratch

WORKDIR /app

COPY --from=builder /app/cryptobot* ./
COPY --from=builder /app/etf_dashboard* ./

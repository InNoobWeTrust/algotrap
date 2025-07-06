# Stage 1: Build the static Rust binary
FROM --platform=linux/amd64 rust:latest as builder

# Install musl target and build tools
RUN apt-get update && apt-get install -y musl-tools libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app

# Copy manifests from the project root
COPY Cargo.toml ./

# Cache dependencies
RUN mkdir src && echo "fn main() {println!(\"dummy\");}" > src/main.rs
RUN cargo build --release --target x86_64-unknown-linux-musl
RUN rm -rf src

# Copy the actual source code from the project root
COPY src ./src

# Build the application
RUN touch src/main.rs && rm -rf target/x86_64-unknown-linux-musl/release/deps/algotrap*
RUN cargo build --release --target x86_64-unknown-linux-musl

# Stage 2: Create the final deployment image
FROM --platform=linux/amd64 node:20-slim

# Install root certificates and wrangler
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN npm install -g wrangler

WORKDIR /app

# Copy the static binary from the builder stage
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/algotrap .

# Copy the entrypoint script from the deployment directory
COPY deployment/entrypoint.sh .

# Make the entrypoint script executable
RUN chmod +x entrypoint.sh

# Set the entrypoint
ENTRYPOINT ["./entrypoint.sh"]

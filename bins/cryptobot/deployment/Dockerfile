# Stage 1: Get the builder which contains static binaries
FROM algotrap-bins:latest AS builder

# Stage 2: Create the final deployment image
#FROM --platform=linux/amd64 node:20-slim
FROM node:20-slim

# Install root certificates and wrangler
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN npm install -g wrangler

WORKDIR /app

# Copy the static binary from the builder stage
COPY --from=builder /app/cryptobot .

# Copy the entrypoint script from the deployment directory
COPY deployment/entrypoint.sh .

# Make the entrypoint script executable
RUN chmod +x entrypoint.sh

# Set the entrypoint
ENTRYPOINT ["./entrypoint.sh"]

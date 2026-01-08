FROM node:20-slim

# Install root certificates and wrangler
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN npm install -g wrangler

WORKDIR /app

# Use build arg to specify the base image
ARG BASE_IMAGE=algotrap-bins:musl-latest

# Copy the static binaries from prebuilt musl image (only if they exist)
COPY --from=${BASE_IMAGE} /app/cryptobot* ./

# Copy the entrypoint script from the deployment directory
COPY deployment/entrypoint.sh .

# Make the entrypoint script executable
RUN chmod +x entrypoint.sh

# Set the entrypoint
ENTRYPOINT ["./entrypoint.sh"]

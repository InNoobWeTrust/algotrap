FROM node:20-slim

# Install root certificates and wrangler
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN npm install -g wrangler

WORKDIR /app

# Copy the static binary from prebuilt image
COPY --from=algotrap-bins:latest /app/cryptobot .

# Copy the entrypoint script from the deployment directory
COPY deployment/entrypoint.sh .

# Make the entrypoint script executable
RUN chmod +x entrypoint.sh

# Set the entrypoint
ENTRYPOINT ["./entrypoint.sh"]

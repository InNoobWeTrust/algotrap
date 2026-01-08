#!/bin/sh

# Exit immediately if a command exits with a non-zero status.
set -e

# Check for required environment variables
if [ -z "$CLOUDFLARE_ACCOUNT_ID" ] || [ -z "$CLOUDFLARE_API_TOKEN" ] || [ -z "$CLOUDFLARE_PAGES_PROJECT_NAME" ]; then
  echo "Error: CLOUDFLARE_ACCOUNT_ID, CLOUDFLARE_API_TOKEN, and CLOUDFLARE_PAGES_PROJECT_NAME must be set."
  exit 1
fi

echo "Executing application to generate HTML..."
# Detect architecture and run the correct binary
ARCH=$(uname -m)
if [ "$ARCH" = "x86_64" ] && [ -f "./cryptobot-x86_64" ]; then
  ./cryptobot-x86_64
elif [ "$ARCH" = "aarch64" ] && [ -f "./cryptobot-aarch64" ]; then
  ./cryptobot-aarch64
elif [ -f "./cryptobot" ]; then
  echo "Using default binary (no platform suffix)"
  ./cryptobot
else
  echo "Error: No suitable cryptobot binary found"
  exit 1
fi


# Prepare the directory for deployment
OUTPUT_DIR="public"
mkdir -p "$OUTPUT_DIR"

# Find the generated HTML file (assumes only one is created)
GENERATED_HTML=$(find . -maxdepth 1 -name '*.html' -print -quit)

if [ -z "$GENERATED_HTML" ]; then
  echo "Error: No .html file found in the current directory after execution."
  exit 1
fi

# Rename and move the file to be the root of the site
mv "$GENERATED_HTML" "$OUTPUT_DIR/index.html"

echo "Output prepared for deployment."

# Deploy directly to production
echo "Deploying to Cloudflare Pages..."
# Respect an upload timeout (seconds). Use UPLOAD_TIMEOUT, fall back to TIMEOUT, then default to 10s.
UPLOAD_TIMEOUT_SECS="${UPLOAD_TIMEOUT_SECS:-${TIMEOUT_SECS-10}}"

if ! timeout "${UPLOAD_TIMEOUT_SECS}s" wrangler pages deploy "$OUTPUT_DIR" --project-name="$CLOUDFLARE_PAGES_PROJECT_NAME"; then
    echo "Error: Deployment failed or timed out after ${UPLOAD_TIMEOUT_SECS}s"
    exit 1
fi

echo "Deployment successful!"

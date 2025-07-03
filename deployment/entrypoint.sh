#!/bin/sh

# Exit immediately if a command exits with a non-zero status.
set -e

# Check for required environment variables
if [ -z "$CLOUDFLARE_ACCOUNT_ID" ] || [ -z "$CLOUDFLARE_API_TOKEN" ] || [ -z "$PROJECT_NAME" ]; then
  echo "Error: CLOUDFLARE_ACCOUNT_ID, CLOUDFLARE_API_TOKEN, and PROJECT_NAME must be set."
  exit 1
fi

echo "Executing application to generate HTML..."
# Run the application binary
./algotrap

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
wrangler pages deploy "$OUTPUT_DIR" --project-name="$PROJECT_NAME"

echo "Deployment successful!"

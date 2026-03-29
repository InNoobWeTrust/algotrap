#!/bin/sh

# Exit immediately if a command exits with a non-zero status.
set -e

# Check for required environment variables
if [ -z "$CLOUDFLARE_ACCOUNT_ID" ] || [ -z "$CLOUDFLARE_API_TOKEN" ] || [ -z "$CLOUDFLARE_PAGES_PROJECT_NAME" ]; then
  echo "Error: CLOUDFLARE_ACCOUNT_ID, CLOUDFLARE_API_TOKEN, and CLOUDFLARE_PAGES_PROJECT_NAME must be set."
  exit 1
fi

if [ -z "$TICKERS" ]; then
  echo "Error: TICKERS env var must be set (JSON array)."
  exit 1
fi

# The binary is now a long-lived service with internal scheduling.
# It handles data fetching, chart generation, and deployment in a loop.
ARCH=$(uname -m)
if [ "$ARCH" = "x86_64" ] && [ -f "./cryptobot-x86_64" ]; then
  exec ./cryptobot-x86_64
elif [ "$ARCH" = "aarch64" ] && [ -f "./cryptobot-aarch64" ]; then
  exec ./cryptobot-aarch64
elif [ -f "./cryptobot" ]; then
  exec ./cryptobot
else
  echo "Error: No suitable cryptobot binary found"
  exit 1
fi

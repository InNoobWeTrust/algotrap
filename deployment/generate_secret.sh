#!/bin/bash

# This script generates the Kubernetes secret YAML from the .env file.

SCRIPT_DIR=$(dirname "$0")
ENV_FILE="$SCRIPT_DIR/../.env"
TEMPLATE_FILE="$SCRIPT_DIR/../k8s/secret.yaml.template"
OUTPUT_FILE="$SCRIPT_DIR/../k8s/secret.yaml"

if [ ! -f "$ENV_FILE" ]; then
  echo "Error: .env file not found at $ENV_FILE"
  exit 1
fi

if [ ! -f "$TEMPLATE_FILE" ]; then
  echo "Error: Secret template file not found at $TEMPLATE_FILE"
  exit 1
fi

# Read .env file and export variables
set -a
source "$ENV_FILE"
set +a

# Read template and replace placeholders
SECRET_YAML=$(cat "$TEMPLATE_FILE")

# Replace each placeholder with the base64 encoded value from .env
SECRET_YAML=$(echo "$SECRET_YAML" | sed "s/{{TFS}}/$(echo -n "$TFS" | base64)/g")
SECRET_YAML=$(echo "$SECRET_YAML" | sed "s/{{CLOUDFLARE_ACCOUNT_ID}}/$(echo -n "$CLOUDFLARE_ACCOUNT_ID" | base64)/g")
SECRET_YAML=$(echo "$SECRET_YAML" | sed "s/{{CLOUDFLARE_API_TOKEN}}/$(echo -n "$CLOUDFLARE_API_TOKEN" | base64)/g")
SECRET_YAML=$(echo "$SECRET_YAML" | sed "s/{{CLOUDFLARE_PAGES_PROJECT_NAME}}/$(echo -n "$CLOUDFLARE_PAGES_PROJECT_NAME" | base64)/g")
SECRET_YAML=$(echo "$SECRET_YAML" | sed "s/{{NTFY_TOPIC}}/$(echo -n "$NTFY_TOPIC" | base64)/g")
SECRET_YAML=$(echo "$SECRET_YAML" | sed "s/{{NTFY_TF_EXCLUSION}}/$(echo -n "$NTFY_TF_EXCLUSION" | base64)/g")
SECRET_YAML=$(echo "$SECRET_YAML" | sed "s/{{NTFY_ALWAYS}}/$(echo -n "$NTFY_ALWAYS" | base64)/g")

echo "$SECRET_YAML" > "$OUTPUT_FILE"

echo "Generated Kubernetes secret at $OUTPUT_FILE"

#!/bin/bash

# This script deploys multiple cronjobs based on environment files.

set -eo pipefail

# Get the directory of the script
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
K8S_DIR="$(dirname "$SCRIPT_DIR")"/k8s

# Directory containing the .env files
ENV_DIR="$SCRIPT_DIR"/env_configs

# Check if the directory exists
if [ ! -d "$ENV_DIR" ]; then
  echo "Error: Directory $ENV_DIR not found." >&2
  exit 1
fi

# Loop through each .env file in the directory
for env_file in "$ENV_DIR"/*.env; do
  if [ -f "$env_file" ]; then
    # Extract the symbol from the filename (e.g., ETH-USDT.env -> eth-usdt)
    filename=$(basename -- "$env_file")
    symbol_name=$(echo "${filename%.*}" | tr '[:upper:]' '[:lower:]')

    echo "Processing $env_file for symbol: $symbol_name"

    # Source the .env file to get the variables
    # and ensure they are exported for the next commands.
    set -a
    # shellcheck disable=SC1090
    source "$env_file"
    set +a

    # Create a temporary directory for the generated files
    tmp_dir=$(mktemp -d)

    # Define unique names for the Kubernetes resources
    secret_name="algotrap-secrets-$symbol_name"
    cronjob_name="algotrap-cronjob-$symbol_name"

    # Generate the secret.yaml from the template
    sed -e "s/{{SECRET_NAME}}/$secret_name/g" \
        -e "s/{{TFS}}/$(echo -n "$TFS" | base64)/g" \
        -e "s/{{CLOUDFLARE_ACCOUNT_ID}}/$(echo -n "$CLOUDFLARE_ACCOUNT_ID" | base64)/g" \
        -e "s/{{CLOUDFLARE_API_TOKEN}}/$(echo -n "$CLOUDFLARE_API_TOKEN" | base64)/g" \
        -e "s/{{CLOUDFLARE_PAGES_PROJECT_NAME}}/$(echo -n "$CLOUDFLARE_PAGES_PROJECT_NAME" | base64)/g" \
        -e "s/{{NTFY_TOPIC}}/$(echo -n "$NTFY_TOPIC" | base64)/g" \
        -e "s/{{NTFY_TF_EXCLUSION}}/$(echo -n "$NTFY_TF_EXCLUSION" | base64)/g" \
        -e "s/{{SYMBOL}}/$(echo -n "$SYMBOL" | base64)/g"         -e "s/{{SL_PERCENT}}/$(echo -n "$SL_PERCENT" | base64)/g"         -e "s/{{TOL_PERCENT}}/$(echo -n "$TOL_PERCENT" | base64)/g"         -e "s/{{NTFY_ALWAYS}}/$(echo -n "$NTFY_ALWAYS" | base64)/g"         "$K8S_DIR/secret.yaml.template" > "$tmp_dir/secret.yaml"

    # Generate the cronjob.yaml from the template
    sed -e "s/{{CRONJOB_NAME}}/$cronjob_name/g" \
        -e "s/{{SECRET_NAME}}/$secret_name/g" \
        "$K8S_DIR/cronjob.yaml.template" > "$tmp_dir/cronjob.yaml"

    # Apply the generated files to the Kubernetes cluster
    echo "Applying Kubernetes configurations for $symbol_name"
    kubectl apply -f "$tmp_dir/secret.yaml"
    kubectl apply -f "$tmp_dir/cronjob.yaml"

    # Clean up the temporary directory
    rm -rf "$tmp_dir"

    echo "Successfully deployed cronjob for $symbol_name"
    echo "--------------------------------------------------"
  fi
done

echo "All cronjobs have been deployed."
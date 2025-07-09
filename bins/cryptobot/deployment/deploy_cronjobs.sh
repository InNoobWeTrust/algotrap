#!/bin/bash

# This script deploys multiple cronjobs based on environment files.

set -eo pipefail

# Get the directory of the script
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
K8S_DIR="$(dirname "$SCRIPT_DIR")"/k8s

# Directory containing the .env files
ENV_DIR="$SCRIPT_DIR"/envs

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
    secret_name="algotrap-cryptobot-secrets-$symbol_name"
    cronjob_name="algotrap-cryptobot-cronjob-$symbol_name"

    # Read variable names from .env.template
    ENV_VARS=$(awk -F'=' '/^[A-Z_]+=/{print $1}' "$SCRIPT_DIR/../.env.template" | tr '\n' ' ')

    SECRET_DATA_CONTENT=""
    for var_name in $ENV_VARS; do
      # Get the value of the variable (already sourced from .env file)
      var_value=$(printf %s "${!var_name}")
      # Base64 encode the value and append to SECRET_DATA_CONTENT
      SECRET_DATA_CONTENT+="  $var_name: \"$(echo -n "$var_value" | base64)\"\n"
    done

    # Generate the secret.yaml from the template
    sed -e "s|{{SECRET_NAME}}|$secret_name|g" \
        -e "s|{{SECRET_DATA}}|$SECRET_DATA_CONTENT|g" \
        "$K8S_DIR/secret.yaml.template" > "$tmp_dir/secret.yaml"

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
#!/bin/bash

# Setup cronjobs for cryptobot with different env files
# Run every 5 minutes for each trading pair

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRYPTOBOT_BIN="$(command -v cryptobot)"

# Check if cryptobot exists
if [ ! -f "$CRYPTOBOT_BIN" ]; then
    echo "Error: cryptobot not found at $CRYPTOBOT_BIN"
    exit 1
fi

# Get current crontab
crontab -l > /tmp/current_cron 2>/dev/null || touch /tmp/current_cron

# Remove existing cryptobot entries
grep -v "cryptobot" /tmp/current_cron > /tmp/new_cron

# Add new cronjobs for each .env file (excluding .example files)
for env_file in "$SCRIPT_DIR"/envs/*.env; do
    if [[ -f "$env_file" && ! "$env_file" =~ \.example$ ]]; then
        basename=$(basename "$env_file" .env)

        # Read env vars from file and build env command
        env_vars=$(printf "\n\nSHELL=/bin/bash\nPATH=%s" "$PATH")
        while IFS='=' read -r key value; do
            # Skip comments and empty lines
            [[ "$key" =~ ^[[:space:]]*# ]] && continue
            [[ -z "$key" ]] && continue

            # Remove quotes and whitespace
            key=$(echo "$key" | xargs)
            value=$(echo "$value" | xargs | sed 's/^"\(.*\)"$/\1/' | sed "s/^'\(.*\)'$/\1/")

            if [[ -n "$key" && -n "$value" ]]; then
                env_vars=$(printf "%s\n%s=%s" "$env_vars" "$key" "$value")
            fi
        done < "$env_file"

        # Get project name for wrangler
        pj_name=$(printf "%s\n" "$env_vars" | grep CLOUDFLARE_PAGES_PROJECT_NAME | cut -d= -f2)

        printf "%s\n*/15 * * * * cd \$(mktemp -d -t cryptobot) && timeout 10s %s && mkdir -p public && mv *.html public/index.html && timeout 20s npx -y wrangler pages deploy public --project-name=%s\n" "$env_vars" "$CRYPTOBOT_BIN" "$pj_name" >> /tmp/new_cron
    fi
done

# Install new crontab
crontab /tmp/new_cron

# Clean up
rm /tmp/current_cron /tmp/new_cron

echo "Cronjobs installed successfully:"
crontab -l | grep cryptobot

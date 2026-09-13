# telegrambot

`telegrambot` is the stateful LLM-powered market analyst. It fetches market
data, uses the shared analysis library and a headless browser to review charts,
then sends analysis and chart images to Telegram.

Shared analysis internals are documented in [`src/README.md`](../../src/README.md).
This README is the operator guide for the binary.

## Prerequisites

- Rust for local execution, or Docker for container deployment.
- A Telegram bot token and destination chat ID.
- An OpenAI-compatible LLM endpoint, API key, and model.
- Browserless/Chromium reachable at `BROWSERLESS_URL` for chart screenshots.

## Configuration

Start from [`.env.example`](.env.example). The required runtime settings are:

| Variable | Purpose |
| --- | --- |
| `TICKERS` | JSON array of ticker objects, including symbols, thresholds, timeframes, and default views |
| `TELEGRAM_BOT_TOKEN` | Telegram bot credential |
| `TELEGRAM_CHAT_ID` | Target chat or group |
| `LLM_API_BASE` | OpenAI-compatible API endpoint |
| `LLM_API_KEY` | LLM provider credential |
| `LLM_MODEL` | Model name accepted by the endpoint |
| `BROWSERLESS_URL` | Browserless service URL |

`PROMPTS_DIR` selects the runtime prompt directory. `SCAN_INTERVAL_SECS`,
`TIER_ALERT_THRESHOLD`, and `TIER_WATCH_THRESHOLD` tune scheduled alerting;
they are optional when using manual commands only. Keep credentials outside
the repository.

## Run locally

With Browserless available locally or through a Kubernetes port-forward:

```bash
cp bins/telegrambot/.env.example bins/telegrambot/.env
# Edit bins/telegrambot/.env
kubectl port-forward svc/browserless 3000:3000 &
cargo run -p telegrambot
```

The analysis harness can be run without sending Telegram messages:

```bash
cargo run -p telegrambot --bin test_analysis
```

## Deploy

### Kubernetes

Build from the workspace root, create the runtime secret, then apply prompts,
Browserless, and the bot deployment:

```bash
docker build -f bins/telegrambot/deployment/Dockerfile -t telegrambot:latest .
kubectl create secret generic telegrambot-env \
  --from-env-file=bins/telegrambot/.env
kubectl apply -f bins/telegrambot/k8s/prompts-configmap.yaml
kubectl apply -f bins/telegrambot/k8s/browserless.yaml
kubectl apply -f bins/telegrambot/k8s/telegrambot.yaml
```

The Kubernetes manifest mounts prompts and persistent bot memory and supplies
the in-cluster Browserless URL. Make the image available to the cluster before
starting the deployment.

### Docker Compose

Use the repository-provided compose deployment when a local container stack is
preferred:

```bash
docker compose -f bins/telegrambot/deployment/docker-compose.yaml up
```

The repository documentation index is available at
[`docs/README.md`](../../docs/README.md).

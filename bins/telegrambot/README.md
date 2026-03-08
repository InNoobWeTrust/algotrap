# 🤖 telegrambot

LLM-powered crypto market analyst that sends periodic analysis to Telegram.
Uses an agentic multi-turn loop to inspect indicators, capture chart screenshots,
and synthesize actionable recommendations.

## Quick Start

```bash
# 1. Copy and fill in config
cp .env.example .env
# Edit .env with your Telegram token, chat ID, LLM endpoint, etc.

# 2. Deploy dependencies to Kubernetes
kubectl apply -f k8s/browserless.yaml
kubectl apply -f k8s/litellm.yaml
# Check litellm logs for GitHub Copilot OAuth code:
kubectl logs -l app=litellm --tail=5

# 3. Run locally (with port-forwards)
kubectl port-forward svc/browserless 3000:3000 &
kubectl port-forward svc/litellm 4000:4000 &
cargo run -p telegrambot

# Or run the test harness (no Telegram needed):
cargo run -p telegrambot --bin test_analysis
```

## How It Works

```
Every N seconds (configurable):
  1. Fetch OHLCV data from BingX across 8 timeframes
  2. Compute indicators (RSSI, ATR bands, structure power, Sharpe, ...)
  3. Run LLM agent loop (multi-turn with tool calls):
     → get_multi_tf_overview     → bird's eye view
     → get_indicator_summary     → drill into specific TFs
     → get_price_action          → raw candle data
     → capture_chart             → Browserless screenshot
  4. Send analysis + chart to Telegram
```

## Configuration

All via environment variables (see [`.env.example`](.env.example)):

| Variable | Description | Example |
|----------|-------------|---------|
| `SYMBOL` | Trading pair | `BTC-USDT` |
| `TFS` | Timeframes (comma-separated) | `5m,15m,1h,4h,1d,1w` |
| `DEFAULT_TF` | Chart default view | `4h` |
| `SL_PERCENT` | Stop-loss % for leverage calc | `0.1` |
| `TOL_PERCENT` | ATR tolerance adjustment | `0.618` |
| `TELEGRAM_BOT_TOKEN` | From @BotFather | |
| `TELEGRAM_CHAT_ID` | Target chat/group ID | |
| `LLM_API_BASE` | OpenAI-compatible endpoint | `http://litellm:4000/v1` |
| `LLM_API_KEY` | API key / proxy master key | |
| `LLM_MODEL` | Model name | `gpt-4o` |
| `BROWSERLESS_URL` | Browserless service | `http://browserless:3000` |
| `ANALYSIS_INTERVAL_SECS` | Loop interval (default: 3600) | `3600` |

## Deployment

### Option 1: Local (cargo run)

```bash
kubectl port-forward svc/browserless 3000:3000 &
kubectl port-forward svc/litellm 4000:4000 &
cargo run -p telegrambot
```

### Option 2: In-cluster (Kubernetes)

```bash
# Build the image (from workspace root)
docker build -f bins/telegrambot/deployment/Dockerfile -t telegrambot:latest .

# Create the secret with your config
kubectl create secret generic telegrambot-env \
  --from-env-file=bins/telegrambot/.env

# Deploy
kubectl apply -f bins/telegrambot/k8s/telegrambot.yaml
```

### Option 3: Docker Compose

```bash
docker compose -f bins/telegrambot/deployment/docker-compose.yaml up
```

## Infrastructure Dependencies

| Service | Manifest | Purpose |
|---------|----------|---------|
| **Browserless** | `k8s/browserless.yaml` | Headless Chrome for chart screenshots |
| **LiteLLM** | `k8s/litellm.yaml` | LLM proxy (GitHub Copilot, OpenAI, etc.) |

> **Note**: LiteLLM's GitHub Copilot provider uses OAuth device flow.
> On first startup, check pod logs for the auth URL and code.

## Project Structure

```
src/
├── main.rs              — entry point, analysis loop
├── lib.rs               — module re-exports
├── config.rs            — EnvConf
├── data.rs              — BingX data fetch + indicators
├── browserless.rs       — screenshot client
├── chart.rs             — chart HTML renderer
├── chart_template.html  — LightweightCharts template
├── telegram.rs          — Telegram messaging
└── llm/
    ├── mod.rs           — agentic LLM loop
    └── tools.rs         — tool definitions + execution
k8s/
├── browserless.yaml     — Browserless deployment
├── litellm.yaml         — LiteLLM proxy deployment
└── telegrambot.yaml     — Bot deployment (in-cluster)
deployment/
├── Dockerfile           — Multi-stage build
└── docker-compose.yaml  — All-in-one local setup
docs/specs/
├── architecture.md      — Full architecture spec
└── progress.md          — Implementation progress log
```

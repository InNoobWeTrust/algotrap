# 🤖 telegrambot

LLM-powered crypto market analyst that sends periodic analysis to Telegram.
Uses an agentic multi-turn loop to inspect indicators, capture chart screenshots
across all timeframes, and synthesize actionable recommendations.

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

```text
Every N seconds (configurable):
  1. Fetch OHLCV data from BingX across all configured timeframes
  2. Compute indicators (RSSI, ATR bands, structure power, Sharpe, ...)
  3. Capture chart screenshots for ALL timeframes via Browserless
  4. Run LLM agent loop (multi-turn with tool calls):
     → get_multi_tf_overview     → bird's eye view
     → get_indicator_summary     → drill into specific TFs
     → get_price_action          → raw candle data
     → capture_chart             → visual confirmation
  5. Send chart album (all TFs) + analysis text to Telegram
     → Header: Unicode-decorated ticker + timestamp
     → Charts: media group album (one image per TF)
     → Text:  concise ≤300 word analysis
```

## Configuration

All via environment variables (see [`.env.example`](.env.example)):

| Variable                  | Description                   | Example                    |
| ------------------------- | ----------------------------- | -------------------------- |
| `SYMBOL`                  | Trading pair                  | `BTC-USDT`                 |
| `TFS`                     | Timeframes (comma-separated)  | `5m,15m,1h,4h,1d,1w`      |
| `DEFAULT_TF`              | Chart default view            | `4h`                       |
| `SL_PERCENT`              | Stop-loss % for leverage calc | `0.1`                      |
| `TOL_PERCENT`             | ATR tolerance adjustment      | `0.618`                    |
| `TELEGRAM_BOT_TOKEN`      | From @BotFather               |                            |
| `TELEGRAM_CHAT_ID`        | Target chat/group ID          |                            |
| `LLM_API_BASE`            | OpenAI-compatible endpoint    | `http://litellm:4000/v1`   |
| `LLM_API_KEY`             | API key / proxy master key    |                            |
| `LLM_MODEL`               | Model name                    | `gpt-5-mini`               |
| `BROWSERLESS_URL`         | Browserless service           | `http://browserless:3000`  |
| `PROMPTS_DIR`             | Prompt config directory       | `config/prompts`           |
| `ANALYSIS_INTERVAL_SECS`  | Loop interval (default: 3600) | `3600`                     |

## Prompt & Tool Configuration

System prompts and user messages are **external files** loaded at runtime
from `PROMPTS_DIR`. Tool definitions and parameter schemas are automatically
derived in Rust code via [`llm_tool`](https://docs.rs/llm-tool) from comprehensive docstrings.

```text
config/prompts/
├── system.txt             — manual /analyze system prompt ({{symbol}}, {{tfs}}, {{default_tf}})
├── user.txt               — manual /analyze user prompt ({{symbol}}, {{time}})
├── system_adaptive.txt    — alert scan system prompt (weights, thresholds, memory)
└── user_adaptive.txt      — alert scan user prompt (dynamic memory context)
```

In Kubernetes, these are mounted from a ConfigMap:

```bash
# Edit prompts
vim k8s/prompts-configmap.yaml

# Apply and restart (no Docker rebuild needed)
kubectl apply -f k8s/prompts-configmap.yaml
kubectl rollout restart deployment/telegrambot
```

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

# Deploy prompts + bot
kubectl apply -f bins/telegrambot/k8s/prompts-configmap.yaml
kubectl apply -f bins/telegrambot/k8s/telegrambot.yaml
```

### Option 3: Docker Compose

```bash
docker compose -f bins/telegrambot/deployment/docker-compose.yaml up
```

## Infrastructure Dependencies

| Service         | Manifest               | Purpose                                  |
| --------------- | ---------------------- | ---------------------------------------- |
| **Browserless** | `k8s/browserless.yaml` | Headless Chrome for chart screenshots    |
| **LiteLLM**     | `k8s/litellm.yaml`     | LLM proxy (GitHub Copilot, OpenAI, etc.) |

> **Note**: LiteLLM's GitHub Copilot provider uses OAuth device flow.
> On first startup, check pod logs for the auth URL and code.

## Project Structure

```text
src/
├── main.rs              — entry point, analysis loop, all-TF chart capture
├── lib.rs               — module re-exports
├── config.rs            — EnvConf (env vars + prompts_dir)
├── data.rs              — BingX data fetch + indicators
├── browserless.rs       — screenshot client
├── chart.rs             — chart HTML renderer
├── chart_template.html  — LightweightCharts template
├── telegram.rs          — Telegram messaging (media groups + ticker header)
└── llm/
    ├── mod.rs           — agentic LLM loop + external prompt loading
    └── tools.rs         — tool definitions (derived via llm_tool) + execution
config/prompts/          — external prompt templates (loaded at runtime)
k8s/
├── browserless.yaml     — Browserless deployment
├── litellm.yaml         — LiteLLM proxy deployment
├── prompts-configmap.yaml — Prompt config (system, user templates)
└── telegrambot.yaml     — Bot deployment (w/ ConfigMap mount)
deployment/
├── Dockerfile           — Multi-stage Debian build
└── docker-compose.yaml  — All-in-one local setup
docs/specs/
├── architecture.md      — Full architecture spec
└── progress.md          — Implementation progress log
```

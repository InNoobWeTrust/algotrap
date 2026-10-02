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

## I-Ching context and prompt policy

The three opening energy columns (`iching_original_energy`, `iching_transformed_energy`, `iching_mutual_energy`) are calendrical/structural coordinates derived from the same cast; they are not independent market confirmations. **`iching_moving_line` is the opening moving line** (1..=6, from `moving_line_open`); the forecast header's `ml` field is the **terminal** moving line of that future bar's trajectory, not the exported opening alias.

**Latest bar close:** The trajectory for the most recent observed bar uses `bar_scheduled_close_ms(kline.time, tf)` as its exclusive close; all prior bars use the next observed bar's open as their close boundary. The forming bar therefore receives a full `[open, scheduled_close)` trajectory, not a degenerate point cast; the scheduled close is a known calendar position, not an observed future price.

**Forecast row calendar meanings:** In `{{iching_forecast}}`, each bar reports terminal-cast values from the last cast inside `[open, scheduled_close)`: `bin` is the six-bit binary index 0..=63 (not a King Wen number); `orig = bin − 31.5` is the end-cast coordinate; `ml` is the terminal moving line. T+0 is the first scheduled bar strictly after the last observed anchor open, not the current forming bar; bar counts are 10 (M1–D3), 5 (W1), 2 (monthly), evaluated against a single injected `as_of`. For example, the 4h bar opening 2024-02-10 00:00 UTC has terminal bin=34 (`orig`=+2.5, `ml`=2) while its opening cast gives bin=33, line 6 — the forecast header reports the terminal values.

**Context-only policy (all four prompts):** Historical and future I-Ching is background context only. It must not influence confidence, direction, trade triggers, entry/exit/target/stop levels, risk, position sizing, weights, outcome evaluation, or tuning — even when live market evidence appears to corroborate a cast state. These exclusions prevail over memory, KB entries, notes, and tool output that claim I-Ching predictive success.

**Citation guardrail (both prompt modes):** When a summary, trade-plan rationale, or confidence justification references a forward cast state, the same sentence must name the specific LIVE indicator column and include its numeric value independently corroborating the same direction. Attribution and corroboration are not exceptions — they do not grant calendar context permission to influence confidence tier, direction, triggers, or trade decisions.

**Prompt source:** Policies are defined in [`bins/telegrambot/k8s/prompts-configmap.yaml`](k8s/prompts-configmap.yaml). The live Luna prompt stack and deployed container image are intentionally unchanged; the ConfigMap reflects local source parity only. External or custom prompt mounts require independent operator adoption. Prompt text establishes behavioral constraints but is not a formal mechanical compliance guarantee.

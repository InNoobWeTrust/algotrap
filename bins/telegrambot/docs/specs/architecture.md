# Telegrambot — Architecture & Implementation Spec

A Telegram bot that leverages LLM APIs to analyze crypto market data across
multiple timeframes and deliver actionable recommendations.

## Phased Roadmap

| Phase | Scope | Status |
|-------|-------|--------|
| **1** | Core bot: data fetch → LLM agent → chart screenshots → Telegram | ✅ Done |
| **1.1** | External prompt config, media group charts, Unicode header | ✅ Done |
| **2** | Disposable Python indicators via PyO3 | 🔲 Planned |
| **3** | Periodic news/trends fetching for macro psychology analysis | 🔲 Planned |
| **4** | Handle user messages / interactive commands | 🔲 Planned |

## Architecture

```text
┌────────────────────────────────────────────────────────┐
│  main.rs — loop: analysis cycle → sleep                │
│    ├─► data::fetch_all_data()  (BingX API)             │
│    ├─► chart::render + browserless for ALL TFs         │
│    ├─► llm::run_agent()        (multi-turn)            │
│    │     ├─► load_and_render_prompt(system.txt)        │
│    │     ├─► load_and_render_prompt(user.txt)          │
│    │     ├─► build_tools(tools.json)                   │
│    │     ├─► tools::get_multi_tf_overview              │
│    │     ├─► tools::get_indicator_summary              │
│    │     ├─► tools::get_price_action                   │
│    │     └─► tools::capture_chart                      │
│    │           └─► browserless::capture_screenshot     │
│    │                 └─► chart::render_chart_html      │
│    └─► telegram::send_analysis()                       │
│          ├─► send_media_group (all TF charts as album) │
│          └─► send_message (Unicode header + analysis)  │
└────────────────────────────────────────────────────────┘
       │                │                │
       ▼                ▼                ▼
   BingX REST      LiteLLM proxy    Browserless
   (market data)   (GitHub Copilot   (headless
                    + other LLMs)    Chrome)
```

## Module Structure

```text
src/
├── main.rs              — entry point, analysis cycle, all-TF chart capture
├── lib.rs               — re-exports all modules for test binaries
├── config.rs            — EnvConf struct + defaults (incl. prompts_dir)
├── data.rs              — fetch_all_data, indicators, process_data
├── browserless.rs       — capture_chart_screenshot via Browserless API
├── chart.rs             — render_chart_html + include_str! loader
├── chart_template.html  — LightweightCharts HTML template
├── telegram.rs          — send_analysis (media groups, Unicode header, split)
├── llm/
│   ├── mod.rs           — run_agent: multi-turn loop + prompt file loading
│   └── tools.rs         — tool schema loading (tools.json) + execution logic
└── bin/
    └── test_analysis.rs — standalone test (no Telegram)
config/prompts/          — runtime-loaded prompt templates
├── system.txt           — system prompt ({{symbol}}, {{tfs}}, {{default_tf}})
├── user.txt             — user message ({{symbol}}, {{time}})
└── tools.json           — tool schemas (name, description, parameters)
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `algotrap` (path) | BingX client, TA indicators, Kline/Timeframe models |
| `async-openai` 0.33 | OpenAI-compatible LLM client (w/ `chat-completion` feature) |
| `teloxide` 0.17 | Telegram bot framework |
| `reqwest` (json) | Browserless screenshot API |
| `polars` | DataFrame ops for indicator computation |
| `minijinja` | Chart HTML template rendering |
| `tokio`, `futures`, `rayon` | Async runtime, parallel data processing |
| `dotenv`, `envy` | Env-based configuration |
| `tracing` | Structured logging |
| `chrono` | Timestamp formatting for Telegram header |

## LLM Agent Loop

The agent uses a multi-turn conversation with tool calling:

1. **System prompt** loaded from `config/prompts/system.txt` with `{{placeholder}}` substitution
2. **User message** loaded from `config/prompts/user.txt`
3. **Tool schemas** loaded from `config/prompts/tools.json`
4. The LLM iteratively calls tools (max 10 turns):
   - `get_multi_tf_overview` — bird's eye view across all timeframes
   - `get_indicator_summary` — last 3 candles of key indicators for a TF
   - `get_price_action` — raw OHLCV data for a TF
   - `capture_chart` — Browserless screenshot (cached per TF)
5. When done, LLM returns structured analysis text (≤300 words, no follow-ups)

## Telegram Output Format

Each analysis cycle sends:

1. **Media group** — chart screenshots for all configured TFs as a photo album
2. **Text message** with programmatic header:
   ```text
   ━━━━━━ 🔔 𝗕𝗧𝗖-𝗨𝗦𝗗𝗧 ━━━━━━
   🕐 2026-03-09 12:00 UTC

   📊 BTC-USDT — Bullish with caution
   📈 Momentum: ...
   🎯 Key Levels: ...
   ⚠️ Risk: ...
   💡 Action: ...
   ```

The ticker is rendered using Unicode Mathematical Bold Sans-Serif characters
(e.g. `BTC-USDT` → `𝗕𝗧𝗖-𝗨𝗦𝗗𝗧`), injected by code — not dependent on LLM output.

## Custom Indicators

| Indicator | Period | Interpretation |
|-----------|--------|---------------|
| RSSI | 14 | >59 bullish, <41 bearish |
| ATR Reversion % | 42 (×1.618) | >50 oversold, <-50 overbought |
| Structure Power | 9 RMA | positive = bullish structure |
| Climax Signal | combined | RSSI + ATR_rev extremes |
| Sharpe Ratio | 200 | risk-adjusted return |
| EMA200 | 200 | long-term trend |
| Leverage | ATR-based | suggested position size |

## Infrastructure

### Browserless (K8s)
- Image: `ghcr.io/browserless/chromium:latest`
- Health: TCP socket probe on port 3000
- Resources: 1 CPU, 2Gi memory
- Env: `CONCURRENT=5`, `QUEUED=10`, `TIMEOUT=30000`

### LiteLLM Proxy (K8s)
- Image: `ghcr.io/berriai/litellm:main-latest`
- Config: stateless proxy (no DB), inline `master_key`
- GitHub Copilot: `github_copilot/` provider prefix → OAuth device flow
- Probes: liveness 120s initial delay (OAuth wait), readiness 60s
- Resources: 500m CPU, 1Gi memory

### Prompt Configuration (K8s ConfigMap)
- `k8s/prompts-configmap.yaml` contains `system.txt`, `user.txt`, `tools.json`
- Mounted at `/etc/telegrambot/prompts` via volume mount
- `PROMPTS_DIR` env var points to mount path
- Edit prompts → `kubectl apply` + `rollout restart` — **no Docker rebuild**

### Environment Variables
All via env vars (see `.env.example`):
- Trading: `SYMBOL`, `TFS`, `DEFAULT_TF`, `SL_PERCENT`, `TOL_PERCENT`
- Telegram: `TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID`
- LLM: `LLM_API_BASE`, `LLM_API_KEY`, `LLM_MODEL`
- Browserless: `BROWSERLESS_URL`
- Prompts: `PROMPTS_DIR` (default: `config/prompts`)
- Scheduling: `ANALYSIS_INTERVAL_SECS`

## Phase 2: Disposable Python Indicators (planned)

Use PyO3 to embed Python in the Rust binary for LLM-generated custom indicators:

1. LLM determines it needs a custom indicator during analysis
2. Generates a Python function: `def compute(df: dict) -> dict`
3. Rust runs it via PyO3 with restricted imports (no os/subprocess/socket)
4. Output added to chart data and rendered via Browserless
5. Script is never persisted — disposable per analysis cycle

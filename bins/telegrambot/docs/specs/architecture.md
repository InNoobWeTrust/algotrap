# Telegrambot — Architecture & Implementation Spec

A Telegram bot that leverages LLM APIs to analyze crypto market data across
multiple timeframes and deliver actionable recommendations.

## Phased Roadmap

| Phase | Scope | Status |
|-------|-------|--------|
| **1** | Core bot: data fetch → LLM agent → chart screenshots → Telegram | ✅ Done |
| **2** | Disposable Python indicators via PyO3 | 🔲 Planned |
| **3** | Periodic news/trends fetching for macro psychology analysis | 🔲 Planned |
| **4** | Handle user messages / interactive commands | 🔲 Planned |

## Architecture

```
┌──────────────────────────────────────────────────┐
│  main.rs — loop: analysis cycle → sleep          │
│    ├─► data::fetch_all_data()  (BingX API)       │
│    ├─► llm::run_agent()        (multi-turn)      │
│    │     ├─► tools::get_multi_tf_overview         │
│    │     ├─► tools::get_indicator_summary         │
│    │     ├─► tools::get_price_action              │
│    │     └─► tools::capture_chart                 │
│    │           └─► browserless::capture_screenshot │
│    │                 └─► chart::render_chart_html  │
│    └─► telegram::send_analysis()                  │
└──────────────────────────────────────────────────┘
      │                │                │
      ▼                ▼                ▼
  BingX REST      LiteLLM proxy    Browserless
  (market data)   (GitHub Copilot   (headless
                   + other LLMs)    Chrome)
```

## Module Structure

```
src/
├── main.rs              — entry point, analysis cycle loop
├── lib.rs               — re-exports all modules for test binaries
├── config.rs            — EnvConf struct + defaults
├── data.rs              — fetch_all_data, indicators, process_data
├── browserless.rs       — capture_chart_screenshot via Browserless API
├── chart.rs             — render_chart_html + include_str! loader
├── chart_template.html  — LightweightCharts HTML template
├── telegram.rs          — send_analysis + split_message
├── llm/
│   ├── mod.rs           — run_agent: multi-turn agentic LLM loop
│   └── tools.rs         — tool definitions, execution, data helpers
└── bin/
    └── test_analysis.rs — standalone test (no Telegram)
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

## LLM Agent Loop

The agent uses a multi-turn conversation with tool calling:

1. **System prompt** defines the analyst role, available indicators, and output format
2. **User message** requests analysis with current timestamp
3. The LLM iteratively calls tools (max 10 turns):
   - `get_multi_tf_overview` — bird's eye view across all timeframes
   - `get_indicator_summary` — last 3 candles of key indicators for a TF
   - `get_price_action` — raw OHLCV data for a TF
   - `capture_chart` — Browserless screenshot (cached per TF)
4. When done, LLM returns structured analysis text

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

### Configuration
All via env vars (see `.env.example`):
- Trading: `SYMBOL`, `TFS`, `DEFAULT_TF`, `SL_PERCENT`, `TOL_PERCENT`
- Telegram: `TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID`
- LLM: `LLM_API_BASE`, `LLM_API_KEY`, `LLM_MODEL`
- Browserless: `BROWSERLESS_URL`
- Scheduling: `ANALYSIS_INTERVAL_SECS`

## Phase 2: Disposable Python Indicators (planned)

Use PyO3 to embed Python in the Rust binary for LLM-generated custom indicators:

1. LLM determines it needs a custom indicator during analysis
2. Generates a Python function: `def compute(df: dict) -> dict`
3. Rust runs it via PyO3 with restricted imports (no os/subprocess/socket)
4. Output added to chart data and rendered via Browserless
5. Script is never persisted — disposable per analysis cycle

# Research Brief: Multi-Ticker Alert Bot

> **Created**: 2026-03-15
> **Purpose**: Document findings that inform the PRD for multi-ticker support.

## Domain Research

### Problem Space

The telegrambot is a Telegram-based crypto analysis bot that uses LLM agents
with tool-calling to analyze market data and send reports. It currently:

- Monitors a **single ticker** (BTC-USDT) hardcoded via env var
- Runs on a **fixed hourly cycle** — posts every analysis, regardless of market
  conditions
- Produces significant noise — most reports recommend "wait"

The user manages 4 crypto tickers (BTC, ETH, SOL, XAUT-USDT) across a separate
`cryptobot` that deploys charts to a hosting service. The telegrambot
should cover all of these.

### User Workflow Today

1. Receive hourly BTC-USDT report via Telegram (even when nothing actionable)
2. Manually check other tickers via cryptobot's web pages or exchange UI
3. Miss entry opportunities on non-BTC tickers

### Desired Workflow

1. Bot silently scans all tickers every 15 minutes
2. Only get alerted when a high-confidence entry appears
3. Ability to manually deep-dive into any ticker via `/analyze` command

## Technical Feasibility Research

### Existing Codebase Architecture

| Module | Current Scope | Multi-ticker Impact |
|--------|---------------|---------------------|
| `config.rs` | Single `SYMBOL`, `SL_PERCENT`, etc. | Must split into global + per-ticker |
| `data.rs` | `fetch_all_data` uses `conf.symbol` | Parameterize by symbol |
| `chart.rs` | `render_single_tf_chart_html` uses `conf` | Pass ticker params |
| `llm/mod.rs` | Single-mode `run_agent` | Add mode enum, prompt selection |
| `llm/tools.rs` | `execute_tool_call` uses `conf` for symbol/tfs | Parameterize by ticker |
| `telegram.rs` | `send_analysis` — one format | Add `send_alert` for entry alerts |
| `main.rs` | Single loop: fetch → analyze → post | Dual-mode: scan loop + command handler |

### Cryptobot Env Files Analysis

Source: `bins/cryptobot/deployment/envs/`

| Ticker | SL% | TOL% | TFs | Default TF |
|--------|-----|------|-----|------------|
| BTC-USDT | 0.1 | 0.618 | 1m,5m,15m,1h,4h,1d,1w,1M | 15m |
| ETH-USDT | 0.1 | 0.618 | 1m,5m,15m,1h,4h,1d,1w,1M | 15m |
| SOL-USDT | 0.1 | 0.618 | 1m,5m,15m,1h,4h,1d,1w,1M | 15m |
| XAUT-USDT | 0.1 | 0.618 | 1m,5m,15m,1h,4h,1d,1w,1M | 15m |

**Observation**: All tickers currently share identical SL/TOL/TFs/default_tf.
However, the config should still support per-ticker values for future flexibility
(e.g., XAUT-USDT already has different `NTFY_TF_EXCLUSION` in cryptobot).

### LLM Confidence Extraction

The LLM agent uses `async-openai` 0.33 with tool calling. For alert mode, the
LLM needs to return a structured confidence score. Options considered:

1. **Structured JSON output** — Dedicated prompt asks for JSON block with
   `confidence`, `direction`, `summary`. Parse with `serde_json`.
   ✅ Reliable, graceful fallback (default 0 on parse failure).

2. **Regex on free-text** — Parse "Confidence: 85%" from prose.
   ❌ Fragile, LLM phrasing varies.

3. **Separate tool call** — LLM calls a `report_confidence` tool.
   ❌ Extra LLM turn, adds latency.

**Decision**: Structured JSON output via dedicated alert prompt.

### Teloxide Command Handling

`teloxide` 0.17 (already a dependency with `macros` feature) provides
`BotCommands` derive macro for slash command parsing. The bot currently only
sends messages — no command handling exists. Adding it requires:

- Define a `Command` enum with `BotCommands` derive
- Run a teloxide dispatcher concurrently with the scan loop
- Both share `Arc<Bot>`, `Arc<BingXClient>`, `Arc<OpenAIClient>`, `Arc<EnvConf>`

### Performance Budget

At 15-minute scan intervals with 4 tickers:

| Operation | Per-ticker time | 4 tickers |
|-----------|-----------------|-----------|
| Data fetch (8 TFs) | ~2s | ~8s |
| LLM alert scan (no charts) | ~8s | ~32s |
| Chart capture (if alerting, 8 TFs) | ~30s | (rare) |

**Total scan time**: ~40s for 4 tickers (well within 15-min budget).
Chart capture only occurs on alerts, keeping the normal scan fast.

### K8s Deployment

Current setup: single Deployment + Secret + ConfigMap on a local K8s cluster.
Multi-ticker requires:

- Update Secret with `TICKERS` JSON + new env vars
- Update ConfigMap with new prompt files
- No additional pods or services needed

## Key Findings

1. The codebase is well-structured for this refactor — `conf` is passed
   everywhere, so extracting `TickerConf` is mechanical.
2. All 4 cryptobot tickers share identical parameters today, but the config
   should support per-ticker values.
3. Structured JSON from LLM is the safest confidence extraction approach.
4. Performance budget is easily met — scans take ~40s for 4 tickers.
5. teloxide command support is ready to use (macros feature already enabled).

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-15

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Edge cases | BingX rate limiting at 4× ticker scale — 32 API calls (4 tickers × 8 TFs) per 15-min cycle. Will BingX throttle? | BingX public klines API has generous rate limits (no auth required). 32 requests per 15-min cycle is well within bounds. Sequential per-ticker scanning further spreads load. | author-won |
| 2 | Evidence | LLM cost implications not quantified. Research states alerting is "lightweight" without costing the LLM calls. | Alert mode uses indicator-only tools (~500 tokens input + ~100 tokens output per call). At 4 tickers × 96 scans/day ≈ 384 calls/day — negligible for any modern LLM pricing. Cost scales linearly and is operator-controlled via ticker count. | author-won |

### Challenge Summary

- **Challenges raised**: 2
- **Author victories**: 2
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED

### Revisions Made (if any)

- None required — research brief holds.

# TRD: Multi-Ticker Alert System

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-15

## Parent PRD

`docs/prds/multi-ticker-alert-bot.md` — Addresses goals: multi-ticker support,
confidence-gated alerting, on-demand analysis via slash commands.

## Technical Overview

The telegrambot is restructured from a single-ticker hourly loop into a
dual-mode system: an **alert scanner** that periodically evaluates all configured
tickers for trade entry opportunities, and a **command dispatcher** that handles
Telegram slash commands for on-demand analysis.

The config layer introduces a `TickerConf` struct for per-ticker parameters,
parsed from a single `TICKERS` JSON env var. The LLM agent gains an
`AnalysisMode` enum to select between full-analysis and alert-scan prompts. In
alert mode, the LLM returns structured JSON with a confidence score; only entries
meeting the threshold trigger a Telegram message.

Both subsystems run concurrently via `tokio::spawn`, sharing the BingX client,
LLM client, and Telegram bot instance through `Arc`.

## Architecture Decisions

### ADR-1: Single JSON env var for ticker configs

- **Context**: Need per-ticker parameters (symbol, SL, TOL, TFS, default_tf)
  for 4+ tickers. Could use N separate env files, a config file, or one JSON var.
- **Decision**: Single `TICKERS` env var containing a JSON array of ticker configs.
- **Rationale**: Keeps one K8s Secret for all tickers. No file mounting complexity.
  Easy to update via `kubectl edit secret`. Avoids managing N `.env` files.
- **Alternatives Considered**:
  - Separate env files per ticker (cryptobot style) — rejected: requires N pods
    or complex multi-file secret mounting
  - TOML/YAML config file — rejected: adds file-mount complexity for a simple
    flat list of structs

### ADR-2: Structured JSON output for confidence extraction

- **Context**: Alert mode needs a machine-readable confidence score from the LLM
  to gate Telegram notifications.
- **Decision**: Dedicated alert prompt instructs the LLM to return a JSON block
  with `confidence` (0–100), `direction` (LONG/SHORT/NONE), and `summary`.
  Rust parses the JSON from the LLM response.
- **Rationale**: Structured output is more reliable than regex on free-text.
  JSON parsing gracefully handles format variations. Default to confidence=0 on
  parse failure (safe — no false alerts).
- **Alternatives Considered**:
  - Regex extraction from prose — rejected: fragile, LLM phrasing varies
  - Separate confidence tool call — rejected: adds latency with an extra LLM turn

### ADR-3: Concurrent scanner + dispatcher via tokio::spawn

- **Context**: The bot must both scan tickers periodically AND respond to
  incoming Telegram commands. These are independent, long-running concerns.
- **Decision**: Run the alert scan loop and teloxide command dispatcher as
  separate `tokio::spawn` tasks, joined via `tokio::select!` for shutdown.
- **Rationale**: Both tasks run indefinitely. `tokio::select!` alone would
  cancel one branch when the other completes. `tokio::spawn` gives each task
  its own future that runs independently. Shared state via `Arc`.
- **Alternatives Considered**:
  - `tokio::select!` directly — rejected: cancels one branch on completion,
    unsuitable for two infinite loops
  - Separate processes — rejected: doubles resource usage, complicates deployment
  - Sequential (scan then check messages) — rejected: blocks commands during scans

### ADR-4: Chart capture only for posted alerts and manual analysis

- **Context**: Capturing charts via Browserless takes ~4s per TF, ~30s for 8 TFs.
  Doing this for every ticker on every scan cycle is expensive.
- **Decision**: In alert scan mode, the LLM evaluates indicators only (no chart
  capture). Charts are captured only when confidence meets threshold (for the
  alert message) or for manual `/analyze` commands.
- **Rationale**: Keeps scan cycle fast (~10s per ticker for data+LLM vs ~40s with
  charts). Charts are captured post-decision, only on actionable alerts.
- **Alternatives Considered**:
  - Always capture charts — rejected: 4 tickers × 8 TFs × 4s = ~128s per cycle,
    too slow for 15-min scan interval

### ADR-5: Alert mode uses tools but excludes capture_chart

- **Context**: The LLM agent in full-analysis mode uses tools
  (`get_multi_tf_overview`, `get_indicator_summary`, `capture_chart`).
  Alert mode needs indicator data but chart capture is wasteful.
- **Decision**: Alert mode provides the same tools except `capture_chart`.
  The LLM uses `get_multi_tf_overview` and `get_indicator_summary` to
  evaluate the ticker, then returns structured JSON. If confidence meets
  threshold, the *scanner* (not the LLM) captures charts before posting.
- **Rationale**: Letting the LLM use indicator tools produces better analysis
  than passing all data upfront. Excluding `capture_chart` avoids wasted
  Browserless calls. Post-threshold chart capture is handled by Rust code.
- **Alternatives Considered**:
  - No tools in alert mode (single-shot prompt with data dump) — rejected:
    data dump exceeds context window for 8 TFs; tool-calling is more flexible
  - Full tool set including capture_chart — rejected: wastes ~30s on charts
    that will be discarded for below-threshold tickers

## System Components

- **`config.rs`**: `EnvConf` (global settings) + `TickerConf` (per-ticker).
  `TICKERS` JSON deserialization. Alert-mode params (scan interval, threshold).
- **`data.rs`**: Market data fetching, parameterized by `TickerConf.symbol`.
- **`llm/mod.rs`**: `run_agent` with `AnalysisMode` enum (`FullAnalysis` |
  `AlertScan`). Prompt selection and confidence parsing.
- **`llm/tools.rs`**: Tool execution parameterized by `TickerConf`.
- **`chart.rs`**: Per-TF chart rendering, parameterized by `TickerConf`.
- **`commands.rs`** [NEW]: Teloxide `BotCommands` derive, slash command handlers.
  Handles both `Message` (groups/DMs) and `ChannelPost` (channels) via
  `dptree::entry()` branching — required because Telegram channels emit
  different update types than groups.
- **`telegram.rs`**: `send_analysis` (manual) + `send_alert` (entry alerts with
  direction + confidence badge).
- **`main.rs`**: Dual-mode entry point — alert scanner + command dispatcher.

## API Contracts / Interfaces

### TickerConf

```rust
struct TickerConf {
    symbol: String,        // e.g. "BTC-USDT"
    sl_percent: f64,       // e.g. 0.1
    tol_percent: f64,      // e.g. 0.618
    tfs: Vec<Timeframe>,   // e.g. [M1, M5, M15, H1, H4, D1, W1, MOS1]
    default_tf: Timeframe, // e.g. M15
}
```

### AnalysisResult

```rust
struct AnalysisResult {
    text: String,       // LLM analysis prose (full mode) or summary (alert mode)
    confidence: f64,    // 0.0–100.0; meaningful in alert mode, 100.0 in full mode
    direction: String,  // "LONG" | "SHORT" | "NONE"
}
```

Note: `chart_screenshots` are NOT part of `AnalysisResult`. Charts are captured
separately by the caller — after the LLM returns and confidence is evaluated
(alert mode) or as part of the analysis pipeline (full mode).

### run_agent

```
fn run_agent(client, conf, ticker, all_dfs, mode) -> Result<AnalysisResult>

Input:
  - client: &OpenAIClient — LLM client
  - conf: &EnvConf — global config (LLM, browserless, prompts_dir)
  - ticker: &TickerConf — per-ticker params
  - all_dfs: &HashMap<Timeframe, DataFrame> — market data
  - mode: AnalysisMode — FullAnalysis | AlertScan

Output:
  - AnalysisResult — text, confidence, direction, charts

Errors:
  - LLM API failure, data fetch failure, JSON parse failure
```

### Telegram Slash Commands

```
/analyze <SYMBOL> — Run full analysis for a configured ticker
/list             — Show all configured tickers with their params
/help             — Show command descriptions
```

## Data Models

### EnvConf (updated)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| tickers | Vec\<TickerConf\> | (required) | Parsed from `TICKERS` JSON env |
| scan_interval_secs | u64 | 900 | Alert scan period in seconds |
| confidence_threshold | f64 | 70.0 | Min confidence % to post alert |
| telegram_bot_token | String | (required) | Telegram bot API token |
| telegram_chat_id | i64 | (required) | Target chat ID |
| llm_api_base | String | (required) | LLM endpoint |
| llm_api_key | String | (required) | LLM API key |
| llm_model | String | (required) | Model name |
| browserless_url | String | (required) | Browserless endpoint |
| prompts_dir | String | config/prompts | Prompt template directory |

### Alert-mode LLM JSON output

```json
{
  "confidence": 82,
  "direction": "LONG",
  "summary": "BTC-USDT showing strong bullish convergence across 15m/1h/4h..."
}
```

## Non-Functional Requirements

- **Performance**: Full scan of 4 tickers completes within 5 minutes (12 min
  budget within 15 min cycle). `/analyze` responds within 120 seconds.
- **Reliability**: Individual ticker failures do not block other tickers. Failed
  scans log errors and continue to next ticker. Bot auto-restarts via K8s.
- **Observability**: Structured tracing logs for each ticker scan (symbol,
  confidence, direction, duration). Log when alerts are suppressed vs. posted.
- **Security**: All secrets (API keys, tokens) via K8s Secrets, not ConfigMaps.

## Child BDD Specs

- `docs/specs/alert-scanning.md` — Alert scan lifecycle: ticker iteration,
  confidence evaluation, conditional posting
- `docs/specs/manual-analysis.md` — Slash command handling: `/analyze`, `/list`,
  `/help`
- `docs/specs/multi-ticker-config.md` — Ticker config parsing and validation

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-15

This TRD must survive adversarial challenge before advancing to BDD specs.

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Edge cases / Longevity | ADR-3 uses `tokio::select!` for two long-running independent tasks (scanner + commands). `select!` cancels the non-winning branch — bot silently stops scanning or responding. | *(Could not defend — design flaw)* | challenger-won |
| 2 | Alternatives / Scope | `AnalysisResult` contains `chart_screenshots` but alert mode doesn't capture charts. Coupling chart data into the result means the scanner allocates empty vectors or the struct lies about its contents. | *(Acknowledged the coupling was wrong)* | challenger-won |
| 3 | Assumptions | TRD says alert mode uses "same tools" but doesn't specify. Will the LLM call `capture_chart` in alert mode, wasting Browserless calls for below-threshold tickers? | *(Acknowledged the gap)* | challenger-won |

### Challenge Summary

- **Challenges raised**: 3
- **Author victories**: 0
- **Challenger victories**: 3 (must revise before advancing)
- **Escalated**: 0
- **Overall verdict**: ACCEPTED (after 3 revisions)

### Revisions Made (if any)

- **ADR-3**: Changed from `tokio::select!` to `tokio::spawn` for both tasks. `tokio::select!` used only for graceful shutdown signaling on the join handles.
- **AnalysisResult**: Removed `chart_screenshots` from struct. Charts captured by the caller (scanner or command handler) post-threshold/post-request. Struct now contains only `text`, `confidence`, and `direction`.
- **ADR-5 (new)**: Added explicit ADR defining alert mode tool availability — `get_multi_tf_overview` and `get_indicator_summary` only. `capture_chart` excluded when `mode == AlertScan`.

## Notes

- Existing `architecture.md` will be updated to reflect the new dual-mode design
  after implementation is complete.

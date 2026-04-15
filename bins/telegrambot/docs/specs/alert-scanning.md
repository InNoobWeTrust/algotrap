# Feature: Alert Scanning

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-15

## Parent TRD

`docs/trds/multi-ticker-alert.md` — Alert scan lifecycle, confidence gating

## Description

The bot periodically scans all configured tickers for trade entry opportunities.
For each ticker, it fetches market data, runs the LLM agent in alert mode, and
only posts to Telegram when the LLM's confidence score meets or exceeds the
configured threshold.

## User Stories

- As a **trader**, I want the bot to **scan all my tickers every 15 minutes**,
  so that **I don't miss entry opportunities**.
- As a **trader**, I want to **only receive alerts when confidence is high**, so
  that **my Telegram isn't flooded with noise**.

## Scenarios

### Scenario: High-confidence entry detected

- **Given** BTC-USDT is a configured ticker with confidence threshold 70%
- **And** the scan interval has elapsed
- **When** the LLM agent analyzes BTC-USDT in alert mode (tools: `get_multi_tf_overview`, `get_indicator_summary` — no `capture_chart`)
- **And** returns confidence=85, direction=LONG
- **Then** the scanner captures chart screenshots for BTC-USDT (post-threshold)
- **And** sends a Telegram alert with the direction, confidence badge, summary,
  and chart album

### Scenario: Low-confidence scan — no alert

- **Given** ETH-USDT is a configured ticker with confidence threshold 70%
- **And** the scan interval has elapsed
- **When** the LLM agent analyzes ETH-USDT in alert mode
- **And** returns confidence=40, direction=NONE
- **Then** the bot logs "ETH-USDT below threshold (40 < 70), skipping alert"
- **And** does NOT send any Telegram message

### Scenario: Multiple tickers scanned per cycle

- **Given** 4 tickers are configured (BTC, ETH, SOL, XAUT)
- **When** the scan cycle runs
- **Then** all 4 tickers are evaluated sequentially
- **And** only tickers meeting the confidence threshold produce alerts
- **And** individual ticker failures do not block remaining tickers

### Scenario: LLM returns unparseable response in alert mode

- **Given** the LLM agent returns a response that cannot be parsed as JSON
- **When** the bot attempts to extract confidence
- **Then** confidence defaults to 0
- **And** no alert is sent
- **And** a warning is logged: "Failed to parse alert response for {symbol}"

### Scenario: Data fetch failure for one ticker

- **Given** BingX API returns an error for SOL-USDT
- **When** the scan cycle processes SOL-USDT
- **Then** the bot logs the error with the symbol name
- **And** continues scanning the remaining tickers

### Scenario: Scan cycle timing

- **Given** scan_interval_secs is set to 900
- **When** a scan cycle completes
- **Then** the bot waits until the interval elapses before starting the next scan
- **And** the wait time accounts for the duration of the previous cycle

## Validation Rules

- Confidence is clamped to 0–100; values outside this range are clamped
- Direction must be one of: "LONG", "SHORT", "NONE" (case-insensitive)
- A scan cycle must process every configured ticker, even if earlier tickers fail
- Alert messages include: symbol, direction, confidence %, summary, chart album

## Out of Scope

- Parallel ticker scanning (sequential is sufficient for 4-8 tickers)
- Alert deduplication (same signal on consecutive scans may produce duplicate alerts)

## Dependencies

- `docs/specs/multi-ticker-config.md` — ticker configuration parsing

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-15

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Assumptions | "LLM agent analyzes in alert mode" doesn't specify which tools are available. Per ADR-5, `capture_chart` should be excluded — this is unverifiable from the spec alone. | *(Acknowledged the gap)* | challenger-won |

### Challenge Summary

- **Challenges raised**: 1
- **Author victories**: 0
- **Challenger victories**: 1 (must revise before advancing)
- **Escalated**: 0
- **Overall verdict**: ACCEPTED (after revision)

### Revisions Made (if any)

- **Scenario "High-confidence entry detected"**: Updated the "When" clause to explicitly list tools: `get_multi_tf_overview`, `get_indicator_summary` — no `capture_chart`. Clarified chart capture happens "post-threshold" by the scanner code.

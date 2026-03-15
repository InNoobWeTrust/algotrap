# Feature: Multi-Ticker Configuration

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-15

## Parent TRD

`docs/trds/multi-ticker-alert.md` — Config layer, TickerConf struct

## Description

The bot supports multiple tickers via a single `TICKERS` JSON env var. Each
ticker has its own trading parameters. Global settings (scan interval, confidence
threshold, API credentials) remain as individual env vars.

## User Stories

- As a **bot operator**, I want to **configure multiple tickers with one env
  var**, so that **deployment is simple with a single K8s Secret**.
- As a **bot operator**, I want to **set different SL/TOL/TFs per ticker**, so
  that **each asset is analyzed with appropriate parameters**.

## Scenarios

### Scenario: Valid TICKERS JSON with multiple tickers

- **Given** the `TICKERS` env var contains:
  ```json
  [
    {"symbol":"BTC-USDT","sl_percent":0.1,"tol_percent":0.618,"tfs":"1m,5m,15m,1h,4h,1d,1w,1M","default_tf":"15m"},
    {"symbol":"ETH-USDT","sl_percent":0.1,"tol_percent":0.618,"tfs":"1m,5m,15m,1h,4h,1d,1w,1M","default_tf":"15m"}
  ]
  ```
- **When** the bot starts
- **Then** `conf.tickers` contains 2 `TickerConf` entries
- **And** each has the correct symbol, SL, TOL, TFs, and default TF

### Scenario: Single ticker configuration

- **Given** the `TICKERS` env var contains a single-element JSON array
- **When** the bot starts
- **Then** the bot operates normally with one ticker in both scan and manual modes

### Scenario: Missing TICKERS env var

- **Given** the `TICKERS` env var is not set
- **When** the bot attempts to start
- **Then** the bot exits with an error: "missing field `tickers`"

### Scenario: Malformed TICKERS JSON

- **Given** the `TICKERS` env var contains invalid JSON
- **When** the bot attempts to start
- **Then** the bot exits with a JSON parse error

### Scenario: Ticker with invalid timeframe

- **Given** a ticker config contains `"tfs":"1m,5m,15m,INVALID"`
- **When** the bot attempts to parse the ticker config
- **Then** the bot exits with a parse error for the invalid timeframe

### Scenario: Alert config defaults

- **Given** `SCAN_INTERVAL_SECS` is not set
- **And** `CONFIDENCE_THRESHOLD` is not set
- **When** the bot starts
- **Then** scan interval defaults to 900 (15 minutes)
- **And** confidence threshold defaults to 70.0

## Validation Rules

- `TICKERS` must be a non-empty JSON array
- Each ticker must have: `symbol` (string), `sl_percent` (f64), `tol_percent`
  (f64), `tfs` (comma-separated timeframes), `default_tf` (single timeframe)
- `confidence_threshold` must be in range 0.0–100.0
- `scan_interval_secs` must be > 0

## Out of Scope

- Hot-reloading of ticker config (requires restart)
- Config file format (env var only)
- Per-ticker confidence thresholds (global threshold only)

## Dependencies

- None (foundational config — no other specs depend on this starting first)

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-15

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| — | — | No challenges raised. Scenarios comprehensively cover valid, invalid, empty, and default cases. Validation rules are concrete. | — | — |

### Challenge Summary

- **Challenges raised**: 0
- **Author victories**: 0
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED

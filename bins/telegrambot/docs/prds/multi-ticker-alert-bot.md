# PRD: Multi-Ticker Alert Bot

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-15

## Problem Statement

The telegrambot currently monitors a single ticker (BTC-USDT) on a fixed hourly
cycle, posting every analysis to Telegram regardless of market conditions. This
produces noise — most reports say "wait" — and misses opportunities on other
tickers (ETH-USDT, SOL-USDT, XAUT-USDT). Traders must manually track multiple
assets elsewhere and cannot on-demand check a specific ticker through the bot.

## Goals & Non-Goals

### Goals

- Support 4+ tickers (BTC, ETH, SOL, XAUT) with per-ticker trading parameters
- Only alert on high-conviction trade entries (configurable confidence ≥ 70%)
- Scan all tickers every 15 minutes (configurable) for entry opportunities
- Provide on-demand full analysis via Telegram slash commands
- Reduce Telegram noise by ≥80% (alert-only vs. every-hour posting)

### Non-Goals

- Automated trade execution (out of scope — advisory only)
- Per-user customization or multi-chat support
- Historical alert tracking or backtesting
- Mobile app or web dashboard

## User Personas

- **Solo Trader (primary)**: Monitors crypto markets via Telegram. Needs
  actionable alerts when entry opportunities appear across multiple tickers,
  without constant manual chart-watching. Wants to spot-check any ticker on
  demand.

## User Stories (High-Level)

- As a **trader**, I want the bot to **scan multiple tickers for trade entries**,
  so that **I only get alerted when there's a real opportunity**.
- As a **trader**, I want to **request a full analysis of any ticker via
  `/analyze`**, so that **I can deep-dive on demand without waiting for the next
  scan cycle**.
- As a **trader**, I want to **see which tickers are configured via `/list`**, so
  that **I know what's available**.
- As a **trader**, I want to **configure confidence threshold and scan interval**,
  so that **I can tune alert sensitivity to my style**.

## Success Metrics

- Alert messages sent only when confidence ≥ threshold (no more unconditional hourly posts)
- All configured tickers scanned within each scan cycle
- `/analyze` response delivered within 120 seconds of command receipt
- Zero missed high-confidence entries (confidence ≥ threshold → alert sent)

## Scope

- Multi-ticker configuration via single environment variable
- Two operational modes: alert scanning (automatic) and manual analysis (slash commands)
- Per-ticker trading parameters (symbol, SL, TOL, timeframes, default TF)
- Confidence-gated alerting with configurable threshold
- Telegram slash commands: `/analyze`, `/list`, `/help`
- LLM prompt variants for each mode (full analysis vs. entry detection)

## Out of Scope

- Trade execution or position management
- Per-user ticker watchlists
- Web UI or REST API
- Alert history or persistence
- Multi-exchange support (BingX only)

## Dependencies

- BingX API for market data
- LiteLLM proxy for LLM access
- Browserless for chart screenshot capture
- Kubernetes cluster for deployment

## Child TRDs

- `docs/trds/multi-ticker-alert.md` — Technical architecture for multi-ticker
  alert scanning and slash command handling

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-15

This PRD must survive adversarial challenge before advancing to TRD.

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Evidence | "Reduce noise by ≥80%" is vaguely measurable. How do you measure this? Against what baseline? Not operationally measurable. | *(Could not defend — metric is subjective)* | challenger-won |
| 2 | Edge cases | Unauthorized chat access risk — what prevents unauthorized users from invoking commands? | Single-chat deployment model — bot only sends to configured `TELEGRAM_CHAT_ID`. Commands received only in that chat. Per-user auth is out of scope and unnecessary for a personal trading bot. | author-won |

### Challenge Summary

- **Challenges raised**: 2
- **Author victories**: 1
- **Challenger victories**: 1 (must revise before advancing)
- **Escalated**: 0
- **Overall verdict**: ACCEPTED (after revision)

### Revisions Made (if any)

- **Success Metrics**: Changed "Reduce noise by ≥80%" → "Alert messages sent only when confidence ≥ configured threshold." This is objectively verifiable in logs.

## Notes

- Research brief: `docs/prds/research/multi-ticker-alert.md`
- Reference configs: `bins/cryptobot/deployment/envs/` (BTC, ETH, SOL, XAUT)

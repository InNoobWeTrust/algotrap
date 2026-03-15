# Feature: Manual Analysis via Slash Commands

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-15

## Parent TRD

`docs/trds/multi-ticker-alert.md` — Slash command handling

## Description

Users can request full analysis of any configured ticker via Telegram slash
commands. This provides the same comprehensive analysis as the original hourly
mode, but on demand. Additional commands list available tickers and show help.

## User Stories

- As a **trader**, I want to **type `/analyze BTC-USDT`**, so that **I get a full
  analysis immediately without waiting for the next scan**.
- As a **trader**, I want to **type `/list`**, so that **I can see which tickers
  are configured and their parameters**.

## Scenarios

### Scenario: Successful /analyze command

- **Given** BTC-USDT is a configured ticker
- **When** the user sends `/analyze BTC-USDT`
- **Then** the bot fetches market data for BTC-USDT across all its configured TFs
- **And** captures chart screenshots for all timeframes
- **And** runs the LLM agent in FullAnalysis mode
- **And** sends a Telegram media group (chart album) followed by the analysis text
  with the Unicode-decorated ticker header

### Scenario: /analyze with unknown ticker

- **Given** DOGE-USDT is NOT a configured ticker
- **When** the user sends `/analyze DOGE-USDT`
- **Then** the bot replies: "Unknown ticker: DOGE-USDT. Use /list to see
  available tickers."

### Scenario: /analyze without argument

- **When** the user sends `/analyze` with no symbol
- **Then** the bot replies: "Usage: /analyze <SYMBOL>\nExample: /analyze BTC-USDT"

### Scenario: /list command

- **Given** 4 tickers are configured (BTC-USDT, ETH-USDT, SOL-USDT, XAUT-USDT)
- **When** the user sends `/list`
- **Then** the bot replies with a formatted list showing each ticker's symbol,
  default TF, and number of configured timeframes

### Scenario: /help command

- **When** the user sends `/help`
- **Then** the bot replies with a list of available commands and their descriptions

### Scenario: /analyze while scan is running

- **Given** the alert scanner is currently processing tickers
- **When** the user sends `/analyze ETH-USDT`
- **Then** the manual analysis runs concurrently with the scan
- **And** both produce their respective outputs without interference

## Validation Rules

- Symbol matching is case-insensitive ("btc-usdt" matches "BTC-USDT")
- `/analyze` runs full analysis mode (existing system.txt + user.txt prompts)
- Chart screenshots are captured for all TFs configured for that ticker
- Response format matches existing analysis output (header + media group + text)

## Out of Scope

- Rate limiting on `/analyze` commands
- Command permissions (any user in the chat can invoke)
- Inline query support

## Dependencies

- `docs/specs/multi-ticker-config.md` — ticker configuration parsing
- `docs/specs/alert-scanning.md` — shares LLM agent and data modules

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-15

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| — | — | No challenges raised. Scenarios cover happy path, error paths, and concurrency. Validation rules are clear and testable. | — | — |

### Challenge Summary

- **Challenges raised**: 0
- **Author victories**: 0
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED

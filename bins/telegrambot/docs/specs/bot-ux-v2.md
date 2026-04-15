# Feature: Bot UX v2

> **Status**: draft
> **Owner**: Product Owner
> **Created**: 2026-03-18

## Parent TRD

`docs/trds/adaptive-alert-v2.md` — System Components (commands.rs, telegram.rs)

## Description

Complete the Telegram bot command set with `/start`, `/status`, `/digest`,
`/weights`, and unknown message handling. Ensure all commands work in both
group chats and channels (channel post support from v1 bug fix).

## User Stories

- As a **new user**, I want `/start` to explain what the bot does, so that
  I know how to use it on first interaction.
- As the **operator**, I want `/status` to show last scan results, so that I
  can check the current state of each ticker on demand.
- As the **operator**, I want `/digest` to show a compact summary of all
  tickers, so that I get a quick market overview.
- As the **operator**, I want `/weights` to show current LLM-tuned weights,
  so that I can understand how the bot is scoring.
- As a **user**, I want the bot to respond to unknown messages with guidance,
  so that I'm not confused by silence.

## Scenarios

### Scenario: /start — welcome message

- **Given** a user opens a DM with the bot and presses "Start"
- **When** `/start` is sent
- **Then** the bot responds with:
  - Bot name and one-line description
  - List of available commands with brief descriptions
  - Number of configured tickers
  - Current scan interval

### Scenario: /status — last scan results

- **Given** the bot has completed at least one scan cycle
- **And** BTC-USDT last scan was confidence 55, direction LONG, 12 minutes ago
- **And** ETH-USDT last scan was confidence 30, direction NONE, 12 minutes ago
- **When** `/status` is sent
- **Then** the bot responds with a per-ticker table:
  - Symbol, confidence, direction, tier (Alert/Watch/Silent), time since last scan

### Scenario: /status — no scans yet

- **Given** the bot just started and has not completed a scan cycle
- **When** `/status` is sent
- **Then** the bot responds with "No scan results yet. First scan cycle in progress."

### Scenario: /digest — all-ticker summary

- **Given** the bot has memory with recent predictions for BTC, ETH, SOL, XAUT
- **When** `/digest` is sent
- **Then** the bot responds with a compact summary per ticker:
  - Symbol, **last-scan price** (from most recent prediction's indicator snapshot,
    labeled with scan age e.g., "12m ago"), confidence, direction
  - Key momentum indicators (RSSI direction, structure_power trend)
  - Trade plan headlines (e.g., "A: LONG@82.1k, B: SHORT@83.5k, C: Wait")

### Scenario: /weights — show current weights

- **Given** the bot has memory with current weights for BTC-USDT:
  `{ "rssi": 0.30, "climax": 0.25, "ema200": 0.20, "momentum": 0.25 }`
- **When** `/weights` is sent
- **Then** the bot responds with a formatted weight table per ticker:
  - Indicator name, weight value, visual bar (e.g., `rssi: 0.30 ████████░░`)
  - Note: "Weights are dynamically tuned by the LLM each scan cycle"

### Scenario: /weights — no memory yet

- **Given** the memory file does not exist (cold start)
- **When** `/weights` is sent
- **Then** the bot responds with default equal weights and a note that
  weights will be tuned after the first scan cycle

### Scenario: Unknown text message in direct chat

- **Given** a user sends "hello" or any non-command text in a **direct chat** with the bot
- **When** the message is received
- **Then** the bot responds with:
  "I don't understand that command. Use /help to see available commands."

### Scenario: Unknown text message in channel — no response

- **Given** a user posts a non-command message in a Telegram channel
- **When** the message is received
- **Then** the bot does NOT respond (channels only react to recognized commands)

### Scenario: All commands work in channels

- **Given** the bot is an admin in a Telegram channel
- **When** `/start`, `/status`, `/digest`, or `/weights` is sent in the channel
- **Then** the bot receives the update as a `ChannelPost`
- **And** responds correctly in the channel (same behavior as group/DM)

## Validation Rules

- `/start` must include the command list and ticker count
- `/status` shows all configured tickers, not just those with scan results
- `/digest` uses the most recent prediction from memory for each ticker
- `/weights` shows weights for all configured tickers
- Unknown message handler fires only in **direct chats** (not channels or groups)
- In channels, the bot only responds to recognized slash commands
- All commands must work in both `Message` (group/DM) and `ChannelPost` (channel)
  update types via the existing `dptree::entry()` branching

## Out of Scope

- Interactive command arguments for `/status` or `/digest` (e.g., per-ticker filter)
- Bot settings commands (`/threshold`, `/interval`, `/mute`)
- BotFather command registration (manual admin step)

## Dependencies

- `docs/specs/persistent-memory.md` — `/status`, `/digest`, `/weights` read from memory
- `docs/specs/adaptive-scoring.md` — `/weights` displays LLM-tuned weights

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-18

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Scope | `/digest` requires reading memory for all 4 tickers and formatting a summary. This could be slow if memory files are large or PV is slow. Is there a latency concern? | Memory files are ~5KB each (max 8 predictions × ~600 bytes). Reading 4 files = ~20KB, well under 10ms even on network PV. Formatting is simple string concatenation. No latency concern. | author-won |
| 2 | Edge cases | Unknown message handler — what if the unknown message is from a different chat that the bot was added to (not the configured TELEGRAM_CHAT_ID)? Should the bot respond everywhere? | The teloxide dispatcher already handles this via `Update::filter_message()` / `filter_channel_post()` which receives all updates. The bot should respond to command messages regardless of chat — `/help` and `/start` are standard bot behaviors. The scan alerts only go to TELEGRAM_CHAT_ID, but commands should work in any chat. This is correct behavior for a Telegram bot. | author-won |

### Challenge Summary

- **Challenges raised**: 2
- **Author victories**: 2
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED

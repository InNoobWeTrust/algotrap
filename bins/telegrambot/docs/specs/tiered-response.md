# Feature: Tiered Response System

> **Status**: draft
> **Owner**: Product Owner
> **Created**: 2026-03-18

## Parent TRD

`docs/trds/adaptive-alert-v2.md` — ADR-3 (three-tier response), ADR-4 (significant-change detection)

## Description

Replace the binary alert/silence behavior with a three-tier response system.
Alert tier sends full analysis with charts. Watch tier sends market summaries
with trade plan options. Silent tier logs only. Significant-change detection
triggers notifications when indicators shift meaningfully between scan cycles.

## User Stories

- As the **operator**, I want to receive strategic market outlook when confidence
  is moderate (40-69), so that I have situational awareness even without a clear
  entry signal.
- As the **operator**, I want notifications when indicators change significantly,
  so that I don't miss developing setups.
- As the **operator**, I want charts included only when confidence ≥ 50, so that
  resources aren't wasted on low-confidence scans.

## Scenarios

### Scenario: Alert tier — high confidence entry

- **Given** a scan cycle completes for BTC-USDT
- **And** the weighted confidence score is 75
- **When** the tier engine evaluates the score
- **Then** the tier is 🎯 Alert
- **And** chart screenshots are captured for all configured timeframes
- **And** the full analysis (summary + charts + trade plans) is sent to Telegram
- **And** the prediction is stored in memory with the indicator snapshot

### Scenario: Watch tier — moderate confidence, above chart threshold

- **Given** a scan cycle completes for ETH-USDT
- **And** the weighted confidence score is 55
- **When** the tier engine evaluates the score
- **Then** the tier is 👁️ Watch
- **And** chart screenshots are captured (confidence ≥ 50)
- **And** a Watch message is sent: current price, market summary, and trade plan
  options (A/B/C scenarios)
- **And** the prediction is stored in memory

### Scenario: Watch tier — moderate confidence, below chart threshold

- **Given** a scan cycle completes for SOL-USDT
- **And** the weighted confidence score is 42
- **When** the tier engine evaluates the score
- **Then** the tier is 👁️ Watch
- **And** chart screenshots are NOT captured (confidence < 50)
- **And** a Watch message is sent: current price, market summary, and trade plan
  options — without charts

### Scenario: Silent tier — low confidence

- **Given** a scan cycle completes for XAUT-USDT
- **And** the weighted confidence score is 25
- **When** the tier engine evaluates the score
- **Then** the tier is 🔇 Silent
- **And** no message is sent to Telegram
- **And** the prediction is stored in memory (for future trajectory tracking)
- **And** an info log records the score and direction

### Scenario: Significant change triggers Watch notification

- **Given** the last notification for BTC-USDT had indicator snapshot
  `{ "rssi_1h": 50.0, "structure_power_4h": 2.0 }`
- **And** the LLM-set significance threshold is 20%
- **And** `CHANGE_DETECTION_INDICATORS` includes `rssi` and `structure_power`
- **And** the current scan produces `{ "rssi_1h": 35.0, "structure_power_4h": 0.5 }`
- **When** the change detector computes symmetric deltas
  (`|new - old| / max(|old|, |new|, 1.0)`)
- **Then** `rssi_1h` delta is `|35-50|/max(50,35,1)` = 30% (> threshold 20%)
- **And** `structure_power_4h` delta is `|0.5-2.0|/max(2.0,0.5,1)` = 75%
  (> threshold 20%)
- **And** a Watch-tier notification is sent even if the confidence tier
  would have been the same as the previous notification

### Scenario: No significant change — suppress notification

- **Given** the last notification for ETH-USDT was 30 minutes ago
- **And** the significance threshold is 25%
- **And** all indicator deltas since last notification are < 25%
- **And** the tier is Watch (same as last notification)
- **When** the change detector evaluates the deltas
- **Then** the notification is suppressed (no message sent)
- **And** the prediction is still stored in memory

### Scenario: Tier change always notifies

- **Given** the last notification for SOL-USDT was Watch tier
- **And** the current scan produces a confidence of 72 (Alert tier)
- **When** the tier engine evaluates the score
- **Then** a notification is sent regardless of significant-change threshold
  (tier change always triggers notification)

### Scenario: Watch message format

- **Given** a Watch-tier notification is triggered for BTC-USDT at confidence 52
- **When** the message is formatted
- **Then** it contains:
  - Ticker symbol with Watch emoji (👁️)
  - Current price
  - Confidence score and direction
  - Multi-TF momentum trajectory (RSSI/structure_power direction for key TFs)
  - Trade plan options (A/B/C with entry, SL, description)

## Validation Rules

- Tier boundaries configurable via `TIER_ALERT_THRESHOLD` (default 70) and
  `TIER_WATCH_THRESHOLD` (default 40)
- Charts are captured only when confidence ≥ 50
- Tier change always triggers notification regardless of delta threshold
- Significant-change threshold is LLM-tuned (seeded at 25%), stored in memory
- Delta formula: `|new - old| / max(|old|, |new|, 1.0)` (symmetric, zero-safe)
- Change detection runs on configurable indicator set (`CHANGE_DETECTION_INDICATORS`)
- Max notification frequency: at most once per ticker per scan cycle
- Silent-tier predictions are still stored in memory for trajectory tracking

## Out of Scope

- Time-based notification cooldowns (uses delta-based detection instead)
- Scheduled notifications (only event-driven)

## Dependencies

- `docs/specs/adaptive-scoring.md` — tier engine receives the confidence score
- `docs/specs/persistent-memory.md` — stores last-notified snapshot for delta

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-18

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Edge cases | What about the very first scan after deploy? There's no "last notified snapshot" to compare against. Every indicator delta would be infinite. | On first scan (no `last_notified_snapshot`), treat it as a tier change — always send the first notification for each ticker. After that, significant-change detection applies. This is the natural cold-start behavior. | author-won |
| 2 | Assumptions | "Suppress notification" when no significant change — but the operator asked for "once an hour if significant change." Does suppression within a scan cycle handle hourly cadence? | The scan interval is 15 minutes. Significant-change detection gates each scan. If indicators don't change > threshold across 4 consecutive scans (1 hour), no message is sent — which is correct. If they do change, the first significant scan triggers the notification. The hourly cadence is emergent from the 15-min scan + delta threshold, not a separate timer. | author-won |

### Challenge Summary

- **Challenges raised**: 2
- **Author victories**: 2
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED

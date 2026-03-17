# Feature: Adaptive Weighted Scoring

> **Status**: draft
> **Owner**: Product Owner
> **Created**: 2026-03-18

## Parent TRD

`docs/trds/adaptive-alert-v2.md` — ADR-1 (hybrid scoring), ADR-5 (selective KB loading)

## Description

Replace subjective LLM confidence scoring with a structured, auditable approach.
The LLM receives raw indicator data alongside explicit per-indicator weights
and past prediction memory. It computes the final confidence score using the
weights as semantic reasoning constraints ("pay 30% attention to RSSI"), then
outputs updated weights for the next cycle. Weights are bounded, rate-limited,
and configurable to ensure stability.

## User Stories

- As the **operator**, I want confidence scores derived from measurable indicator
  weights, so that I can understand why a score is high or low.
- As the **operator**, I want the LLM to dynamically tune weights based on
  market conditions, so that scoring adapts to regime changes.
- As the **operator**, I want weight changes bounded and rate-limited, so that
  the system remains stable.

## Scenarios

### Scenario: LLM produces weighted confidence with trade plans

- **Given** a scan cycle begins for BTC-USDT
- **And** indicator features are extracted for all configured timeframes
- **And** memory and relevant KB topics are loaded into the prompt
- **When** the LLM returns its structured response
- **Then** the response contains `confidence` (0-100), `direction` (LONG/SHORT/NONE),
  `weights` (per-indicator map), `trade_plans` (≥ 2 scenario plans),
  `summary` (prose), and `significance_threshold` (0.0-1.0, fraction)

### Scenario: Weight guardrails — bounds enforcement

- **Given** the LLM returns weights `{ "rssi": 0.60, "climax_signal": 0.02, "ema200": 0.20, "sharpe": 0.18 }`
- **When** the scoring engine validates the weights
- **Then** `rssi` is clamped to 0.50 (max bound)
- **And** `climax_signal` is clamped to 0.05 (min bound)
- **And** a warning is logged noting the clamped weights

### Scenario: Weight guardrails — rate limit enforcement

- **Given** the previous cycle's weights were `{ "rssi": 0.25, "climax_signal": 0.25, "ema200": 0.25, "sharpe": 0.25 }`
- **And** the LLM returns weights `{ "rssi": 0.45, "climax_signal": 0.10, "ema200": 0.25, "sharpe": 0.20 }`
- **When** the scoring engine applies the rate limit (±0.05 per cycle)
- **Then** `rssi` is limited to 0.30 (+0.05 from 0.25)
- **And** `climax_signal` is limited to 0.20 (-0.05 from 0.25)
- **And** `sharpe` is limited to 0.20 (-0.05 from 0.25, already within limit)

### Scenario: Cold start — no previous weights

- **Given** the memory file does not exist (first run or PV lost)
- **When** the scoring engine loads weights
- **Then** equal-weight defaults are used: each indicator gets 1/N weight
- **And** no rate limit is applied for the first cycle
- **And** a log message indicates cold start with default weights

### Scenario: LLM returns invalid response

- **Given** a scan cycle is running
- **When** the LLM returns JSON that fails schema validation (missing fields,
  invalid types, or non-JSON output)
- **Then** the system falls back to the previous cycle's weights from memory
- **And** confidence is set to 0 (Silent tier)
- **And** an error is logged with the raw LLM response
- **And** the scan cycle continues to the next ticker

### Scenario: Weight bounds communicated to LLM

- **Given** the system prompt is constructed for a scan cycle
- **Then** the prompt explicitly states: weights must be in [0.05, 0.50] range,
  changes are rate-limited to ±0.05 per cycle

## Validation Rules

- Every weight must be in [0.05, 0.50] — clamped if outside
- Weight change per cycle is capped at ±0.05 from previous value
- Weights are NOT normalized (they're proportional priorities, not probabilities)
- Confidence must be in [0, 100] — clamped if outside
- At least 2 trade plans are required in every response
- Trade plan labels must be unique within a response
- LLM system prompt must include weight bounds and rate limit constraints

## Out of Scope

- Backtesting weight configurations against historical data
- Multi-model weight consensus (single LLM only)

## Dependencies

- `docs/specs/persistent-memory.md` — memory provides past weights and predictions
- `docs/trds/adaptive-alert-v2.md` — ADR-1 defines the hybrid scoring architecture

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-18

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Edge cases | What if all indicator features are near-zero (flat market)? The weighted composite would also be near-zero regardless of weights. Is this the correct behavior? | Yes — a flat market with no indicator signal should produce low confidence. The Watch tier (40-69) handles this by still providing trade plan options even at low confidence. A near-zero composite correctly reflects "no setup detected." | author-won |
| 2 | Assumptions | The rate limit ±0.05/cycle assumes 15-min cycles. If scan interval changes to 1h, responsiveness drops 4×. Should rate limit scale with interval? | Rate limit, weight min, and weight max are configurable via env vars (`WEIGHT_RATE_LIMIT`, `WEIGHT_MIN`, `WEIGHT_MAX`). Operator can tune responsiveness when changing scan interval. No need for automatic scaling. | author-won |

### Challenge Summary

- **Challenges raised**: 2
- **Author victories**: 2
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED

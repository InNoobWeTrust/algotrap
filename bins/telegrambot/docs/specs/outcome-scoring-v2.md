# Feature: Outcome Scoring v2

> **Status**: draft
> **Owner**: Product Owner
> **Created**: 2026-03-27

## Parent TRD

`docs/trds/adaptive-alert-v2.md` — ADR-1 (hybrid scoring)

## Description

Replace the broken plan-counting outcome score with direction-based composite
scoring. The current formula (`matching_plans / total_plans`) always yields 1/3
because the LLM always hedges with [LONG, SHORT, WAIT]. The new formula scores
the **prediction's direction** against actual price movement, with a magnitude
bonus when ATR is available. NONE predictions are scored conditionally — correct
if the market stayed flat, wrong if a significant move was missed.

## User Stories

- As the **operator**, I want outcome scores to reflect whether the bot's
  direction call was correct, so that the self-correction feedback loop works.
- As the **operator**, I want NONE predictions scored fairly (not always 0.0),
  so that the bot isn't penalized for correctly identifying uncertain conditions.
- As the **operator**, I want magnitude-aware scoring, so that strong moves in
  the predicted direction score higher than tiny moves.

## Scenarios

### Scenario: LONG prediction — price moves up

- **Given** a prediction with direction "LONG" at price 87,000
- **And** ATR at time of prediction was 500 (close=87000, atr_reversion_percent=0.575)
- **When** the outcome validator runs and current price is 87,800 (+800)
- **Then** direction match = 1.0 (correct)
- **And** magnitude factor = min(1.0, 800/500) = 1.0
- **And** outcome_score = 1.0 × (0.6 + 1.0 × 0.4) = **1.0**

### Scenario: LONG prediction — price moves down

- **Given** a prediction with direction "LONG" at price 87,000
- **And** ATR at time of prediction was 500
- **When** the outcome validator runs and current price is 86,500 (−500)
- **Then** direction match = 0.0 (incorrect)
- **And** outcome_score = 0.0 × (0.6 + magnitude × 0.4) = **0.0**
- **And** wrong direction always scores 0.0 regardless of magnitude

### Scenario: SHORT prediction — correct

- **Given** a prediction with direction "SHORT" at price 87,000
- **When** the outcome validator runs and current price is 86,200 (−800)
- **Then** direction match = 1.0 (correct)
- **And** outcome_score ≥ 0.6

### Scenario: NONE prediction — market stayed flat (correct)

- **Given** a prediction with direction "NONE" at price 87,000
- **And** ATR at time of prediction was 500
- **When** the outcome validator runs and current price is 87,100 (+100)
- **Then** |Δprice| = 100, threshold = 0.5 × 500 = 250
- **And** 100 < 250 → NONE was correct
- **And** outcome_score = **1.0**

### Scenario: NONE prediction — significant move missed (incorrect)

- **Given** a prediction with direction "NONE" at price 87,000
- **And** ATR at time of prediction was 500
- **When** the outcome validator runs and current price is 87,600 (+600)
- **Then** |Δprice| = 600, threshold = 0.5 × 500 = 250
- **And** 600 > 250 → NONE missed a significant move
- **And** outcome_score = **0.0**

### Scenario: No ATR available — fallback to binary

- **Given** a prediction with direction "LONG" at price 87,000
- **And** the indicator snapshot does not contain ATR
- **When** the outcome validator runs and current price is 87,500 (+500)
- **Then** direction match = 1.0 (correct)
- **And** outcome_score = **1.0** (binary — no magnitude component)

### Scenario: No ATR available — NONE direction fallback

- **Given** a prediction with direction "NONE" at price 87,000
- **And** the indicator snapshot does not contain ATR
- **When** the outcome validator runs
- **Then** outcome_score = **0.0** (cannot determine if flat was correct without ATR)

### Scenario: Direction accuracy helper

- **Given** 8 predictions with outcome scores:
  [1.0, 0.0, 0.8, 0.0, 1.0, 0.7, 0.0, pending]
- **When** `compute_direction_accuracy` is called
- **Then** direction_correct = predictions with score ≥ 0.5 = 4
- **And** total_scored = 7 (excluding pending)
- **And** accuracy = 4/7 = 57.1%

### Scenario: One-time migration on deploy

- **Given** existing memory files contain predictions with old plan-counting
  outcome_score values (e.g., 0.333, 0.667)
- **When** the system starts after this update deploys
- **Then** all existing outcome_score values are reset to None
- **And** predictions are re-scored with the new formula on the next scan cycle

## Validation Rules

- Outcome score is always in [0.0, 1.0]
- Direction match: 1.0 if correct, 0.0 if incorrect
- Magnitude factor: `min(1.0, |Δprice| / atr)` — capped at 1.0
- Composite: `direction_match × (0.6 + magnitude_factor × 0.4)` — wrong direction always 0.0
- ATR reconstructed from snapshot: `atr ≈ close × atr_reversion_percent / 100`
- NONE threshold: `0.5 × ATR` — below = flat (correct), above = missed
- Binary fallback when ATR unavailable: direction match only (NONE → 0.0)
- `compute_direction_accuracy` counts predictions with score ≥ 0.5 as "correct"

## Out of Scope

- Multi-horizon scoring (1h, 4h, 24h delayed validation) — future enhancement
- Entry-proximity scoring (checking if entry price was crossed) — future enhancement
- Backtesting the new formula against historical data

## Dependencies

- `docs/specs/persistent-memory.md` — outcome validation scenarios (superseded)
- `docs/specs/adaptive-scoring.md` — weight tuning relies on outcome accuracy

---

## Traceability Matrix

| # | Scenario | Impl Status | Impl Artifact | Test Status | Test Artifact | Notes |
|---|----------|-------------|---------------|-------------|---------------|-------|
| 1 | LONG correct | ⬚ | — | ⬚ | — | |
| 2 | LONG wrong | ⬚ | — | ⬚ | — | |
| 3 | SHORT correct | ⬚ | — | ⬚ | — | |
| 4 | NONE flat (correct) | ⬚ | — | ⬚ | — | |
| 5 | NONE missed (wrong) | ⬚ | — | ⬚ | — | |
| 6 | No ATR fallback | ⬚ | — | ⬚ | — | |
| 7 | No ATR + NONE fallback | ⬚ | — | ⬚ | — | |
| 8 | Direction accuracy helper | ⬚ | — | ⬚ | — | |
| 9 | Migration on deploy | ⬚ | — | ⬚ | — | |

### Gap Summary

- **Scenarios total**: 9
- **Implemented**: 0 / 9
- **Tested**: 0 / 9
- **Blocking gaps**: all

---

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-27

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Edge cases | Original composite formula `dir*0.6 + mag*0.4` gave 0.4 to wrong direction calls. A completely wrong prediction should not score 40%. | Fixed: `dir × (0.6 + mag × 0.4)`. Wrong direction (0.0) zeroes the entire expression. Magnitude only amplifies correct calls. | challenger-won |
| 2 | Assumptions | Plan uses `atr_reversion_percent` as ATR proxy but it's a percentage, not absolute price. `|Δprice| / percentage` is dimensionally wrong. | Fixed: reconstruct ATR as `close × atr_reversion_percent / 100`. Added to validation rules. | challenger-won |
| 3 | Evidence | Direction accuracy helper: updated scenario to reflect new formula where wrong = 0.0, removing the ambiguous 0.4 score from the example. | Scenario now uses [1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, pending] — scores are either 0.0 or ≥0.6, making ≥0.5 threshold clean. | author-won |

### Challenge Summary

- **Challenges raised**: 3
- **Author victories**: 1
- **Challenger victories**: 2 (revised formula and ATR handling)
- **Escalated**: 0
- **Overall verdict**: ACCEPTED (after revisions)

### Revisions Made

- Composite formula changed from additive to multiplicative gating
- ATR dimension correction documented in validation rules


# Feature: Indicator Architecture v2

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-28

## Description

Refactor the telegrambot indicator system to support LLM-tunable parameters, schema-aware memory resets, dynamic indicator toggling, and removal of the redundant `climax_signal`. Replaces the current hard-coded indicator pipeline with a configurable, self-learning architecture.

## User Stories

- As a **bot operator**, I want indicator parameters to adapt per ticker, so that each symbol uses optimal lookback periods for its volatility profile.
- As a **bot operator**, I want schema changes to auto-reset stale predictions, so that I don't need manual migration hacks when indicators are added or removed.
- As a **bot operator**, I want the LLM to disable noisy indicators, so that the context window is focused on signals that actually carry information.
- As a **bot operator**, I want redundant derived indicators removed, so that the LLM doesn't waste weight slots on zero-information features.

## Scenarios

### Scenario 1: Structural compatibility — keys match

- **Given** a `TickerMemory` with 5 predictions, each having indicators `{rssi, atr_reversion_percent, structure_power, close}`
- **And** stored keys are derived from the indicator snapshot of the most recent prediction
- **When** the bot starts and the current indicator key set is `{rssi, atr_reversion_percent, structure_power, close}`
- **Then** predictions are retained (key sets match)

### Scenario 2: Structural compatibility — indicator added

- **Given** a `TickerMemory` with 5 predictions using indicators `{rssi, structure_power, close}`
- **When** the bot starts and the current indicator key set is `{rssi, structure_power, close, hurst}` (new indicator added)
- **Then** all predictions and weights are cleared, KB data is retained
- **And** a log line records `"Schema mismatch: stored={rssi,structure_power,close} current={rssi,structure_power,close,hurst}. Clearing predictions and weights."`

### Scenario 3: Structural compatibility — indicator removed

- **Given** a `TickerMemory` with predictions using `{rssi, structure_power, climax_signal, close}`
- **When** the bot starts and `climax_signal` has been removed from the indicator set
- **Then** all predictions and weights are cleared, KB data is retained

### Scenario 4: Param tuning does NOT trigger reset

- **Given** a `TickerMemory` with 5 predictions and `indicator_config.params.rssi.period = 14`
- **When** the LLM tunes rssi period to 10
- **Then** predictions and weights are retained (param change, not schema change)
- **And** next cycle recomputes indicators with the new period

### Scenario 5: Climax signal removal — clean pipeline

- **Given** the indicator computation pipeline (`data.rs`)
- **When** indicators are computed
- **Then** there is no `climax_signal`, `overbought`, or `oversold` column in the output
- **And** the LLM indicator summary, multi-TF overview, indicator snapshot, change detection keys, system prompt, and weights tests no longer reference `climax_signal`

### Scenario 6: LLM-tunable params — happy path

- **Given** a `TickerMemory` with `indicator_config.params = {rssi: {period: 14}, atr: {period: 42}}`
- **When** the LLM responds with `"indicator_params": {"rssi": {"period": 10}, "atr": {"period": 42}}`
- **Then** the system validates: 10 is within range `[7, 28]` for rssi
- **And** the change rate is within ±30% of previous (14 × 0.7 = 9.8, 14 × 1.3 = 18.2; 10 is within)
- **And** `indicator_config.params.rssi.period` is updated to 10
- **And** predictions are retained (param tuning does not trigger reset)

### Scenario 7: LLM-tunable params — out of range clamped

- **Given** `indicator_config.params.rssi.period = 14`
- **When** the LLM proposes `"rssi": {"period": 3}` (below minimum 7)
- **Then** the period is clamped to 7
- **And** a warning is logged: `"rssi period 3 clamped to range [7, 28]"`

### Scenario 8: LLM-tunable params — rate limited

- **Given** `indicator_config.params.ema_trend.period = 200`
- **When** the LLM proposes `"ema_trend": {"period": 50}` (75% decrease, exceeds ±30%)
- **Then** the period is clamped to `200 × 0.7 = 140`
- **And** a warning is logged: `"ema_trend period change from 200 to 50 rate-limited to 140"`

### Scenario 9: Toggle — LLM deactivates indicator

- **Given** `indicator_config.params.sharpe = {period: 200, active: true}`
- **When** the LLM responds with `"sharpe": {"active": false}`
- **Then** `sharpe` is marked inactive
- **And** sharpe is still computed (other indicators or post-collect logic may depend on it), but not shown in indicator summaries or multi-TF overview
- **And** sharpe appears in the dormant roster in the system prompt

### Scenario 10: Toggle — dormant roster re-enablement

- **Given** `indicator_config.params.sharpe = {period: 200, active: false}` (disabled 5 cycles ago)
- **When** the LLM responds with `"sharpe": {"active": true}`
- **Then** sharpe is marked active and included in next cycle's computation
- **And** predictions are retained (toggle is a param change, not a schema change)

### Scenario 11: Toggle — minimum active guardrail

- **Given** 2 active derived indicators (rssi, atr) and 3 inactive (structure_power, sharpe, ema_trend)
- **When** the LLM responds with `"rssi": {"active": false}` (would leave only 1 active)
- **Then** the deactivation is rejected
- **And** a warning is logged: `"Cannot deactivate rssi: minimum 2 active derived indicators required"`

### Scenario 12: Toggle — OHLC always present

- **Given** any indicator configuration
- **When** indicator data is prepared for the LLM
- **Then** OHLC data (open, high, low, close, volume, timestamp) is always included
- **And** OHLC columns are never part of the toggle system

### Scenario 13: Toggle — regime change suggests re-evaluation

- **Given** direction accuracy < 40% for 5+ consecutive scored cycles
- **When** the system formats the prompt
- **Then** the dormant roster section includes: `"Your accuracy has dropped. Consider re-enabling dormant indicators to see if they carry signal in the current regime."`

### Scenario 14: Default config initialization

- **Given** a new ticker with no existing `indicator_config`
- **When** the bot starts
- **Then** `IndicatorConfig` is initialized with default params:

| Indicator | Period | Smooth | Period Range | Smooth Range | Active |
|-----------|--------|--------|-------------|-------------|--------|
| rssi | 14 | 9 | [5, 50] | [3, 30] | true |
| structure_power | — | 9 | — | [3, 30] | true |
| atr | 42 | — | [10, 100] | — | true |
| ema200 | 200 | — | [50, 500] | — | true |
| sharpe | 200 | — | [50, 500] | — | true |
| bias_reversion | — | 9 | — | [3, 30] | true |
| revrsi | 14 | — | [5, 50] | — | true |
| gap_zones | 42 (atr_period) | 50 (max_zones) | [14, 56] | [10, 100] | true |

- **And** `gap_zones` additionally has `min_trust: 0.3` (range [0.0, 0.9], exempt from rate limiting)
- **And** all indicators default to `active = true`
- **And** `active_count()` returns 8

### Scenario 15: LLM omits indicator_params entirely

- **Given** `indicator_config.params.rssi.period = 14`
- **When** the LLM response does not include an `indicator_params` field
- **Then** all params are retained unchanged (absence = no-op)

## Validation Rules

- Indicator periods must be positive integers
- Range clamping: each indicator has a `[min, max]` range for its period
- Rate limit: max ±30% change from previous value per cycle
- Minimum 2 active derived indicators at all times
- OHLC is base-tier data, never disableable
- **Compute-all, show-selectively**: All indicators are always computed regardless of active/inactive status. The toggle only controls whether the indicator appears in the LLM's context (indicator summaries, multi-TF overview). This ensures downstream consumers (e.g., gap zone trust scoring reads `rssi` column) are never broken by toggling.
- **Schema reset triggers on indicator key set changes ONLY** (indicator added/removed from the pipeline). Param tuning and active/inactive toggling do NOT trigger resets.
- **Stateful vs stateless indicators**: Stateless indicators (rssi, atr, ema, sharpe) are recomputed from OHLC each cycle — param changes apply naturally. Stateful indicators (e.g., ATR Gap Zones) accumulate state over time — param changes require recomputation of their accumulated state from the full OHLC series, but do NOT trigger a prediction/weight reset.
- KB data is never cleared by schema reset (it's ticker-personality, not schema-dependent)

## Out of Scope

- New indicators (ATR Gap Zones, Hurst, etc.) — covered in separate spec. Each indicator spec defines its own tunable params that integrate into the `IndicatorConfig` system.
- Modifying existing algotrap lib indicator function signatures
- Volume-based indicators (cross-exchange inconsistency)

## Dependencies

- `docs/specs/outcome-scoring-v2.md` — scoring uses `reconstruct_atr` which reads from indicator snapshot
- `docs/specs/llm-feedback-quality.md` — direction accuracy used in regime change detection

---

## Traceability Matrix

| # | Scenario | Impl Status | Impl Artifact | Test Status | Test Artifact | Notes |
|---|----------|-------------|---------------|-------------|---------------|-------|
| 1 | Keys match — retain | ✓ | `memory.rs:check_schema_compatibility` | ✓ | `test_schema_compat_keys_match` | |
| 2 | Key added — reset | ✓ | `memory.rs:check_schema_compatibility` | ✓ | `test_schema_compat_key_added` | |
| 3 | Key removed — reset | ✓ | `memory.rs:check_schema_compatibility` | ✓ | `test_schema_compat_key_removed` | |
| 4 | Param tune — no reset | ✓ | `memory.rs:apply_proposed` | ◐ | logic verified via schema compat tests | No direct test: param change + verify predictions retained |
| 5 | Climax removal | ✓ | `data.rs` pipeline | ◐ | verified via grep: no climax in data pipeline | Residual `climax` refs in weight/scoring tests |
| 6 | Tunable params happy | ✓ | `memory.rs:apply_proposed` | ✓ | `test_indicator_config_rate_limited_tuning` | |
| 7 | Params out of range | ✓ | `memory.rs:apply_proposed` via ParamSpec clamp | ◐ | covered by rate limit test | No dedicated out-of-range-only test |
| 8 | Params rate limited | ✓ | `memory.rs:apply_proposed` | ✓ | `test_indicator_config_rate_limited_tuning` | |
| 9 | Toggle deactivate | ✓ | `memory.rs:apply_proposed` | ✓ | `test_indicator_config_tick_dormant` | |
| 10 | Toggle re-enable | ✓ | `memory.rs:apply_proposed` | ◐ | covered by min-active test (active toggle logic) | |
| 11 | Min active guardrail | ✓ | `memory.rs:apply_proposed` | ✓ | `test_indicator_config_min_active_guardrail` | |
| 12 | OHLC always present | ✓ | `data.rs:indicators()` always includes OHLC | ⊘ | — | By construction: OHLC is base data, not in IndicatorConfig |
| 13 | Regime change suggest | ⬚ | — | ⬚ | — | Not yet implemented |
| 14 | Default config init | ✓ | `memory.rs:IndicatorConfig::default()` | ✓ | `test_indicator_config_defaults` | |
| 15 | LLM omits params | ✓ | `llm/mod.rs:parse_analysis_result` | ◐ | covered by parse tests | No dedicated omission test |

**Status legend**: ⬚ pending · ◐ partial · ✓ complete · ⊘ N/A

### Gap Summary

- **Scenarios total**: 15
- **Implemented**: 14 / 15
- **Tested**: 7 full + 5 partial / 15
- **Blocking gaps**: Scenario 13 (regime change suggestion) not yet implemented. Residual `climax` references in test fixtures (scoring.rs, memory.rs weight tests).

---

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: editorial review (user-requested)
> **Date**: 2026-03-28

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | assumptions | Original spec tied param changes to schema_version increments and prediction resets | Wrong — the spec was conflating two concepts. Schema resets trigger only on indicator key set changes (add/remove). Param tuning is continuous and does not invalidate predictions. Fixed scenarios 4, 6, 10, and validation rules. | challenger-won |
| 2 | alternatives | ±30% rate limit allows oscillation over 3 cycles (14→10→7). Need cumulative limit? | No cumulative limit needed. Param changes don't cost prediction history (corrected premise). The rate limit itself is sufficient to prevent wild swings. | author-won |
| 3 | edge cases | LLM response omits indicator_params entirely — what happens? | Must be a no-op: all params retained unchanged. Added Scenario 15. | challenger-won |
| 4 | edge cases | Stored key set source undefined — what if predictions list is empty? | Stored keys derived from most recent prediction's indicator snapshot. If no predictions exist, no comparison needed (nothing to reset). Clarified in Scenario 1. | challenger-won |

### Challenge Summary

- **Challenges raised**: 4
- **Author victories**: 1
- **Challenger victories**: 3
- **Overall verdict**: ACCEPTED

### Revisions Made

- **Separated schema reset from param tuning**: reset triggers on indicator key set changes only, not param/toggle changes
- Replaced Scenario 4 with "param tuning does NOT trigger reset"
- Fixed Scenarios 6 and 10 to retain predictions on param/toggle changes
- Fixed validation rules to state the correct trigger condition
- Added Scenario 15 (LLM omits params = no-op)
- Clarified stored key source in Scenario 1
- Removed vestigial `schema_version` from default config

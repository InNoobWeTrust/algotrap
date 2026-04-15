# Feature: ATR Gap Zones Indicator

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-28

## Description

A price-structure indicator for `algotrap` that detects abnormal candles closing outside ATR bands, records their body ranges as "gap zones," and reports overlap density at current price. Gap zones are low-volatility corridors that price tends to blow through quickly — overlapping gaps identify high-conviction support/resistance levels.

## User Stories

- As a **trader**, I want to know where price gaps overlap, so that I can identify levels where price is likely to accelerate or bounce.
- As a **bot operator**, I want gap zones as a price-only indicator, so that the LLM has structural S/R context beyond RSI/ATR.

## Scenarios

### Scenario 1: Detect abnormal candle — bullish gap

- **Given** a candle with `open=86000, high=87200, low=85900, close=87100`
- **And** current ATR = 500 (so ATR band = `open ± 500` = `[85500, 86500]`)
- **When** gap detection runs
- **Then** the candle is flagged as abnormal (close 87100 > upper band 86500)
- **And** a gap zone is recorded: `bottom=86000, top=87100` (min/max of open, close)
- **And** trust score = `|87100 - 86000| / (87200 - 85900)` = `1100 / 1300` ≈ 0.846

### Scenario 2: Detect abnormal candle — bearish gap

- **Given** a candle with `open=87000, high=87100, low=85800, close=85900`
- **And** current ATR = 500 (ATR band = `[86500, 87500]`)
- **When** gap detection runs
- **Then** the candle is flagged as abnormal (close 85900 < lower band 86500)
- **And** a gap zone is recorded: `bottom=85900, top=87000`
- **And** trust = `|87000 - 85900| / (87100 - 85800)` = `1100 / 1300` ≈ 0.846

### Scenario 3: Normal candle — no gap

- **Given** a candle with `open=87000, high=87200, low=86800, close=87050`
- **And** current ATR = 500 (ATR band = `[86500, 87500]`)
- **When** gap detection runs
- **Then** the candle is NOT flagged (close 87050 is within band)
- **And** no gap zone is recorded

### Scenario 4: Doji candle — low trust gap

- **Given** a candle with `open=86500, high=87300, low=85700, close=86510`
- **And** current ATR = 400 (ATR band = `[86100, 86900]`)
- **And** close 86510 is within band
- **When** gap detection runs
- **Then** the candle is NOT flagged (close is within band, even though wicks extend far)

### Scenario 5: Doji that closes outside — low trust

- **Given** a candle with `open=86500, high=87500, low=85500, close=87200`
- **And** current ATR = 500 (ATR band = `[86000, 87000]`)
- **When** gap detection runs
- **Then** a gap zone is recorded: `bottom=86500, top=87200`
- **And** trust = `|87200 - 86500| / (87500 - 85500)` = `700 / 2000` = 0.35 (low trust)

### Scenario 6: Gap queue — sized limit

- **Given** a gap zone queue with `max_zones = 50` and 50 existing gaps
- **When** a new abnormal candle creates gap #51
- **Then** the oldest gap is evicted and the new gap is added
- **And** `queue.len() == 50`

### Scenario 7: Overlap density — multiple gaps at price

- **Given** gap zones:
  - Gap A: `[86000, 86500]` trust 0.9
  - Gap B: `[86200, 86800]` trust 0.7
  - Gap C: `[87000, 87500]` trust 0.8
- **When** overlap is computed at price `86300`
- **Then** gaps A and B overlap at 86300 → count = 2, weighted = 0.9 + 0.7 = 1.6
- **And** gap C does not overlap

### Scenario 8: Overlap density — no gaps at price

- **Given** gap zones: `[86000, 86500]`, `[87000, 87500]`
- **When** overlap is computed at price `86700` (in the gap-free zone between)
- **Then** overlap count = 0

### Scenario 9: LLM context output

- **Given** gap zones with 3 gaps above current price and 1 below
- **When** gap zone summary is formatted for the LLM
- **Then** output includes: `gap_zones_above: 3, gap_zones_below: 1, nearest_gap: 86200-86500 (trust 0.85)`
- **And** the nearest gap is the one closest to current price (either above or below)

### Scenario 10: Empty history — no gaps

- **Given** fewer than `atr_period` candles in history (not enough to compute ATR)
- **When** gap detection runs
- **Then** no gaps are detected and the gap queue remains empty
- **And** LLM output: `"No gap zones detected yet"`

### Scenario 11: Gap aging

- **Given** a gap zone recorded 30 candles ago
- **When** the gap zone summary is formatted
- **Then** the gap's `age_bars = 30` is included
- **And** the formatted output shows age context (e.g., recent gaps are more prominent than old gaps)

### Scenario 12: LLM tunes gap detection params

- **Given** gap zone config: `{atr_period: 42, max_zones: 50, min_trust: 0.3}`
- **When** the LLM responds with `"gap_zones": {"atr_period": 28, "max_zones": 30, "min_trust": 0.5}`
- **Then** `atr_period` is validated: 28 is within range `[14, 56]` and within ±30% of 42 → accepted
- **And** `max_zones` is validated: 30 is within `[10, 100]` → accepted
- **And** `min_trust` is validated: 0.5 is within `[0.0, 0.9]` → accepted
- **And** predictions are retained (param tuning does not trigger prediction/weight reset)
- **And** gap zone state is recomputed from the full OHLC series using the new params (stateful indicator recomputation)

### Scenario 13: Gap rejected by min_trust filter

- **Given** `min_trust = 0.5` and an abnormal candle with `open=86500, high=87500, low=85500, close=87200`
- **When** gap detection runs
- **Then** trust = `|87200 - 86500| / (87500 - 85500)` = 0.35 < 0.5
- **And** the gap is NOT recorded (below min_trust threshold)

## Tunable Parameters

Gap zone detection has 3 LLM-tunable params, managed through the `IndicatorConfig` system (see `indicator-architecture-v2.md`):

| Param        | Default | Range      | Rationale                                                                                                                          |
| ------------ | ------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `atr_period` | 42      | [14, 56]   | ATR lookback for "normal" range. Shorter = more sensitive to recent vol (more gaps detected). Longer = catches only extreme moves. |
| `max_zones`  | 50      | [10, 100]  | Queue depth for historical gaps. More zones = longer memory of S/R levels.                                                         |
| `min_trust`  | 0.3     | [0.0, 0.9] | Minimum body/wick ratio to record a gap. Higher = stricter quality filter.                                                         |

All params follow the same guardrails as other indicators: range clamping, no-op guard. `atr_period` and `max_zones` use ±30% rate limiting. `min_trust` is exempt from rate limiting — it's a quality filter threshold, not a computational parameter, so the LLM should be able to adjust it freely within range.

## Validation Rules

- Trust score: `|close - open| / (high - low)`, range `[0, 1]`. If `high == low` (zero-range candle), trust = 0
- Gap zone: `bottom = min(open, close)`, `top = max(open, close)`
- Abnormal detection: candle is abnormal if `close > open + ATR` OR `close < open - ATR` (strict inequality — touching the band edge is NOT abnormal)
- Queue max size: configurable, default 50
- Overlap is computed at a specific price point; a gap overlaps if `bottom ≤ price ≤ top`
- All indicators are price-only — no volume dependency

## Out of Scope

- Integration with telegrambot (covered by indicator-architecture-v2.md)
- Volume-weighted gap zones
- Multi-timeframe gap aggregation (future enhancement)

## Dependencies

- `algotrap::ta::volatility::atr` — ATR computation
- `algotrap::ta::ohlc::Ohlc` — OHLC type

---

## Traceability Matrix

| #   | Scenario                    | Impl Status | Impl Artifact         | Test Status | Test Artifact                               | Notes                                        |
| --- | --------------------------- | ----------- | --------------------- | ----------- | ------------------------------------------- | -------------------------------------------- |
| 1   | Bullish gap detection       | ✓           | `src/ta/gap_zones.rs` | ✓           | `test_bullish_gap`                          |                                              |
| 2   | Bearish gap detection       | ✓           | `src/ta/gap_zones.rs` | ✓           | `test_bearish_gap`                          |                                              |
| 3   | Normal candle no gap        | ✓           | `src/ta/gap_zones.rs` | ✓           | `test_normal_candle_no_gap`                 |                                              |
| 4   | Doji within band            | ✓           | `src/ta/gap_zones.rs` | ⊘           | —                                           | Covered by logic: close must be outside band |
| 5   | Doji outside band low trust | ✓           | `src/ta/gap_zones.rs` | ◐           | `test_min_trust_filter`                     | Trust calc tested via scenario 13            |
| 6   | Sized queue eviction        | ✓           | `src/ta/gap_zones.rs` | ✓           | `test_queue_limit`                          |                                              |
| 7   | Overlap density multi       | ✓           | `src/ta/gap_zones.rs` | ✓           | `test_overlap_density`                      |                                              |
| 8   | Overlap density none        | ✓           | `src/ta/gap_zones.rs` | ✓           | `test_no_overlap`                           |                                              |
| 9   | LLM context output          | ✓           | `llm/tools.rs`        | ✓           | `test_gap_zone_summary`                     |                                              |
| 10  | Empty history               | ✓           | `src/ta/gap_zones.rs` | ✓           | `test_empty_history`                        |                                              |
| 11  | Gap aging                   | ✓           | `src/ta/gap_zones.rs` | ✓           | `test_bullish_gap` (age=0)                  | age_bars embedded in detection               |
| 12  | LLM tunes params            | ✓           | `memory.rs`           | ✓           | `test_indicator_config_rate_limited_tuning` | Via IndicatorConfig system                   |
| 13  | Gap rejected by min_trust   | ✓           | `src/ta/gap_zones.rs` | ✓           | `test_min_trust_filter`                     |                                              |

**Status legend**: ⬚ pending · ◐ partial · ✓ complete · ⊘ N/A

### Gap Summary

- **Scenarios total**: 13
- **Implemented**: 13 / 13
- **Tested**: 12 / 13 (scenario 4 covered by logic, no separate test)
- **Blocking gaps**: None

---

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: self-review
> **Date**: 2026-03-28

### Debate Record

| #   | Vector       | Challenge                                                                                      | Response                                                                                                                                                                                      | Verdict        |
| --- | ------------ | ---------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| 1   | assumptions  | ATR compression after vol squeeze causes many false positives — normal candles look "abnormal" | Two layers of defense: (1) trust score + min_trust filter removes low-quality gaps; (2) `atr_period` is LLM-tunable — if squeeze causes noise, LLM can increase the period to widen the band. | challenger-won |
| 2   | edge cases   | Close exactly at band edge (`close == open + ATR`) — abnormal or not?                          | Strict inequality `>` means band-touch is NOT abnormal. Gap thesis requires breaking through, not touching.                                                                                   | challenger-won |
| 3   | edge cases   | min_trust filter has no scenario — gaps can be detected but never filtered                     | Added Scenario 13: gap rejected when trust < min_trust                                                                                                                                        | challenger-won |
| 4   | alternatives | ±30% rate limit on min_trust float is awkward (0.3 ±30% = [0.21, 0.39])                        | min_trust is a quality filter threshold, not a computational param — exempt from rate limiting. LLM can adjust freely within range.                                                           | challenger-won |

### Challenge Summary

- **Challenges raised**: 4
- **Author victories**: 0
- **Challenger victories**: 4
- **Overall verdict**: ACCEPTED

### Revisions Made

- Clarified strict inequality in validation rules
- Added Scenario 13 (min_trust filter rejects low-trust gaps)
- Exempted min_trust from ±30% rate limiting
- Added tunable params section

## Notes

- This indicator is **stateful** — gap zones accumulate over candle history. Unlike stateless indicators (rssi, atr) where param changes apply naturally on the next cycle, changing gap zone params (e.g., `atr_period`) invalidates all previously accumulated gaps.
- **Design choice: recompute from full OHLC series each cycle.** The bot already fetches complete candle history, so gap zones are recalculated fresh each cycle with current params. This means param changes apply naturally without requiring a separate state invalidation mechanism.
- Gap zone detection lives in `algotrap::ta::gap_zones` as free functions (`is_atr_gap`, `body_ratio`) returning lazy `Expr`, plus pure-Rust structs/functions for post-collect aggregation (`GapZone`, `overlap_density`, `gap_zone_summary`).
- Trust score acts as a quality filter — the LLM should weight high-trust gap clusters more heavily


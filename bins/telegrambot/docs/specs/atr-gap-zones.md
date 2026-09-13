# Feature: ATR Gap Zones Indicator

> **Owner**: InNoobWeTrust
> **Created**: 2026-03-28

## Description

A price-structure indicator for `algotrap` that detects abnormal candles closing outside ATR bands and records their body ranges as "gap zones." The scalar detector qualifies a candidate when `is_atr_gap && body_ratio >= body_ratio_threshold`. Queries return bounded raw recent zones; charts and LLM context derive their own composition from that raw relation.

## User Stories

- As a **trader**, I want raw recent gap-zone relations, so that downstream charts and analysis can derive levels appropriate to the current view.
- As a **bot operator**, I want gap zones as a price-only indicator, so that the LLM has structural S/R context beyond RSI/ATR.

## Scenarios

### Scenario 1: Detect abnormal candle — bullish gap

- **Given** a candle with `open=86000, high=87200, low=85900, close=87100`
- **And** current ATR = 500, `atr_band_multiplier = 1.618`, and `atr_gap_multiplier = 1.0`
- **And** the ATR band uses `atr_band_multiplier` for its width, while the candidate boundary uses `open ± ATR × atr_gap_multiplier`
- **When** gap detection runs
- **Then** the candle is flagged as an ATR-gap candidate because close 87100 exceeds the explicit boundary `open + ATR × atr_gap_multiplier = 86500`
- **And** a gap zone is recorded: `bottom=86000, top=87100` (min/max of open, close)
- **And** the candidate body ratio is evaluated against `body_ratio_threshold`

### Scenario 2: Detect abnormal candle — bearish gap

- **Given** a candle with `open=87000, high=87100, low=85800, close=85900`
- **And** current ATR = 500, `atr_band_multiplier = 1.618`, and `atr_gap_multiplier = 1.0`
- **And** the ATR band uses `atr_band_multiplier` for its width, while the candidate boundary uses `open ± ATR × atr_gap_multiplier`
- **When** gap detection runs
- **Then** the candle is flagged as an ATR-gap candidate because close 85900 is below the explicit boundary `open - ATR × atr_gap_multiplier = 86500`
- **And** a gap zone is recorded: `bottom=85900, top=87000`
- **And** the candidate body ratio is evaluated against `body_ratio_threshold`

### Scenario 3: Normal candle — no gap

- **Given** a candle with `open=87000, high=87200, low=86800, close=87050`
- **And** current ATR = 500, with the explicit candidate boundary `open ± ATR × atr_gap_multiplier = [86500, 87500]`
- **When** gap detection runs
- **Then** the candle is NOT flagged (close 87050 is within band)
- **And** no gap zone is recorded

### Scenario 4: Doji candle — body-ratio qualification

- **Given** a candle with `open=86500, high=87300, low=85700, close=86510`
- **And** current ATR = 400, with the explicit candidate boundary `open ± ATR × atr_gap_multiplier = [86100, 86900]`
- **And** close 86510 is within band
- **When** gap detection runs
- **Then** the candle is NOT flagged (close is within band, even though wicks extend far)

### Scenario 5: Doji that closes outside — threshold qualification

- **Given** a candle with `open=86500, high=87500, low=85500, close=87200`
- **And** current ATR = 500 (ATR band = `[86000, 87000]`)
- **When** gap detection runs with `body_ratio_threshold = 0.618`
- **Then** the candidate is rejected because `body_ratio = 700 / 2000 = 0.35` is below the threshold

### Scenario 6: Recent-zone budget — bounded relation

- **Given** more than 16 qualifying raw zones and `max_zones = 16`
- **When** the recent-zone relation is queried
- **Then** no more than 16 recent raw zones are returned
- **And** charts/LLM context derive their own composition from those zones

### Scenario 7: Raw recent-zone relation — bounded query

- **Given** gap zones:
  - Gap A: `[86000, 86500]`
  - Gap B: `[86200, 86800]`
  - Gap C: `[87000, 87500]`
- **When** the query is bounded by `max_zones = 2`
- **Then** the query returns at most the two most recent raw zones
- **And** downstream charts/LLM context choose their own composition

### Scenario 8: Raw recent-zone relation — no zones

- **Given** gap zones: `[86000, 86500]`, `[87000, 87500]`
- **When** no qualifying recent zones are available
- **Then** the raw-zone query returns an empty relation

### Scenario 9: Two-layer ownership

- **Given** raw recent zones returned by the bounded query
- **When** charts or LLM context consume the relation
- **Then** each consumer derives its own summaries and composition
- **And** the scalar detector and query layer do not apply consumer-specific weighting or nearest/overlap summaries

### Scenario 10: Empty history — no gaps

- **Given** fewer candles than the upstream ATR indicator's configured lookback (not enough to compute ATR)
- **When** gap detection runs
- **Then** no gaps are detected and the gap queue remains empty
- **And** the bounded raw-zone query returns an empty relation

### Scenario 11: Raw-zone age relation

- **Given** a gap zone recorded 30 candles ago
- **When** the raw recent-zone relation is queried
- **Then** the zone's `age_bars = 30` is available to downstream consumers
- **And** charts/LLM context decide how to use that age context

### Scenario 12: LLM proposes gap-zone params

- **Given** gap-zone config: `{max_zones: 16, body_ratio_threshold: 0.618, atr_band_multiplier: 1.618, atr_gap_multiplier: 1.0}`
- **When** the LLM responds with `"gap_zones": {"max_zones": 20, "body_ratio_threshold": 0.7, "atr_band_multiplier": 2.0, "atr_gap_multiplier": 1.2}`
- **Then** `max_zones` is validated: 20 is within `[1, 32]` and within ±30% of 16 → accepted
- **And** `body_ratio_threshold` is validated: 0.7 is within `[0.0, 1.0]` and within ±30% of 0.618 → accepted
- **And** `atr_band_multiplier` is validated: 2.0 is within `[0.5, 5.0]` and within ±30% of 1.618 → accepted
- **And** `atr_gap_multiplier` is validated: 1.2 is within `[0.5, 5.0]` and within ±30% of 1.0 → accepted
- **And** the bounded raw-zone query uses the updated values

### Scenario 13: Gap rejected by body-ratio threshold

- **Given** `body_ratio_threshold = 0.5` and an abnormal candle with `open=86500, high=87500, low=85500, close=87200`
- **When** gap detection runs
- **Then** `body_ratio = |87200 - 86500| / (87500 - 85500) = 0.35 < 0.5`
- **And** the gap is NOT recorded because `0.35 < body_ratio_threshold`

## Tunable Parameters

Gap-zone configuration has exactly 4 LLM-tunable params, managed through the `IndicatorConfig` system (see this spec):

| Param                     | Default | Range      | Rationale                                                                                              |
| ------------------------- | ------- | ---------- | ------------------------------------------------------------------------------------------------------ |
| `max_zones`               | 16      | [1, 32]    | Per-timeframe bound on raw recent zones returned to consumers; it is also the LLM budget.              |
| `body_ratio_threshold`    | 0.618   | [0.0, 1.0] | Candidate qualification threshold applied with `is_atr_gap`; it preserves meaningful candle structure. |
| `atr_band_multiplier`     | 1.618   | [0.5, 5.0] | ATR band width used by band-reversion geometry.                                                         |
| `atr_gap_multiplier`      | 1.0     | [0.5, 5.0] | Explicit ATR-distance boundary used by gap candidate detection.                                         |

The consumer-facing defaults above apply when no proposal overrides them. Pure TA uses explicit multipliers supplied by its caller and owns no multiplier defaults. All four proposal changes share a ±30% rate limit per cycle; `max_zones` remains the per-timeframe recent-zone/LLM budget, `body_ratio_threshold` qualifies scalar candidates, `atr_band_multiplier` controls ATR band width / band-reversion geometry, and `atr_gap_multiplier` controls the explicit ATR-distance candidate boundary.

## Validation Rules

- Body ratio: `|close - open| / (high - low)`, range `[0, 1]`. If `high == low` (zero-range candle), body ratio = 0
- Gap zone: `bottom = min(open, close)`, `top = max(open, close)`
- Abnormal detection: candidate if `close > open + ATR × atr_gap_multiplier` OR `close < open - ATR × atr_gap_multiplier` (strict inequality — touching the explicit boundary is NOT abnormal)
- The scalar detector qualifies only when `is_atr_gap && body_ratio >= body_ratio_threshold`
- Queries return bounded raw recent zones; charts and LLM context own downstream composition
- All indicators are price-only — no volume dependency

## Out of Scope

- Integration with telegrambot (covered by the indicator config and output-activation spec)
- Volume-weighted gap zones
- Multi-timeframe gap aggregation (future enhancement)

## Dependencies

- `algotrap::ta::ops::gap_candidate` — pure ATR-gap candidate detection and scalar qualification using explicit multipliers
- `algotrap::query::gap_zones` — bounded raw recent-zone query and per-timeframe budget

Pure TA owns no multiplier defaults; callers provide the explicit `atr_band_multiplier` and `atr_gap_multiplier` values. The canonical query path is `algotrap::query::gap_zones`.

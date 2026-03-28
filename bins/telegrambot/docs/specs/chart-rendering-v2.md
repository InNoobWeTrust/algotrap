# Feature: Chart Rendering v2 — Compile-Time Safety + Visual Enhancements

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-28
> **Research**: `docs/prds/research/prompt-engineering-v2.md`, `polyglot_lib.pine` (TradingView reference)

## Parent Spec

`docs/specs/indicator-architecture-v2.md` — Compute-all, show-selectively principle

## Description

The chart rendering pipeline has two problems:

1. **Silent breakage**: The chart HTML template references DataFrame column names as raw JS strings. When indicators are added/removed from the Rust pipeline, the template silently breaks — only discovered after deployment (e.g., `climax_signal` removed but still referenced).
2. **Missing visuals**: Key signals visible on the TradingView reference chart (ATR climax, biased candle markers, gap zone levels) are computed by the bot but invisible on the screenshot chart.

This spec addresses both: a column registry with compile-time assertions prevents (1), and porting 5 visual features from the Pine Script reference closes (2).

## User Stories

- As a **bot operator**, I want stale chart template references caught by `cargo test`, so that indicator changes never silently break the chart screenshot.
- As a **trader**, I want ATR climax circles on the chart, so that I can see at a glance where price broke through volatility bands.
- As a **trader**, I want biased candle arrows on the chart, so that I can identify strong directional candles with wick confirmation.
- As a **trader**, I want gap zone price levels on the chart, so that I can correlate computed S/R zones with price action visually.
- As a **trader**, I want RSSI background tinting, so that I can immediately see the market regime (bullish/bearish/neutral).

## Scenarios

### Group A: Compile-Time Safety

#### Scenario 1: Column registry matches data pipeline

- **Given** a `CHART_COLUMNS` const array defined in `chart.rs`
- **And** the `indicators()` function in `data.rs` produces a set of column aliases
- **When** `cargo test` runs
- **Then** a test scans `chart_template.html` for all `d.xxx` references
- **And** asserts every referenced column (excluding OHLCV base: `time`, `open`, `high`, `low`, `close`, `volume`) is present in `CHART_COLUMNS`
- **And** if a referenced column is NOT in `CHART_COLUMNS`, the test fails with a message naming the unknown columns

#### Scenario 2: Stale column reference caught at test time

- **Given** an indicator column `foo_signal` was removed from `data.rs`
- **And** the chart template still references `d.foo_signal`
- **When** `cargo test` runs
- **Then** the test fails: `"Chart template references unknown columns: [\"foo_signal\"]"`
- **And** the developer either removes the reference from the template or adds the column back

#### Scenario 3: New column added without template usage (harmless)

- **Given** a new indicator column `bar_index` is added to `CHART_COLUMNS`
- **And** the chart template does NOT yet reference `d.bar_index`
- **When** `cargo test` runs
- **Then** the test passes (unused columns in the registry are harmless — they exist in the DataFrame but the template just doesn't render them)

#### Scenario 4: `CHART_COLUMNS` consistency with DataFrame at runtime

- **Given** the bot processes market data for a ticker
- **When** `process_data()` produces a DataFrame
- **Then** every column in `CHART_COLUMNS` exists in the resulting DataFrame
- **And** a debug assertion (`debug_assert!`) verifies this in development builds

### Group B: ATR Climax Markers

#### Scenario 5: Buying climax circle rendered

- **Given** a candle with `close >= atr_upperband`
- **When** the chart is rendered
- **Then** a green circle marker (●) is placed `aboveBar` at that candle's time
- **And** the circle color is `rgba(157, 225, 159, 0.7)` (soft green)

#### Scenario 6: Selling climax circle rendered

- **Given** a candle with `close <= atr_lowerband`
- **When** the chart is rendered
- **Then** a red circle marker (●) is placed `belowBar` at that candle's time
- **And** the circle color is `rgba(201, 134, 134, 0.7)` (soft red)

#### Scenario 7: Normal candle — no climax marker

- **Given** a candle with `atr_lowerband < close < atr_upperband`
- **When** the chart is rendered
- **Then** no climax marker is placed at that candle

### Group C: ATR Reversion Arrows

#### Scenario 8: Bullish reversion arrow

- **Given** a candle with `atr_reversion_percent > 50` AND `rssi < 46`
- **When** the chart is rendered
- **Then** a green `arrowUp` marker is placed `belowBar`
- **And** the arrow color is `rgba(150, 225, 150, 0.5)`

#### Scenario 9: Bearish reversion arrow

- **Given** a candle with `atr_reversion_percent < -50` AND `rssi > 54`
- **When** the chart is rendered
- **Then** a red `arrowDown` marker is placed `aboveBar`
- **And** the arrow color is `rgba(220, 80, 80, 0.5)`

#### Scenario 10: No reversion — no arrow

- **Given** a candle with `|atr_reversion_percent| < 50`
- **When** the chart is rendered
- **Then** no reversion arrow is placed

### Group D: Biased Candle Markers

#### Scenario 11: Bullish biased candle (strictly rising)

- **Given** the `biased_candle` column value is `1` (pre-computed in Rust)
  - Which means: `bottom_wick >= 23.6%`, `candle_bias >= 0.5`, prev hlc3 was falling, candle 1.5x stronger
- **When** the chart is rendered
- **Then** a blue `arrowUp` marker is placed `belowBar`
- **And** the marker color is `rgba(33, 150, 243, 0.8)` (distinct from ATR reversion green)

#### Scenario 12: Bearish biased candle (strictly falling)

- **Given** the `biased_candle` column value is `-1` (pre-computed in Rust)
  - Which means: `top_wick >= 23.6%`, `candle_bias < 0.5`, prev hlc3 was rising, candle 1.5x stronger
- **When** the chart is rendered
- **Then** a pink `arrowDown` marker is placed `aboveBar`
- **And** the marker color is `rgba(233, 30, 99, 0.8)` (distinct from ATR reversion red)

#### Scenario 13: Non-biased candle — no marker

- **Given** the `biased_candle` column value is `0`
- **When** the chart is rendered
- **Then** no biased candle marker is placed

### Group E: Gap Zone Price Lines

#### Scenario 14: Active gap zones shown as price lines

- **Given** the DataFrame contains gap zones (from `extract_gap_zones`)
- **And** the `gap_zones` indicator is active (`ic.is_active("gap_zones")`)
- **When** the chart is rendered
- **Then** for each gap zone with `trust >= min_trust`:
  - A horizontal price line is drawn at the zone's `level` (midpoint of top/bottom)
  - The line style is dashed
  - The line color is based on direction: blue-tinted for bullish gaps, orange-tinted for bearish gaps
  - The line opacity scales with trust score (higher trust = more opaque)
- **And** at most 10 price lines are rendered (most recent first, hardcoded cap)

#### Scenario 15: Gap zones inactive — no price lines

- **Given** the `gap_zones` indicator is inactive (`ic.is_active("gap_zones") == false`)
- **When** the chart is rendered
- **Then** no gap zone price lines are drawn
- **Note**: The data is still computed (compute-all principle) but not shown

### Group F: RSSI Background Tint

#### Scenario 16: Bullish regime — green tint

- **Given** the latest candle has `rssi >= 60`
- **When** the chart background is rendered
- **Then** the price pane (pane 0) has a subtle green tint: `rgba(0, 137, 123, 0.05)`

#### Scenario 17: Bearish regime — red tint

- **Given** the latest candle has `rssi <= 40`
- **When** the chart background is rendered
- **Then** the price pane (pane 0) has a subtle red tint: `rgba(136, 14, 79, 0.05)`

#### Scenario 18: Neutral regime — no tint

- **Given** the latest candle has `40 < rssi < 60`
- **When** the chart background is rendered
- **Then** no background tint is applied

## Validation Rules

- `CHART_COLUMNS` must be updated whenever `data.rs:indicators()` changes column aliases
- All marker colors must be semi-transparent (opacity ≤ 0.8) to avoid obscuring price action
- Multiple markers on the same bar are allowed (LightweightCharts supports stacking)
- Markers must be sorted by time before calling `setMarkers()` (LightweightCharts requirement)
- Gap zone price lines capped at 10 (hardcoded) to prevent visual clutter
- RSSI tint is based on the **last candle's** RSSI only (not per-bar), applied as a CSS overlay

## Changes Required

### `src/chart.rs`

- **Add**: `CHART_COLUMNS` const array
- **Add**: Test `chart_template_references_only_known_columns` (regex scan of template)
- **Modify**: `render_single_tf_chart_html` to accept gap zone data and indicator config for conditional rendering

### `src/chart_template.html`

- **Remove**: Dead `climax_signal` marker code (lines 192-198)
- **Add**: ATR climax circle markers (JS, using existing `d.atr_upperband`, `d.atr_lowerband`, `d.close`)
- **Add**: ATR reversion arrow markers (JS, using `d.atr_reversion_percent`, `d.rssi`)
- **Add**: Biased candle arrow markers (JS, reads pre-computed `d.biased_candle` column)
- **Add**: Gap zone price lines (JS, from injected gap zone data via minijinja)
- **Add**: RSSI background tint (CSS overlay, conditional on last candle's RSSI)

### `src/data.rs`

- **Add**: `biased_candle` lazy Polars expression to `indicators()` — produces `i8` column (1 = rising, -1 = falling, 0 = none)
- **Add**: Optional `debug_assert!` for `CHART_COLUMNS` presence in DataFrame

## Traceability Matrix

| # | Scenario | Impl Status | Impl Artifact | Test Status | Test Artifact | Notes |
|---|----------|-------------|---------------|-------------|---------------|-------|
| 1 | Column registry matches pipeline | ⬚ | — | ⬚ | — | |
| 2 | Stale column caught at test | ⬚ | — | ⬚ | — | |
| 3 | Unused column harmless | ⬚ | — | ⬚ | — | |
| 4 | DataFrame consistency | ⬚ | — | ⬚ | — | debug_assert only |
| 5 | Buying climax circle | ⬚ | — | ⬚ | — | |
| 6 | Selling climax circle | ⬚ | — | ⬚ | — | |
| 7 | Normal candle no climax | ⬚ | — | ⬚ | — | |
| 8 | Bullish reversion arrow | ⬚ | — | ⬚ | — | |
| 9 | Bearish reversion arrow | ⬚ | — | ⬚ | — | |
| 10 | No reversion no arrow | ⬚ | — | ⬚ | — | |
| 11 | Bullish biased candle | ⬚ | — | ⬚ | — | |
| 12 | Bearish biased candle | ⬚ | — | ⬚ | — | |
| 13 | Non-biased no marker | ⬚ | — | ⬚ | — | |
| 14 | Gap zone price lines | ⬚ | — | ⬚ | — | |
| 15 | Gap zones inactive | ⬚ | — | ⬚ | — | |
| 16 | Bullish RSSI tint | ⬚ | — | ⬚ | — | |
| 17 | Bearish RSSI tint | ⬚ | — | ⬚ | — | |
| 18 | Neutral no tint | ⬚ | — | ⬚ | — | |

**Status legend**: ⬚ pending · ◐ partial · ✓ complete · ⊘ N/A

### Gap Summary

- **Scenarios total**: 18
- **Implemented**: 0 / 18
- **Tested**: 0 / 18
- **Blocking gaps**: All scenarios pending

---

## Adversarial Review

### Debate Record

_To be filled after review._

### Challenge Summary

_Pending._

## Verification

- `cargo test` catches stale column references (Scenario 1-3)
- Visual verification: deploy to staging, capture chart screenshots across 3 tickers (BTC, ETH, SOL) on 1h timeframe, compare with TradingView polyglot_lib.pine overlay for the same period
- Markers should not overlap excessively on volatile periods (inspect 2024-03 crash candles)
- RSSI tint should be subtle enough to not interfere with reading price action

## Future Work

- **Canvas plugin**: Draw gap zone boxes (rich rendering) instead of price lines
- **Rendering framework migration**: Move to a framework that supports fill, gradients, and boxes natively
- **RLI indicator**: Add `ta::rli` (RSI of ATR) to the lib, render as scaled oscillator with gradient background
- **Data-driven chart generation** (Approach 4): Define chart series as Rust structs, generate JS from config — true compile-time safety for all chart elements

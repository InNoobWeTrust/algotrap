# Feature: Chart Rendering v2 — Compile-Time Safety + Visual Enhancements

> **Owner**: InNoobWeTrust
> **Created**: 2026-03-28
> **Research**: `docs/prds/research/prompt-engineering-v2.md`, `polyglot_lib.pine`

## Parent Spec

`docs/specs/atr-gap-zones.md`

## Description

The chart pipeline renders a single timeframe from the Telegram presentation
frame into a self-contained LightweightCharts page. A column registry keeps
template references aligned with the presentation output contract. The chart
also renders ATR markers, indicator bands, RSSI context, and supplied gap-zone
bands.

## Scenarios

### Group A: Compile-Time Safety

#### Scenario 1: Column registry matches the presentation contract

- **Given** `chart.rs` defines `CHART_COLUMNS`
- **When** `cargo test` scans `chart_template.html`
- **Then** every referenced derived column is present in `CHART_COLUMNS`
- **And** OHLCV base columns (`time`, `open`, `high`, `low`, `close`, `volume`)
  are excluded from the registry check

#### Scenario 2: Stale template references fail the test

- **Given** a derived column is absent from the presentation output contract
- **And** the template still references it as `d.column_name`
- **When** `cargo test` runs
- **Then** the test reports the unknown column

### Group B: Chart Markers and Indicator Context

#### Scenario 3: ATR climax markers

- **Given** a candle closes at or beyond `atr_upperband` or `atr_lowerband`
- **When** the chart is rendered
- **Then** the corresponding semi-transparent circle marker is placed above or
  below the bar

#### Scenario 4: ATR reversion markers

- **Given** `atr_reversion_percent` and `rssi` satisfy the configured bullish or
  bearish reversion condition
- **When** the chart is rendered
- **Then** the corresponding semi-transparent arrow marker is placed on the bar

#### Scenario 5: Marker ordering

- **Given** multiple markers are generated
- **When** markers are passed to LightweightCharts
- **Then** they are sorted by time first

### Group C: Gap Zone Bands

#### Scenario 6: Supplied raw zones render as bands

- **Given** `MarketData.gap_zones` contains the bounded result of
  `algotrap::query::gap_zones::recent_gap_zones`
- **And** each record is strictly prior to the decision candle and is already
  ordered oldest-to-newest
- **When** the chart is rendered
- **Then** `chart::gap_zones_to_chart_json` supplies only `{top,bottom,direction}`
  for each record
- **And** the chart draws a band between `top` and `bottom`
- **And** bullish, bearish, and flat directions use their direction colors
- **And** fill opacity and border opacity are fixed renderer values

#### Scenario 7: Overlapping zones blend without requalification

- **Given** supplied zones overlap in price
- **When** the chart is rendered
- **Then** overlapping fills blend naturally
- **And** the renderer does not recalculate qualification, apply a confidence
  filter, weight opacity, summarize nearest or overlapping zones,
  or impose an additional render cap

#### Scenario 8: No supplied zones

- **Given** the timeframe has no entries in `MarketData.gap_zones`
- **When** the chart is rendered
- **Then** no gap-zone primitive is attached

### Group D: RSSI Background Context

#### Scenario 9: RSSI tint class

- **Given** the latest candle RSSI is at or above 60, at or below 40, or between
  those bounds
- **When** the chart is rendered
- **Then** the container receives the corresponding bullish, bearish, or neutral
  tint class

## Gap-Zone Data Contract

The scalar detector is the pure TA operation
`algotrap::ta::ops::gap_candidate`. It receives the caller-supplied
`body_ratio_threshold`; the detector does not own application configuration.

`algotrap::query::gap_zones::recent_gap_zones` materializes the latest bounded
raw records that qualify before the decision candle. It excludes the decision
candle, returns records in ascending time order, and is bounded by `max_zones`.
Telegrambot carries those records in the parallel
`MarketData.gap_zones` map. The chart adapter
`chart::gap_zones_to_chart_json` projects each record to exactly:

```json
{"top": 105.0, "bottom": 100.0, "direction": "bullish"}
```

The chart consumes this projection as rendering input only. Qualification,
query bounding, and tuning occur before rendering.

## Gap-Zone Tuning

| Field | Default | Allowed range | Proposal rate limit |
|---|---:|---:|---:|
| `max_zones` | 16 | 1–32 | ±30% |
| `body_ratio_threshold` | 0.618 | 0–1 | ±30% |
| `atr_band_multiplier` | 1.618 | 0.5–5 | ±30% |
| `atr_gap_multiplier` | 1 | 0.5–5 | ±30% |

## Validation Rules

- `CHART_COLUMNS` tracks the Telegram presentation output contract.
- Marker colors remain semi-transparent and markers are time-sorted.
- Gap-zone rendering uses only the supplied raw-zone projection.
- Gap-zone fill and border opacity are fixed values in the chart renderer.
- Gap-zone visual overlap is handled by normal canvas blending.
- RSSI tint is based on the latest candle and is applied as a container class.

## Changes Required

| File | Change |
|---|---|
| `bins/telegrambot/src/chart.rs` | Keep the current column registry, render signature, `GapZoneRecord` adapter, and template-reference test aligned. |
| `bins/telegrambot/src/chart_template.html` | Render supplied `{top,bottom,direction}` zones with the existing fixed-opacity band primitive; retain marker and RSSI behavior. |
| `bins/telegrambot/src/data.rs` | Preserve the presentation-frame contract consumed by the chart. |
| `src/ta/ops/gap_candidate.rs` | Provide the pure scalar detector with caller-supplied `body_ratio_threshold`. |
| `src/query/gap_zones.rs` | Provide bounded, raw, ascending-time records strictly prior to the decision candle. |
| `bins/telegrambot/src/presentation.rs` | Carry chart-compatible presentation fields without embedding gap-zone bands in the frame. |
| `bins/telegrambot/src/main.rs`, `bins/telegrambot/src/commands.rs` | Pass the timeframe's pre-budgeted zones and RSSI tint to the chart renderer. |
| `bins/telegrambot/Cargo.toml` | Provide the existing test-only regex dependency used by the column-reference test. |

## API References

- `algotrap::ta::ops::gap_candidate`
- `algotrap::query::gap_zones::recent_gap_zones`
- `algotrap::query::gap_zones::GapZoneRecord`
- `telegrambot::data::MarketData::gap_zones`
- `telegrambot::chart::gap_zones_to_chart_json`
- `telegrambot::chart::render_single_tf_chart_html`

## Verification

- Run the chart template-reference test.
- Verify the gap-zone adapter emits only `top`, `bottom`, and `direction`.
- Verify supplied zones render with direction colors, fixed opacity, and
  natural overlap blending.
- Verify empty zone input attaches no gap-zone primitive.
- Confirm the diff is limited to this specification and its TRD.

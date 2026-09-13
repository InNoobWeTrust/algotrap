# TRD: Chart Rendering v2

> **Owner**: InNoobWeTrust
> **Created**: 2026-03-28
> **Parent**: `docs/specs/chart-rendering-v2.md`

## Architecture Overview

```text
Telegram presentation frame ──> chart.rs column registry ──> chart_template.html
             │                             │                         │
             └──── MarketData.gap_zones ──┴─ gap-zone JSON ──────────┘
```

The chart pipeline is:

```text
ComputedFrame + pre-budgeted zones → JSON → minijinja → HTML + JS → Browserless PNG
```

The presentation frame supplies chart columns. Gap zones are a parallel
timeframe map and are not embedded as derived chart columns.

## Component 1: Column Registry

`bins/telegrambot/src/chart.rs` owns `CHART_COLUMNS`, excluding the base
OHLCV fields (`time`, `open`, `high`, `low`, `close`, `volume`). The test
`chart_template_references_only_known_columns` scans property references in
the template and fails when a derived reference is not registered.

The chart renderer accepts the current inputs:

```rust
pub fn render_single_tf_chart_html(
    tf: &Timeframe,
    df: &dyn ComputedFrame,
    ticker: &TickerConf,
    gap_zones_json: &str,
    rssi_tint: &str,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>>
```

The renderer receives serialized zones and a precomputed RSSI tint class. It
does not receive an indicator activation context for gap-zone visibility.

## Component 2: Markers and Indicator Series

`chart_template.html` renders the supplied presentation columns as
LightweightCharts series. ATR climax circles and ATR reversion arrows share a
marker series and are sorted by timestamp before being set. Marker colors are
semi-transparent so they do not obscure price action.

## Component 3: Gap-Zone Data Flow

### Scalar detection

The pure detector is `algotrap::ta::ops::gap_candidate`. Its input includes a
caller-supplied `body_ratio_threshold`; it applies scalar qualification and
returns candidate body bounds and direction facts.

### Query boundary

`algotrap::query::gap_zones::recent_gap_zones` receives a completed output
frame, a decision timestamp, and `max_zones`. It returns a bounded relation of
raw `GapZoneRecord` values:

- only qualifying records with `time < decision_time_ms` are included;
- the latest `max_zones` qualifying records are retained;
- the returned vector is sorted in ascending time order;
- the records retain raw candle/body bounds and direction.

Telegrambot stores each timeframe's records in
`MarketData.gap_zones: HashMap<Timeframe, Vec<GapZoneRecord>>`.

### Chart projection

`chart::gap_zones_to_chart_json` maps `GapZoneRecord` to the minimal chart
shape. The output contains exactly three properties per zone:

```json
{"top": 105.0, "bottom": 100.0, "direction": "bullish"}
```

The adapter performs no qualification, filtering, weighting, summarization,
or truncation. The query has already applied the configured `max_zones` bound.

### Rendering primitive

The template parses the injected JSON and attaches `GapZonePrimitive` to the
candlestick series when the array is non-empty. The primitive:

1. maps `top` and `bottom` to y-coordinates;
2. paints a direction-colored rectangle across the visible time range;
3. draws dashed top and bottom borders using fixed opacity values; and
4. lets overlapping rectangles blend through normal canvas compositing.

No additional render-side qualification, confidence weighting, minimum filter,
nearest-zone summary, overlap summary, or hardcoded render limit is applied.

## Component 4: Gap-Zone Tuning Contract

The application-owned tuning fields are:

| Field | Default | Range | Rate limit |
|---|---:|---:|---:|
| `max_zones` | 16 | 1–32 | ±30% |
| `body_ratio_threshold` | 0.618 | 0–1 | ±30% |
| `atr_band_multiplier` | 1.618 | 0.5–5 | ±30% |
| `atr_gap_multiplier` | 1 | 0.5–5 | ±30% |

`max_zones` controls the bounded query result. The other fields configure the
upstream calculations used by the scalar detector and query input frame. None
of these fields is reinterpreted by the chart renderer.

## Component 5: RSSI Tint

`rssi_tint_class` maps the latest RSSI value to `bullish`, `bearish`, or
`neutral`. The template applies the class to the chart container. This is a
single latest-candle CSS tint, not a per-zone or per-bar gap-zone calculation.

## File and Change Table

| File | Current responsibility |
|---|---|
| `bins/telegrambot/src/chart.rs` | Chart renderer, column registry, RSSI class, and `GapZoneRecord` JSON projection. |
| `bins/telegrambot/src/chart_template.html` | LightweightCharts series, markers, RSSI container tint, and fixed-opacity gap-zone primitive. |
| `bins/telegrambot/src/data.rs` | `MarketData` and the parallel per-timeframe `gap_zones` map. |
| `bins/telegrambot/src/presentation.rs` | Telegram chart/presentation output contract. |
| `src/ta/ops/gap_candidate.rs` | Pure scalar gap-candidate detector. |
| `src/query/gap_zones.rs` | Bounded raw-zone query and `GapZoneRecord` type. |
| `bins/telegrambot/src/main.rs` | Supplies timeframe zones and tint during chart generation. |
| `bins/telegrambot/src/commands.rs` | Supplies timeframe zones and tint for command-driven charts. |
| `bins/telegrambot/Cargo.toml` | Test-only dependency for the column scanner. |

## Dependencies and API References

- Rust types: `algotrap::query::gap_zones::GapZoneRecord` and
  `algotrap::query::gap_zones::GapZoneDirection`.
- Query API: `algotrap::query::gap_zones::recent_gap_zones`.
- TA API: `algotrap::ta::ops::gap_candidate`.
- Telegrambot state: `telegrambot::data::MarketData::gap_zones`.
- Chart API: `telegrambot::chart::gap_zones_to_chart_json` and
  `telegrambot::chart::render_single_tf_chart_html`.
- Test dependency: `regex` remains test-only for the template column scanner.

## Performance and Correctness Constraints

- Zone selection is bounded before chart serialization.
- Chart projection is linear in the supplied zone vector.
- Rendering uses one canvas primitive and fixed-opacity fills/borders.
- No chart-side recomputation of gap qualification is permitted.
- The chart JSON contract is exactly `{top,bottom,direction}`.

## Verification

### Automated

- Run the chart template-reference test.
- Test the gap-zone adapter with bullish, bearish, and flat records.
- Assert the adapter emits exactly three keys per record and preserves order.

### Visual

- Render charts with no zones, one zone, multiple directions, and overlapping
  zones.
- Confirm fixed fill/border opacity and direction colors.
- Confirm the RSSI container tint and marker ordering remain unchanged.

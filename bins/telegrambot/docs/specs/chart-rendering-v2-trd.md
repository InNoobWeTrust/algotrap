# TRD: Chart Rendering v2

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-28
> **Parent**: `docs/specs/chart-rendering-v2.md`

## Architecture Overview

```
data.rs:indicators()     chart.rs:CHART_COLUMNS      chart_template.html
  (produces columns) ───── (registry) ─────────────── (consumes columns)
                              │
                        cargo test asserts
                        bidirectional sync
```

The chart pipeline is: `DataFrame → JSON → minijinja → HTML+JS → Browserless → PNG`.

Column names flow from Rust (`data.rs`) through JSON serialization into the JS template. The column registry acts as a contract between the two sides, enforced by tests.

## Component 1: Column Registry + Test

### `CHART_COLUMNS` const

```rust
// src/chart.rs

/// Canonical list of derived indicator columns available to the chart template.
///
/// OHLCV base columns (time, open, high, low, close, volume) are implicit.
/// Update this list whenever `data.rs:indicators()` changes column aliases.
pub const CHART_COLUMNS: &[&str] = &[
    "volume_sma",
    "bias_reversion",
    "ema200",
    "neutral_revrsi",
    "bullish_revrsi",
    "bearish_revrsi",
    "atr_upperband",
    "atr_lowerband",
    "atr_percent",
    "structure_power",
    "structure_power_sma",
    "rssi",
    "rssi_ma",
    "atr_reversion_percent",
    "leverage",
    "sharpe",
    "is_atr_gap",
    "body_ratio",
    "biased_candle",
];

/// OHLCV base columns — always present, not in the registry.
const BASE_COLUMNS: &[&str] = &["time", "open", "high", "low", "close", "volume"];
```

### Template scanner test

```rust
#[cfg(test)]
mod chart_tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn chart_template_references_only_known_columns() {
        let template = include_str!("chart_template.html");

        // Match `d.xxx` and `d["xxx"]` patterns in JS
        let re = regex::Regex::new(r#"d\.([a-z_][a-z0-9_]*)|d\["([a-z_][a-z0-9_]*)"\]"#).unwrap();
        let referenced: HashSet<&str> = re
            .captures_iter(template)
            .filter_map(|c| c.get(1).or(c.get(2)).map(|m| m.as_str()))
            .filter(|k| !BASE_COLUMNS.contains(k))
            .collect();

        let known: HashSet<&str> = CHART_COLUMNS.iter().copied().collect();

        let unknown: Vec<&&str> = referenced.difference(&known).collect();
        assert!(
            unknown.is_empty(),
            "Chart template references unknown columns: {:?}\n\
             Either add them to CHART_COLUMNS or remove from the template.",
            unknown
        );
    }
}
```

### DataFrame debug assertion

```rust
// In data.rs or chart.rs, called after process_data()
#[cfg(debug_assertions)]
fn assert_chart_columns_present(df: &DataFrame) {
    for col_name in crate::chart::CHART_COLUMNS {
        debug_assert!(
            df.column(col_name).is_ok(),
            "CHART_COLUMNS lists '{}' but DataFrame is missing it. \
             Update CHART_COLUMNS or fix indicators().",
            col_name
        );
    }
}
```

## Component 2: Markers — ATR Climax, Reversion, Biased Candles

All three marker types use `createSeriesMarkers()`. They share one marker array, sorted by time, set once.

### Marker generation (JS in template)

```javascript
// ─── Markers ───────────────────────────────────────────────────────
const markers = [];

// ATR Climax circles
data.forEach(d => {
    if (d.close >= d.atr_upperband) {
        markers.push({
            time: d.time,
            position: 'aboveBar',
            shape: 'circle',
            color: 'rgba(157,225,159,0.7)',
        });
    }
    if (d.close <= d.atr_lowerband) {
        markers.push({
            time: d.time,
            position: 'belowBar',
            shape: 'circle',
            color: 'rgba(201,134,134,0.7)',
        });
    }
});

// ATR Reversion arrows
data.forEach(d => {
    if (d.atr_reversion_percent > 50 && d.rssi < 46) {
        markers.push({
            time: d.time,
            position: 'belowBar',
            shape: 'arrowUp',
            color: 'rgba(150,225,150,0.5)',
        });
    }
    if (d.atr_reversion_percent < -50 && d.rssi > 54) {
        markers.push({
            time: d.time,
            position: 'aboveBar',
            shape: 'arrowDown',
            color: 'rgba(220,80,80,0.5)',
        });
    }
});

// Biased candle arrows (pre-computed in Rust as `biased_candle` column)
// Values: 1 = strictly rising, -1 = strictly falling, 0 = none
data.forEach(d => {
    if (d.biased_candle > 0) {
        markers.push({
            time: d.time,
            position: 'belowBar',
            shape: 'arrowUp',
            color: 'rgba(33,150,243,0.8)',  // Blue (distinct from reversion green)
        });
    }
    if (d.biased_candle < 0) {
        markers.push({
            time: d.time,
            position: 'aboveBar',
            shape: 'arrowDown',
            color: 'rgba(233,30,99,0.8)',   // Pink (distinct from reversion red)
        });
    }
});

// Sort by time (LightweightCharts requirement) and set
markers.sort((a, b) => a.time - b.time);
markersSeries.setMarkers(markers);
```

### Marker color palette (collision-free)

| Marker Type | Color | Shape | Position |
|------------|-------|-------|----------|
| ATR climax (buy) | `rgba(157,225,159,0.7)` soft green | circle | aboveBar |
| ATR climax (sell) | `rgba(201,134,134,0.7)` soft red | circle | belowBar |
| ATR reversion (bull) | `rgba(150,225,150,0.5)` pale green | arrowUp | belowBar |
| ATR reversion (bear) | `rgba(220,80,80,0.5)` pale red | arrowDown | aboveBar |
| Biased candle (bull) | `rgba(33,150,243,0.8)` blue | arrowUp | belowBar |
| Biased candle (bear) | `rgba(233,30,99,0.8)` pink | arrowDown | aboveBar |

Each type uses a distinct hue so they're visually separable when stacking on the same bar.

## Component 3: Gap Zone Price Lines

Gap zone data needs to be injected from Rust into the template via minijinja.

### Rust side

```rust
// src/chart.rs — updated signature
pub fn render_single_tf_chart_html(
    tf: &Timeframe,
    df: &DataFrame,
    ticker: &TickerConf,
    gap_zones_json: &str,  // serialized Vec<GapZoneLevel> or "[]"
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    // ...
    Ok(render!(
        TDV_HTML_TEMPLATE,
        dataset => dataset,
        symbol => format!("BingX:{}", ticker.symbol),
        tf => tf.to_string(),
        sl_percent => format!("{:.0}", ticker.sl_percent * 100.),
        tol_percent => format!("{:.2}", ticker.tol_percent * 100.),
        gap_zones => gap_zones_json,
    ))
}

/// Minimal gap zone for chart rendering.
#[derive(Serialize)]
pub struct GapZoneLevel {
    pub level: f64,        // midpoint of zone
    pub direction: String, // "bullish" or "bearish"
    pub trust: f64,        // 0.0-1.0
}
```

### JS side (in template)

```javascript
// Gap zone price lines (injected as JSON array via minijinja)
const gapZones = JSON.parse('{{ gap_zones }}');
gapZones.forEach(zone => {
    const opacity = Math.min(0.8, 0.2 + zone.trust * 0.6);
    const color = zone.direction === 'bullish'
        ? `rgba(33,150,243,${opacity})`    // blue-tinted
        : `rgba(255,152,0,${opacity})`;    // orange-tinted
    candlestickSeries.createPriceLine({
        price: zone.level,
        color: color,
        lineWidth: 1,
        lineStyle: LightweightCharts.LineStyle.Dashed,
        axisLabelVisible: false,
    });
});
```

### Gap zone capping

The number of gap zones rendered is capped at `max_zones` from `IndicatorConfig` (default 50). But 50 price lines would be visually noisy. The chart caps at **10 most recent** zones above `min_trust`. This is a render-side cap, not a data-side cap — the LLM tool still sees all zones.

## Component 4: RSSI Background Tint

The simplest implementation: CSS overlay on the container div, controlled by a template variable.

### Rust side

```rust
// In render_single_tf_chart_html, compute rssi-based class:
let rssi_tint = match last_rssi {
    r if r >= 60.0 => "bullish",
    r if r <= 40.0 => "bearish",
    _ => "neutral",
};
```

### CSS + template

```html
<style>
    #container.tint-bullish { background: rgba(0,137,123,0.05); }
    #container.tint-bearish { background: rgba(136,14,79,0.05); }
</style>
<div id="container" class="tint-{{ rssi_tint }}" data-symbol="{{ symbol }}">
</div>
```

This is a static tint based on the **last candle**. Per-bar dynamic tinting would require an `AreaSeries` hack that's not worth the complexity for a static screenshot.

## Component 5: Biased Candle — Rust Expression

Pre-compute biased candle detection as a lazy previous dataframe implementation expression in `data.rs`, producing an `i8` column.

### Logic (from Pine `polyglot_lib.pine` L530-549)

A candle is "strictly rising" when ALL of:
1. `bottom_wick >= 23.6%` of candle height (strong lower rejection)
2. `(hlc3 - low) / (high - low) >= 0.5` (body biased upward)
3. Previous `hlc3` was falling (`hlc3[1] < hlc3[2]`) — reversal context
4. `(high - low) / (high[1] - low[1]) >= 1.5` — current candle is 1.5x stronger

"Strictly falling" is the mirror: `top_wick >= 23.6%`, bias < 0.5, prev hlc3 rising, stronger.

### Rust expression

```rust
// In data.rs:indicators()

// Biased candle detection (Pine polyglot_lib L530-549 equivalent)
let spread = col("high") - col("low");
let body_top = max_horizontal([col("open"), col("close")]).unwrap();
let body_bot = min_horizontal([col("open"), col("close")]).unwrap();
let top_wick = (col("high") - body_top) / spread.clone();
let bottom_wick = (body_bot - col("low")) / spread.clone();
let hlc3 = (col("high") + col("low") + col("close")) / lit(3.0);
let candle_bias = (hlc3.clone() - col("low")) / spread.clone();
let prev_hlc3_falling = hlc3.clone().shift(1).gt(hlc3.clone().shift(2)); // was falling
let prev_hlc3_rising = hlc3.clone().shift(1).lt(hlc3.clone().shift(2));  // was rising
let stronger = spread.clone().gt(spread.clone().shift(1) * lit(1.5));

let strictly_rising = bottom_wick.clone().gte(lit(0.236))
    .and(candle_bias.clone().gte(lit(0.5)))
    .and(prev_hlc3_falling)
    .and(stronger.clone());

let strictly_falling = top_wick.clone().gte(lit(0.236))
    .and(candle_bias.lt(lit(0.5)))
    .and(prev_hlc3_rising)
    .and(stronger);

let biased_candle = when(strictly_rising)
    .then(lit(1i8))
    .otherwise(
        when(strictly_falling)
            .then(lit(-1i8))
            .otherwise(lit(0i8))
    )
    .alias("biased_candle");
```

### Column semantics

| Value | Meaning |
|-------|---------|
| `1` | Strictly rising — bullish reversal candle |
| `-1` | Strictly falling — bearish reversal candle |
| `0` | Not biased (default) |

### Test

```rust
#[test]
fn test_biased_candle_strictly_rising() {
    // Candle with big bottom wick, upward bias, stronger than previous,
    // and previous hlc3 was falling
    let df = df! {
        "open"   => [100.0, 100.0, 95.0],   // 3rd candle: open low
        "high"   => [102.0, 101.0, 105.0],   // big range
        "low"    => [ 99.0, 100.0,  90.0],   // deep wick down
        "close"  => [101.0,  99.0, 104.0],   // close high (biased up)
    }.unwrap();
    // hlc3: [100.67, 100.0, 99.67] — falling going into bar 3
    // spread[2]=15, spread[1]=1 → 15/1 = 15 >> 1.5 ✓
    // bottom_wick = (95-90)/15 = 0.33 >= 0.236 ✓
    // candle_bias = (99.67-90)/15 = 0.64 >= 0.5 ✓
    // Result: biased_candle[2] == 1
}
```

## Diff Summary

### Files modified

| File | Change Type | Description |
|------|-------------|-------------|
| `src/chart.rs` | Modify | Add `CHART_COLUMNS`, `GapZoneLevel` struct, update `render_single_tf_chart_html` signature, add test |
| `src/chart_template.html` | Modify | Remove `climax_signal` markers, add 3 marker types + gap zone price lines + RSSI tint |
| `src/data.rs` | Modify | Add `biased_candle` expression to `indicators()`, add `debug_assert!` for column presence |
| `src/main.rs` | Modify | Pass gap zone JSON and RSSI tint to `render_single_tf_chart_html` |
| `src/commands.rs` | Modify | Pass gap zone JSON and RSSI tint to `render_single_tf_chart_html` |
| `src/llm/tools.rs` | Modify | Pass gap zone JSON and RSSI tint to `render_single_tf_chart_html` |

### Files NOT modified

| File | Reason |
|------|--------|
| `src/ta/*.rs` | All required computations already exist (biased_candle uses existing OHLC columns, no new ta function needed) |
| `config/prompts/*` | No prompt changes |
| `src/memory.rs` | No memory changes |

## Dependencies

- `regex` — **must be added** to `[dev-dependencies]` in `bins/telegrambot/Cargo.toml` (test-only usage for column registry scanner)
- No new runtime crates needed

## Performance Considerations

- Column registry test: ~0ms (regex on 220-line file)
- Marker generation: O(n) per data point, n ≈ 500 candles typical — negligible
- Gap zone price lines: ≤10 `createPriceLine()` calls — negligible
- RSSI tint: 1 CSS class — negligible

## Resolved Questions

1. ~~**Biased candle computation: JS or Rust?**~~ **Decision: Rust.** Pre-compute as `biased_candle: i8` column in `data.rs` (values: `1` = strictly rising, `-1` = strictly falling, `0` = none). This gives compile-time safety via the column registry, unit-testable logic in Rust, and trivial JS consumption (`d.biased_candle > 0`). See Component 5 below for the Rust expression design.
2. ~~**Gap zone price line cap**~~ **Decision: hardcoded at 10.** Chart render engine will be refactored later; no need to wire configurability into `IndicatorConfig` now.

## Open Questions

None — all resolved.

## Verification

### Automated

- `cargo test` — column registry test catches stale references
- `cargo test` — existing tests still pass (no pipeline changes)

### Visual

- Deploy to staging
- Capture charts for BTC/ETH/SOL on 1h timeframe
- Verify:
  - ATR climax circles appear at correct candles (compare with TradingView)
  - Reversion arrows appear only when conditions met
  - Gap zone price lines render at correct levels
  - RSSI tint matches regime
  - No marker clutter on high-volatility periods
  - Chart screenshot file size stays under 500KB (new elements don't bloat PNG)

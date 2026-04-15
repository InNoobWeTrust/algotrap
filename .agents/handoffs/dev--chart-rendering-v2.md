---
branch: dev
topic: chart rendering v2 — compile-time safety + visual parity
status: implementation-complete
updated: 2026-03-29T03:42:00+07:00
agent: antigravity
---

# Chart Rendering v2 — Compile-Time Safety + Visual Parity

## Status

**Implementation complete.** Chart rendering pipeline hardened with column registry safety test, 5 visual indicators ported from TradingView `polyglot_lib.pine`, and gap zone box primitive for TradingView-style horizontal bands.

103 tests pass (19 lib + 84 telegrambot).

## What was done

### 1. Compile-time safety (column registry)

Problem: Chart template is static HTML loaded via `include_str!`. If a Rust column is renamed/removed, JS silently reads `undefined` — no error until deployed.

Solution: `CHART_COLUMNS` const + `#[test] chart_template_references_only_known_columns` regex-scans the template for all `d.xxx` patterns and asserts each exists in the registry. Catches column drift at `cargo test` time.

### 2. Visual indicators ported

| Indicator | Marker Type | Source |
|-----------|------------|--------|
| ATR Climax | Filled circle (size 3), green ↑ / red ↓ | polyglot_lib.pine L350 |
| ATR Reversion | Arrow, green ↑ / red ↓ | polyglot_lib.pine L408 |
| Biased Candle | Arrow, blue ↑ / pink ↓ | polyglot_lib.pine L530 |
| Gap Zones | Box primitive (filled bands) | polyglot_lib.pine gap zones |
| RSSI Tint | CSS class overlay on container | polyglot_lib.pine RSI bg |

### 3. Data pipeline changes

- `biased_candle` column computed in Rust (`data.rs:indicators()`) as `i32`: `1` rising, `-1` falling, `0` none
- `gap_zones_to_chart_json()` produces `{top, bottom, direction, trust}` for box rendering (direction from `z.bullish`)
- Gap zones extracted at all 3 callsites independently of LLM indicator active/inactive toggle
- `last_rssi_from_df()` + `rssi_tint_class()` helpers in `chart.rs`
- `render_single_tf_chart_html()` signature: new `gap_zones_json` + `rssi_tint` params

### 4. Gap zone box primitive

Inline `ISeriesPrimitive` implementation in the template (no external dependency):
- `GapZonePrimitive` → `GapZoneBandRenderer` → draws `fillRect` + dashed border lines
- Semi-transparent fill, opacity scaled by trust score
- Renders as full-width horizontal bands matching TradingView's box rendering

## Files changed

| File | Changes |
|------|---------|
| `chart.rs` | +120 lines: CHART_COLUMNS, helpers, registry test, rssi_tint tests |
| `chart_template.html` | Replaced climax_signal markers, added 3 marker types + box primitive + RSSI tint |
| `data.rs` | +30 lines: biased_candle lazy Polars expression |
| `Cargo.toml` | +regex dev-dependency |
| `lib.rs` | +allow(ambiguous_glob_imports) for Polars 0.51 |
| `main.rs` | Updated render callsite with gap zone extraction + rssi_tint |
| `commands.rs` | Updated render callsite with gap zone extraction |
| `llm/tools.rs` | Updated render callsite with gap zone extraction |
| `browserless.rs` | Changed viewport from 1920×1080 to 1080×1080 (square for Telegram mobile) |
| `gap_zones.rs` | Added `bullish` field to GapZone struct |
| `telegram.rs` | Fixed pre-existing missing `llm_debug` in test fixture |
| `test_chart_render.rs` | [NEW] Test binary for visual chart verification |

## Key decisions

1. **Column safety via test assertion** (not codegen/macro) — lowest friction, catches 100% of `d.xxx` drift.
2. **Biased candle computed in Rust** (not JS) — ensures compile-time type safety for the logic.
3. **Gap zones as box primitive** (not price lines) — matches TradingView's visual style using `ISeriesPrimitive` canvas API.
4. **Circle markers use `size: 3`** — best approximation of TradingView's hollow rings without a full canvas plugin.
5. **`chart_template.html` force-tracked** (`git add -f`) — `.gitignore:*.html` was excluding it silently.
6. **Gap zones render independently of LLM** — extracted from DataFrame at chart callsites, not gated by `is_active`.
7. **Bullish vs bearish gap colors** — `z.bullish` (close > open) determines direction; cyan for bullish, orange for bearish (matching TradingView's inverse gradient).
8. **Square viewport** — 1080×1080 for optimal Telegram mobile display (same on portrait/landscape).

- **Hollow ring circles**: Requires full canvas `ISeriesPrimitive` for custom circle drawing (deferred to "hack canvas plugin later")

## Verification

- `cargo test --workspace`: 103 passed (19 lib + 84 bot)
- `cargo run --bin test_chart_render`: Visual charts verified in browser with real BingX data
- Gap zone bands, circle markers, biased candle arrows all rendering correctly

## Blockers

None.

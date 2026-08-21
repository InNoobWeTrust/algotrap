# TRD: ATR Gap Zones — Lazy previous dataframe implementation Implementation

> **Status**: draft
> **Parent spec**: `docs/specs/atr-gap-zones.md` (BDD scenarios)
> **Created**: 2026-03-28

## Problem

The current `detect_gap_zones` eagerly iterates raw `&[f64]` slices — manually computing ATR via loops, scanning for gaps, and collecting structs. This violates the algotrap indicator convention where:

- All indicators return `Expr` (lazy previous dataframe implementation expressions)
- All computation runs through previous dataframe implementation' vectorized, SIMD-optimized engine
- Callers apply indicators uniformly via `df.lazy().with_columns(indicators(...))`

Additionally, the current trust score (`body_ratio` alone) only captures single-candle body quality. It ignores whether the gap move has relative strength behind it — a key factor in whether the zone acts as real support/resistance or is just noise.

## Convention (codebase evidence)

| File | Pattern | Example |
|------|---------|---------|
| `ta/volatility.rs` | `fn(&Ohlc, len) -> Expr` | `atr` = `rma(&true_range(ohlc), len)` |
| `ta/rsi.rs` | diff → `when/then` → `.rma()` → formula | `rsi`, `rev_rsi` |
| `ta/metric.rs` | `.rolling_std()` + `.rolling_sum()` | `sharpe` |
| `ta/experimental.rs` | compose from existing primitives | `rssi` = `(open + bar_bias()).rsi(len)` |
| `ta/ma.rs` | single expression transform | `sma`, `rma`, `ema` |

All consumed in `data.rs` via `df.lazy().with_columns(indicators(ticker, ic)).collect()`.

## Design: 2 Lazy Columns + Strength-Weighted Post-Collect

Gap zone detection decomposes into three concerns:
1. **Per-row detection** (lazy) — is this candle a gap? What's its body quality?
2. **Per-gap strength weighting** (post-collect) — compose body quality with existing momentum indicators
3. **Cross-row aggregation** (post-collect) — which gaps overlap at current price?

> **Compute-all, show-selectively**: All indicators are always computed regardless of the LLM's active/inactive toggle. The toggle controls **visibility** (what appears in LLM context), not **computation**. This is critical here: gap zone trust scoring reads `rssi` and `structure_power` columns at post-collect time. If those were skipped when "inactive", gap zones would silently produce degraded trust scores. See `indicator-architecture-v2.md` validation rules.

### Layer 1: Lazy Expressions (algotrap lib)

Two expression-returning functions in `ta/gap_zones.rs`:

| Function | Signature | Description |
|----------|-----------|-------------|
| `is_atr_gap` | `fn(ohlc: &Ohlc, atr_period: usize) -> Expr` | `close > open + atr` OR `close < open - atr` (strict inequality) |
| `body_ratio` | `fn(ohlc: &Ohlc) -> Expr` | `abs(close - open) / (high - low)`, 0 if zero-range candle |

`is_atr_gap` composes from existing `atr(ohlc, period)` in `ta/volatility.rs` — no reimplementation.

`body_ratio` is necessary but not sufficient for trust scoring. It captures single-candle decisiveness (body vs. wick) — a prerequisite gate. A candle with low body_ratio is indecisive regardless of momentum, so it should not form a trusted gap zone. But a high body_ratio alone doesn't mean the gap has real structural weight behind it.

> **Why not `gap_bottom`/`gap_top` expressions?** These are trivially `min(open,close)` / `max(open,close)`, computed from OHLC columns that already exist in the DataFrame. Adding them as lazy columns wastes memory on every row when only ~50 gap rows need them. Compute inline during post-collect extraction.

### Layer 2: Post-Collect Extraction + Strength Weighting (telegrambot)

```rust
fn extract_gap_zones(df: &DataFrame, params: &GapZoneParams) -> Vec<GapZone>
```

Steps:
1. Filter rows: `is_atr_gap == true && body_ratio >= min_trust`
2. For matching rows, read existing columns:
   - OHLC: `bottom = min(open, close)`, `top = max(open, close)`
   - `structure_power`: directional bias momentum (RMA of bar_bias)
   - `rssi`: relative structure strength index (RSI of structural bar power)
3. Compute composite `trust` score (see **Trust Composition** below)
4. Compute `age_bars = df.height() - 1 - row_index`
5. Keep only last `max_zones` entries (most recent)

This is acceptable eager code because:
- It runs on **filtered output** (~10-50 rows from a 500-row DataFrame)
- The expensive ATR computation already ran in previous dataframe implementation' vectorized engine
- The momentum columns (`rssi`, `structure_power`) are already materialized — just reading, not computing
- Overlap/summary are inherently cross-row aggregation — no previous dataframe implementation expression for "count zones containing price X"

#### Trust Composition

`body_ratio` alone answers: "was this candle decisive?" (body vs. wick quality).

The existing indicators answer whether the move had **relative strength** behind it:

| Existing Column | What It Measures | Gap Relevance |
|-----------------|------------------|---------------|
| `structure_power` | Smoothed directional bias (RMA of `bar_bias`) | High abs value = strong momentum backing the gap. Low = the gap occurred during directionless chop. |
| `rssi` | RSI applied to structural bar power (range ~0-100, neutral=50) | Extreme RSSI (>60 or <40) = sustained directional pressure. Near-50 = no relative strength behind the move. |

**Composite formula:**

```
rssi_strength = abs(rssi - 50) / 50              // 0.0 (neutral) → 1.0 (extreme)
trust = body_ratio * (0.5 + 0.5 * rssi_strength) // body_ratio gates, rssi_strength scales
```

Rationale:
- `body_ratio` is the baseline — it ranges [0, 1] and acts as a quality gate via `min_trust`
- `rssi_strength` is a scaling factor that boosts trust when momentum confirms the gap
- The `0.5 + 0.5 * rssi_strength` factor means:
  - RSSI at 50 (neutral): trust = `body_ratio * 0.5` (halved — no momentum confirmation)
  - RSSI at 80 or 20 (extreme): trust = `body_ratio * 0.8`
  - RSSI at 100 or 0 (theoretical max): trust = `body_ratio * 1.0`
- Structure_power sign implicitly captured by RSSI — same underlying `bar_bias` feeds both

> **Why not bake rssi_strength into a lazy expression?** We could, but it would add a composite column computed for every row when only ~50 gap rows need it. Reading `rssi` from an already-materialized column at extraction time is cheaper and keeps the expression layer simple.

> **Why not use `structure_power` directly?** Its scale is price-dependent and unbounded, making it awkward to normalize into a [0,1] trust factor. RSSI normalizes the same underlying signal into a fixed 0-100 range, which maps cleanly to a strength factor.

### Layer 3: Summary (unchanged)

`overlap_density(zones, price)` and `gap_zone_summary(zones, price)` remain as pure Rust on `&[GapZone]`. The `weighted_trust` in overlap density now reflects composite strength, not just body quality.

## ATR Independence

Gap zones has its own tunable `atr_period` (default 42, range [14, 56]). The main pipeline also computes ATR with `ic.period("atr", 42)`.

**If the periods are the same**, previous dataframe implementation' lazy engine _may_ deduplicate the computation. **If they diverge**, two separate ATR columns are computed. This is by design — the gap zone's ATR lookback serves a different purpose (defining "normal" range for gap detection) than the main ATR (used for stop placement, leverage, reversion bands).

In `data.rs`, the gap zone ATR is embedded inside `is_atr_gap(ohlc, gap_atr_period)` — it's an intermediate computation within the expression, not a named output column.

## Data Flow

```
Klines → DataFrame
  → .lazy()
  → .with_columns([
      ..existing indicators (rssi, structure_power, etc.)..
      is_atr_gap(&ohlc, gap_period).alias("is_atr_gap"),
      body_ratio(&ohlc).alias("body_ratio"),
    ])
  → .collect()                         // previous dataframe implementation: vectorized ATR + comparison + ratio
  → extract_gap_zones(&df, &params)    // filter, read rssi column, compute composite trust
  → gap_zone_summary(&zones, price)    // pure aggregation
  → format for LLM
```

## Changes Required

### algotrap lib (`src/ta/gap_zones.rs`)

- **Delete**: `detect_gap_zones` (eager implementation)
- **Add**: `is_atr_gap(ohlc: &Ohlc, atr_period: usize) -> Expr`
- **Add**: `body_ratio(ohlc: &Ohlc) -> Expr`
- **Add**: `OhlcGapZones` trait with `is_atr_gap` and `body_ratio` methods
- **Keep**: `GapZone`, `GapZoneParams`, `OverlapDensity`, `GapZoneSummary` structs
- **Keep**: `overlap_density`, `gap_zone_summary` — pure aggregation
- **Rewrite tests**: Use DataFrame-based tests instead of raw slice tests

### telegrambot (`src/data.rs`)

- **Add**: `is_atr_gap` + `body_ratio` to `indicators()` vec (always included — compute-all principle; `is_active` only controls LLM visibility in tools.rs, not computation)
- **Add**: `extract_gap_zones(df: &DataFrame, params: &GapZoneParams) -> Vec<GapZone>` — filter materialized columns, read `rssi` for composite trust

### telegrambot (`src/llm/tools.rs`)

- **Update**: `compute_gap_zone_context` to call `extract_gap_zones` from materialized DataFrame — no more OHLC extraction into `Vec<f64>`

## Verification

- All 9 existing gap zone test scenarios rewritten for expression API
- All telegrambot tests pass
- `cargo test -p algotrap -p telegrambot` green
- Gap detection ATR runs inside previous dataframe implementation engine (no manual loop over full series)
- Composite trust reflects momentum — gaps at extreme RSSI score higher than gaps at neutral RSSI

## Resolved Questions

1. ~~**Should `body_ratio` live in `ta/common.rs` instead of `ta/gap_zones.rs`?**~~ **Decision**: Keep in `ta/gap_zones.rs`. It's a general candle quality metric, but only gap zones uses it currently. Move to common if a second consumer appears.
2. **Trust formula tuning**: The `0.5 + 0.5 * rssi_strength` weighting is a starting point. The 0.5/0.5 split (baseline vs. momentum scaling) could itself be LLM-tunable via IndicatorConfig. Defer until real-world data shows whether it needs adjustment.

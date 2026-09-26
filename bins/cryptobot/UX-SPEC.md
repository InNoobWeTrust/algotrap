# Unit 01 — Quick-Track UX Specification

## 1. Quick-track rationale

This is a single replacement pane within the established one-page chart. The Quick-track scope adds no navigation, controls, or interaction changes. Existing chart behavior remains the surrounding contract.

## 2. Pane map

| Pane | Content |
|---:|---|
| 0 | Price |
| 1 | Structure Power |
| 2 | ATR Reversion |
| 3 | I-Ching Energy |

Pane 3 packs three semantically distinct layers on the shared energy scale:

- `本卦 I-Ching Original average` — near-opaque cool-blue stepped foreground line
  (`rgba(79, 195, 247, 0.95)`, 2px). Plots `(open+high+low+close)/4`; the intra-bar
  OHLC range is usually tiny so candlesticks collapse to flat ticks.
- `变卦 Transformed projection` — more transparent amber dashed step line
  (`rgba(255, 183, 77, 0.40)`). This is
  the destination produced by the terminal moving-line cast, not another current
  observation.
- `互卦 Mutual inner band + mean` — `互卦` renders two stepped edge lines at
  `iching_mutual_high` and `iching_mutual_low` (visible 1px, 0.25 alpha),
  delineating the full intra-bar range, plus a line-only mean at
  `iching_mutual_mean` for the inner-structure bias. Lightweight-Charts v5
  cannot fill between two dynamic lines: each series uses zero-split
  BaselineSeries fills to zero, so fill overlap is NOT the inner range — the
  inner range reads as the subtraction between the edge lines. Fills stay
  equal and modest as pure sign wash (teal-green `rgba(38, 166, 154, 0.12)`
  above 0, coral-red `rgba(239, 83, 80, 0.12)` below 0, fading to 0.00 at
  zero). Mean uses a slightly lighter same-hue tint
  (`rgba(110, 231, 183, 0.35)` / `rgba(252, 165, 165, 0.35)`, 1px, fills 0.00)
  so it reads on the wash while staying below the Transformed dashed line
  (0.40/2px).

## 3. I-Ching pane presentation contract

Pane 3 uses one compact in-pane watermark identifying the three layer roles. The pane watermark names each channel with its authoritative 卦 character (本, 变, 互) to preserve the source tradition.
The I-Ching values use the natural range `[-31.5,31.5]` and align to candle
time in Unix seconds. The bar interval is right-half-open: a cast at the next
bar's opening boundary belongs only to that next bar.

The RSSI pane and RSSI tint are absent. The Sharpe pane is absent. Reverse RSI price overlays and all unrelated chart behavior remain unchanged.

## 4. Loading, empty, and error behavior

Input JSON continues existing behavior for loading, empty, and error conditions. Malformed I-Ching data must not be fabricated by the UI. No new interactions or states are introduced beyond the existing chart.

Responsive behavior inherits chart autosizing and must preserve label readability at narrow widths.

## 5. Scope boundary

This document is the Quick-track UI contract for the replacement I-Ching pane only. It does not specify implementation source, dependencies, configuration, navigation, controls, or behavior outside the contract above.

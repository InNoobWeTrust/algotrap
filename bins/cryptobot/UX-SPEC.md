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

Pane 3 contains exactly three simultaneous solid line series:

- `I-Ching Original` — `#4FC3F7`
- `I-Ching Transformed` — `#FFB74D`
- `I-Ching Nuclear` — `#CE93D8`

## 3. I-Ching pane presentation contract

Pane 3 uses one in-pane watermark/legend identifying all three exact series labels and their colors. The I-Ching values use the natural range `[-31.5,31.5]` and align to candle time in Unix seconds.

The RSSI pane and RSSI tint are absent. The Sharpe pane is absent. Reverse RSI price overlays and all unrelated chart behavior remain unchanged.

## 4. Loading, empty, and error behavior

Input JSON continues existing behavior for loading, empty, and error conditions. Malformed I-Ching data must not be fabricated by the UI. No new interactions or states are introduced beyond the existing chart.

Responsive behavior inherits chart autosizing and must preserve label readability at narrow widths.

## 5. Scope boundary

This document is the Quick-track UI contract for the replacement I-Ching pane only. It does not specify implementation source, dependencies, configuration, navigation, controls, or behavior outside the contract above.

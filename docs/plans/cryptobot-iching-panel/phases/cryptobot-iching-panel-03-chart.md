# Unit 03 — Embedded Chart Pane Replacement

## Outcome


> **Status: implemented (Sep 2026).** Reflects the final trajectory + mutual-band model shipped in this PR (src/ta/iching/trajectory.rs, bins/cryptobot/src/presentation.rs, bins/chartlib renderer, bins/cryptobot/UX-SPEC.md).
Modify the embedded Lightweight Charts template to remove all RSSI and Sharpe UI/runtime behavior and render the I-Ching Energy pane (two stepped `LineSeries` + three `BaselineSeries`).

## Writable surface

- `[MODIFY]` `bins/cryptobot/src/main.rs` only, specifically `TDV_HTML_TEMPLATE` and any focused template assertions in its existing test module.

## Invariants and contracts

- Consume the trajectory columns `iching_open`, `iching_high`, `iching_low`, `iching_close`, `iching_transformed_close`, `iching_mutual_high`, `iching_mutual_low`, and `iching_mutual_mean` from each candle after the established milliseconds-to-seconds normalization. The three open-cast energy aliases (`iching_original_energy`, `iching_transformed_energy`, `iching_mutual_energy`) are present for backward compatibility but are not directly consumed by the chart series.
- Remove all RSSI artifacts: `rssi-bullish`/`rssi-bearish` CSS and runtime class logic; RSSI series, baseline/direction series, data bindings, and watermark. Do not remove Reverse RSI overlays in price pane `0`.
- Remove all Sharpe artifacts: series, data binding, watermark, and related chart behavior.
- Retain price pane `0`, Structure Power pane `1`, and ATR Reversion pane `2`; add I-Ching Energy as pane `3` containing exactly two stepped `LightweightCharts.LineSeries` and three `LightweightCharts.BaselineSeries` (zero-split, `baseValue` price 0).
- Use the exact series labels and visual semantics from `bins/cryptobot/UX-SPEC.md`:
  - `本卦 I-Ching Original average` — stepped `LineSeries`, `rgba(79,195,247,0.95)`, 2px; value = (`iching_open`+`iching_high`+`iching_low`+`iching_close`)/4.
  - `变卦 Transformed projection` — stepped dashed `LineSeries` (`lineStyle: 2`), `rgba(255,183,77,0.40)`, 2px; value = `iching_transformed_close`.
  - `互卦 Mutual inner band + mean` — three `BaselineSeries`: edge lines `ichingMutualHighSeries` / `ichingMutualLowSeries` (positive teal-green `rgba(38,166,154,0.25)` / negative coral-red `rgba(239,83,80,0.25)`, 1px, fills 0.12→0.00) and line-only mean `ichingMutualMeanSeries` (positive `rgba(110,231,183,0.35)` / negative `rgba(252,165,165,0.35)`, 1px, fills 0.00).
- Add exactly one compact in-pane watermark in pane `3` identifying all three layer roles with their 卦 characters (本/变/互).
- Do not clamp/normalize the I-Ching values; their natural displayed scale is `[-31.5, 31.5]`. Keep all existing unrelated series, watermarks, data mappings, loading behavior, and resize behavior intact except necessary pane-index reindexing.

## Acceptance criteria and required evidence

- Focused template assertions verify all trajectory field names consumed by the chart, the exact labels with 卦 characters, the final color tokens (`rgba(79,195,247,0.95)`, `rgba(255,183,77,0.40)`, teal-green/coral-red BaselineSeries options), one compact in-pane watermark in pane `3`, and exactly two stepped `LineSeries` plus three `BaselineSeries` targeted to pane `3`.
- Focused assertions verify no remaining RSSI/Sharpe pane series/bindings/watermarks/tint class strings, while Reverse RSI series still target price pane `0` and Structure/ATR retain panes `1`/`2`.
- Code inspection of the interval update establishes every I-Ching data point uses its normalized candle `time` and matching energy field.

## Dependencies and stop condition

- Depends on Units 01 and 02.
- Stop if Lightweight Charts cannot render three lines in a single pane with the stated API, if a legend requires a new library/control, or if a required template alteration reaches outside `main.rs`; report the blocker rather than redesigning.

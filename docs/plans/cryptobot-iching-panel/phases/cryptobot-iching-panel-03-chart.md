# Unit 03 — Embedded Chart Pane Replacement

## Outcome

Modify the embedded Lightweight Charts template to remove all RSSI and Sharpe UI/runtime behavior and render one three-line I-Ching Energy pane.

## Writable surface

- `[MODIFY]` `bins/cryptobot/src/main.rs` only, specifically `TDV_HTML_TEMPLATE` and any focused template assertions in its existing test module.

## Invariants and contracts

- Consume exactly `iching_original_energy`, `iching_transformed_energy`, and `iching_nuclear_energy` from each candle after the established milliseconds-to-seconds normalization.
- Remove all RSSI artifacts: `rssi-bullish`/`rssi-bearish` CSS and runtime class logic; RSSI series, baseline/direction series, data bindings, and watermark. Do not remove Reverse RSI overlays in price pane `0`.
- Remove all Sharpe artifacts: series, data binding, watermark, and related chart behavior.
- Retain price pane `0`, Structure Power pane `1`, and ATR Reversion pane `2`; add I-Ching Energy as pane `3` containing exactly three `LightweightCharts.LineSeries` and no additional series in that pane.
- Use labels exactly `I-Ching Original`, `I-Ching Transformed`, `I-Ching Nuclear`, with opaque solid non-gradient colors respectively `#4FC3F7`, `#FFB74D`, `#CE93D8`. Configure/express one watermark or in-template legend that identifies all three labels and their series colors.
- Do not clamp/normalize the I-Ching values; their natural displayed scale is `[-31.5, 31.5]`. Keep all existing unrelated series, watermarks, data mappings, loading behavior, and resize behavior intact except necessary pane-index reindexing.

## Acceptance criteria and required evidence

- Focused template assertions verify all three locked field names, labels, colors, one I-Ching watermark, and exactly three I-Ching `LineSeries` targeted to pane `3`.
- Focused assertions verify no remaining RSSI/Sharpe pane series/bindings/watermarks/tint class strings, while Reverse RSI series still target price pane `0` and Structure/ATR retain panes `1`/`2`.
- Code inspection of the interval update establishes every I-Ching data point uses its normalized candle `time` and matching energy field.

## Dependencies and stop condition

- Depends on Units 01 and 02.
- Stop if Lightweight Charts cannot render three lines in a single pane with the stated API, if a legend requires a new library/control, or if a required template alteration reaches outside `main.rs`; report the blocker rather than redesigning.

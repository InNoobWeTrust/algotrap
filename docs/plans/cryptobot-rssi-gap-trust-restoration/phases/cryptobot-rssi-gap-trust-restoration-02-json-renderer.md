# P02 — Serialize Trust and Render Trust-Weighted Gap Zones

## Unit ID and outcome

**P02.** Consume the app-owned presentation wrapper in Cryptobot’s JSON boundary, publish one numeric `gapZones[*].trust` value per raw zone, and replace the hardcoded gap-band opacity/border strengths with that supplied trust—without restoring RSSI visuals or adding a fallback.

## Writable surface

- `[MODIFY]` `bins/cryptobot/src/main.rs`
  - `compute_crypto_frames` container type only, to match P01.
  - `process_ticker`’s existing zone serialization path.
  - `gap_zone_to_json`.
  - `TDV_HTML_TEMPLATE`, limited to `GapZoneBandRenderer` consumption of the existing zone data.
  - Existing `#[cfg(test)]` module only.

No other file is writable for this unit.

## Implementation contract

1. Replace `Vec<GapZoneRecord>` at the main/presentation boundary with `Vec<presentation::GapZonePresentation>` in the exact `HashMap<Timeframe, (Box<dyn ComputedFrame>, ...)>` type. Do not alter batch order, invalid-sibling isolation, or timeframe behavior.
2. Change `gap_zone_to_json` to accept `&presentation::GapZonePresentation`. It must serialize all currently emitted raw keys at their existing flat JSON locations from `zone.raw`:
   `time_ms`, `open`, `high`, `low`, `close`, `volume`, `body_bottom`, `body_top`, `body_ratio`, and direction text.
3. Add exactly one top-level JSON member, `trust`, from `zone.trust`. It must be numeric and finite. Do not serialize `rssi`, `rssi_ma`, raw RSI input, EMA(9), a nested raw-zone object, SQL/candle fields, or a public chart field.
4. In `GapZoneBandRenderer.draw`, take the supplied `z.trust` as the single styling strength used for both zone fill opacity and border opacity. Remove the hardcoded `0.12`/`0.4` strengths. Do not provide `??`, `||`, neutral constants, clamping, or any fallback if `trust` is missing/invalid; P01 makes invalid/missing association a server-side computation failure instead.
5. Keep the existing direction-dependent RGB colors, raw `body_top`/`body_bottom` geometry, primitive selection/attachment/lifecycle, ordering, chart time behavior, and renderer line geometry intact. Only the alpha strength source changes.
6. Do not add an RSSI series, pane, CSS class, visual tint, watermark, legend, placeholder, or candle mapping. Preserve I-Ching, Reverse RSI, ATR, Structure, and all existing pane indices/series.

## Acceptance criteria and required evidence

- Extend `main.rs` serialization tests with a manually constructed `GapZonePresentation` fixture to prove `gap_zone_to_json` emits the exact pre-existing raw keys plus numeric `trust`, while candle JSON fields do not gain `rssi`, `rssi_ma`, or `trust`.
- Extend the existing batch-adapter test so expected and actual wrapped zones compare including `raw` and `trust`, proving the signature propagation retains timeframe isolation/order.
- Add/extend template assertions that verify the renderer references `z.trust` for both fill and stroke strength and no longer contains the previous hardcoded `const opacity = 0.12` or `const borderOpacity = 0.4` placeholders.
- Retain the current assertion that the full template contains no case-insensitive `rssi` artifact; retain the assertions preserving Reverse RSI, Structure, ATR Reversion, and I-Ching pane behavior.
- Assert JSON source inspection has `trust` only beneath `gapZones` and not as a candle record/public chart field.

## Dependencies and prerequisites

- Requires P01’s `GapZonePresentation` type and changed `compute_crypto_frame` signature.
- P03 depends on P01 and P02.

## Stop conditions

Stop and report if the wrapper cannot be consumed without a source change outside `main.rs`, if the renderer would need a missing-trust fallback, or if the proposed change surfaces RSSI/EMA in a candle/chart/SQL field or restores a visual. Do not redesign the primitive or edit query/TA code.

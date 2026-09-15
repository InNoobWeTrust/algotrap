# P01 — Derive and Associate App-Owned Gap-Zone Trust

## Unit ID and outcome

**P01.** In the Cryptobot presentation layer, recreate raw `RSI(14)` on `kline.open + bar_bias`, derive the locked trust formula, associate each raw gap zone to the trust of its exact source candle, and return app-owned wrapped zones without changing `GapZoneRecord`, query code, TA code, chart fields, or SQL projection.

## Writable surface

- `[MODIFY]` `bins/cryptobot/src/presentation.rs`
  - Imports/constants needed for the existing `rsi` kernel and its state.
  - `CryptoIndicators`, `CryptoIndicatorState`, `CryptoIndicatorRow`, `CryptoIndicators::new`, and `Kernel for CryptoIndicators::transition`.
  - New private helper(s) for formula calculation and raw-zone/time association.
  - New `pub(crate) GapZonePresentation` wrapper.
  - `compute_crypto_frame` return signature and its raw-zone wrapping path.
  - Existing `#[cfg(test)]` helpers and focused tests only.

No other file is writable for this unit.

## Implementation contract

1. Add one internal `rsi(14)` processor and `RsiState` to Cryptobot’s existing aggregate. Feed it **only** the current candle’s `kline.open + bias`, where `bias` is the same `require_output("bar bias", ...)` value already used by neutral Reverse RSI.
2. Do not add the raw RSI output to `CryptoIndicatorRow` as a serializable/public chart field. It may be used locally in `transition` to calculate one private, finite per-row `gap_zone_trust` scalar after `body_ratio` is available.
3. Derive the scalar exactly, with no clamping or EMA:

   ```text
   body_ratio * (0.5 + 0.5 * abs(raw_rssi - 50.0) / 50.0)
   ```

   The intended values are therefore `0.5 * body_ratio` at RSI 50 and `body_ratio` at RSI 0 or 100.
4. Define the locked wrapper exactly:

   ```rust
   pub(crate) struct GapZonePresentation {
       pub(crate) raw: GapZoneRecord,
       pub(crate) trust: f64,
   }
   ```

   It must preserve the raw zone untouched and carry only its separately derived display confidence.
5. After `recent_gap_zones` returns its raw, time-sorted `Vec<GapZoneRecord>`, associate each `raw.time_ms` with the internal row generated from the Kline at the same exact millisecond timestamp. Reject a missing or duplicate match, a non-finite trust, or an unexpected rows/Klines cardinality mismatch with `MarketError::computation`. Do not choose the nearest candle, reuse another candle’s value, use zone `body_ratio` as a fallback, or synthesize a neutral confidence.
6. Change `compute_crypto_frame` exactly to return `Vec<GapZonePresentation>`. Retain projected frame behavior and source-frame columns unchanged: do not add `rssi`, `rssi_ma`, `trust`, or any related field to `crypto_output_frame`, `crypto_select_expressions`, or projected records.
7. Preserve all existing controls: raw candidate facts/geometry/direction/ordering, `recent_gap_zones` invocation, decision-time semantics, I-Ching values, Reverse RSI behavior, and ATR Reversion (`ATR(42) * 1.618`, open-centered, `close` signal).

## Acceptance criteria and required evidence

- Add a focused formula fixture/test using directly supplied scalar inputs (or equivalently controlled fixture output) that proves:
  - `rssi = 50.0` returns `0.5 * body_ratio`;
  - `rssi = 0.0` returns `body_ratio`;
  - `rssi = 100.0` returns `body_ratio`.
- Add a focused multi-candle gap fixture whose selected zones have distinguishable associated RSI/trust values. Assert every returned wrapper keeps the raw-zone time/order/geometry/direction and receives the trust from the Kline with equal `time_ms`, not a neighboring candle. Assert a missing association returns an error; no fallback value is emitted.
- Extend current aggregate/collection assertions to prove RSI input is `open + bar_bias` and raw RSI is used for confidence. The test must distinguish raw RSI behavior from an EMA(9)-smoothed sequence; it must not introduce EMA(9).
- Update return-type consumers in existing `presentation.rs` tests and prove existing raw-zone values still compare through `wrapper.raw` while `trust` is finite.
- Retain or extend current schema tests to prove both source and projected/candle schemas omit `rssi`, `rssi_ma`, `rssi_direction`, `rssi_color`, and `trust`.
- Existing ATR test `atr_reversion_uses_close_signal_outside_open_centered_band` remains unchanged in intent and passes, proving the `ATR(42) * 1.618` computation was not altered.

## Dependencies and prerequisites

- No prerequisite implementation unit. It owns all calculation/typing changes.
- P02 depends on the exact wrapper and return signature produced here.

## Stop conditions

Stop and report rather than expand scope if any requirement forces a change under `src/query/**` or `src/ta/**`, adds an RSI/candle/chart/SQL field, requires EMA(9), changes ATR Reversion, or cannot detect an absent/duplicate time association without a fallback. Escalate the data-model issue to `software-architect` if the locked app-owned wrapper is insufficient.

# Unit 02 — Presentation Transformation, Serialization, and Removal

## Outcome

Replace RSSI and Sharpe production in the Rust presentation pipeline with three I-Ching energy columns, retaining one output row per input Kline and proving the new/error contracts with focused existing-module tests.

## Writable surface

- `[MODIFY]` `bins/cryptobot/src/presentation.rs` only, including its existing `#[cfg(test)]` module.

## Invariants and contracts

- Import and call only the existing facade `algotrap::ta::plum_blossom_signal_with_policy` with `LeapMonthPolicy::Allow` for every candle. `src/ta/iching/**` is read-only and untouched.
- Convert each `Kline.time` millisecond value into `DateTime<Utc>` before facade invocation. An invalid/out-of-range conversion returns the presentation transformation error; never use a substitute date/time.
- For every successfully transformed input candle, serialize nullable numeric source and chart projection fields named exactly `iching_original_energy`, `iching_transformed_energy`, and `iching_nuclear_energy`. Populate Original and Nuclear from their signal channels; populate Transformed from Plum Blossom's transformed channel energy.
- Propagate any facade error. A genuine timestamp/facade failure must not become zero, skipped output, or a silent null.
- Delete the RSSI calculation/state/row fields/source-frame columns/SQL selection, color, direction, and any dependent presentation-only signal expressions. Delete the Sharpe calculation/state/row fields/source-frame columns/SQL selection and color. Remove stale constants/imports and revise all schema/cardinality/parity expectations accordingly.
- Preserve candles, ATR, Structure Power, Reverse RSI inputs/outputs, gap behavior, source ordering, and all unrelated projection contracts.

## Acceptance criteria and required evidence

- Existing presentation tests are updated to assert the exact source and projected schemas: the three I-Ching fields are present; `rssi`, `rssi_ma`, `rssi_direction`, `rssi_color`, `sharpe`, and `sharpe_color` are absent.
- Focused tests prove output length/order remains equal to input Klines; each valid fixture row has all three expected finite energy values in `[-31.5, 31.5]`; and the values equal direct calls to the locked facade using the corresponding millisecond timestamp and `Allow`.
- Focused tests prove an invalid/out-of-range Kline timestamp fails presentation transformation and proves a facade error is propagated rather than serialized as a replacement value.
- Existing invalid-Kline/error tests and updated projection parity tests continue to pass.

## Dependencies and stop condition

- Depends on Unit 01 only for the consumer contract; Unit 03 depends on its projected field names.
- Stop if the facade cannot supply all three channels under `Allow`, if timestamp conversion needs an unplanned dependency/API change, or if retaining any RSSI/Sharpe field is required for unrelated behavior; report rather than expand scope.

# Unit 02 — Presentation Transformation, Serialization, and Removal

## Outcome


> **Status: implemented (Sep 2026).** Reflects the final trajectory + mutual-band model shipped in this PR (src/ta/iching/trajectory.rs, bins/cryptobot/src/presentation.rs, bins/chartlib renderer, bins/cryptobot/UX-SPEC.md).
Replace RSSI and Sharpe production in the Rust presentation pipeline with three I-Ching energy columns, retaining one output row per input Kline and proving the new/error contracts with focused existing-module tests.

## Writable surface

- `[MODIFY]` `bins/cryptobot/src/presentation.rs` only, including its existing `#[cfg(test)]` module.

## Invariants and contracts

- Call `iching_bar_trajectory(kline.time, bar_close_time_ms)` for every candle to obtain the full `IchingBarTrajectory`. `plum_blossom_signal_with_policy` with `LeapMonthPolicy::Allow` is also called per candle for the `CryptoIndicatorRow` open-cast energy fields. `src/ta/iching/**` is read-only and untouched.
- Convert each `Kline.time` millisecond value into `DateTime<Utc>` before facade invocation. An invalid/out-of-range conversion returns the presentation transformation error; never use a substitute date/time.
- For every input candle, serialize all 13 nullable numeric trajectory columns from `IchingBarTrajectory`: open-cast energy aliases `iching_original_energy` (= `energy_open`), `iching_transformed_energy` (= `transformed_open`), `iching_mutual_energy` (= `mutual_open`); Original envelope `iching_open`, `iching_high`, `iching_low`, `iching_close`; terminal cast `iching_moving_line` (nullable u8), `iching_transformed_close`, `iching_mutual_close`; Mutual band `iching_mutual_high`, `iching_mutual_low`, `iching_mutual_mean`.
- Propagate any facade error. A genuine timestamp/facade failure must not become zero, skipped output, or a silent null.
- Delete the RSSI calculation/state/row fields/source-frame columns/SQL selection, color, direction, and any dependent presentation-only signal expressions. Delete the Sharpe calculation/state/row fields/source-frame columns/SQL selection and color. Remove stale constants/imports and revise all schema/cardinality/parity expectations accordingly.
- Preserve candles, ATR, Structure Power, Reverse RSI inputs/outputs, gap behavior, source ordering, and all unrelated projection contracts.

## Acceptance criteria and required evidence

- Existing presentation tests assert the exact source and projected schemas: all 13 I-Ching trajectory columns are present (`iching_original_energy`, `iching_transformed_energy`, `iching_mutual_energy`, `iching_open`, `iching_high`, `iching_low`, `iching_close`, `iching_moving_line`, `iching_transformed_close`, `iching_mutual_close`, `iching_mutual_high`, `iching_mutual_low`, `iching_mutual_mean`); `rssi`, `rssi_ma`, `rssi_direction`, `rssi_color`, `sharpe`, and `sharpe_color` are absent.
- Focused tests prove output length/order remains equal to input Klines; each valid fixture row carries all trajectory values within the natural domain where applicable; and values equal direct calls to `iching_bar_trajectory` using the corresponding millisecond timestamps.
- Focused tests prove an invalid/out-of-range Kline timestamp fails presentation transformation and that a trajectory computation error is propagated rather than serialized as a replacement value.
- Existing invalid-Kline/error tests and updated projection parity tests continue to pass.

## Dependencies and stop condition

- Depends on Unit 01 only for the consumer contract; Unit 03 depends on its projected field names.
- Stop if the facade cannot supply all three channels under `Allow`, if timestamp conversion needs an unplanned dependency/API change, or if retaining any RSSI/Sharpe field is required for unrelated behavior; report rather than expand scope.

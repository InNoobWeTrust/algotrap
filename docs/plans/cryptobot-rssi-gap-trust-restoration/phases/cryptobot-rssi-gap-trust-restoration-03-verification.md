# P03 — Bounded Contract Verification

## Unit ID and outcome

**P03.** Verify the completed repair exclusively through focused Cryptobot tests and static contract inspection, demonstrating confidence restoration while proving all prohibited RSSI/chart/query/ATR changes remain absent.

## Writable surface

- No production files.
- `[MODIFY]` only the existing focused test assertions in `bins/cryptobot/src/presentation.rs` or `bins/cryptobot/src/main.rs` if P01/P02 demonstrably omitted one of this plan’s locked acceptance checks.

## Verification contract

1. Run the existing focused Cryptobot Rust test target/filter(s) that execute `presentation.rs` and `main.rs` tests, then the Cryptobot package test target.
2. Run the project-established Rust formatting and lint checks applicable to Cryptobot, if available without installing/fetching anything.
3. Inspect the final implementation scope mechanically and review the two source files for the following negatives:
   - no modified/new file under `src/query/**` or `src/ta/**`;
   - no `GapZoneRecord` field change;
   - no `rssi`, `rssi_ma`, EMA(9), or trust in source/chart SQL projection, candle JSON, or public chart fields;
   - no RSSI chart/tint/placeholder/fallback;
   - no hardcoded neutral gap-strength fallback;
   - no ATR Reversion input/multiplier/centering change.
4. Review positives:
   - internally raw `RSI(14)` receives `open + bar_bias`;
   - `trust = body_ratio * (0.5 + 0.5 * abs(rssi - 50) / 50)`;
   - each `gapZones` object has finite top-level `trust` sourced by exact zone timestamp association;
   - `GapZonePrimitive` uses supplied trust for both opacity and border strength;
   - raw geometry/direction/order and existing I-Ching/Reverse RSI/ATR/Structure panels are unchanged.

## Acceptance criteria and required evidence

- The focused tests prove all three formula points: RSI 50 gives `0.5 * body_ratio`; RSI 0 and 100 give `body_ratio`.
- A time-association fixture proves selected zones get the confidence belonging to their exact millisecond source candle and an absent association errors rather than falling back.
- Schema/JSON/template checks prove RSSI remains absent from candle fields, SQL projection, public chart fields, and panes; `trust` appears only per gap-zone JSON object.
- Existing ATR Reversion test passes and continues to establish close-vs-open-centered `ATR(42) * 1.618` behavior.
- Focused Cryptobot test/package, formatting, and lint outputs are recorded as pass/fail evidence. If a named check is unavailable under the no-command/no-network planning safety rules, record it as unavailable rather than compensating with a new tool or broader execution.
- Final scope review identifies only `bins/cryptobot/src/presentation.rs` and `bins/cryptobot/src/main.rs` as implementation edits, alongside these requested plan documents.

## Dependencies and prerequisites

- Requires P01 and P02 to be complete.

## Stop conditions

Immediately stop, clean up any newly created in-scope temporary artifact, and report the exact breach if a check requires network access, dependency installation, background processes, browser/Git/.env/secret/host-PID/socket access, more than one isolated planner, more than 1 CPU/1 GiB/2 PID, or a file outside the declared surfaces. Do not retry recursively or dispatch subagents.

# Unit 04 — Focused Verification and Boundary Check

## Outcome


> **Status: superseded (Sep 2026).** The I-Ching pane now renders the candlestick trajectory model from `src/ta/iching/trajectory.rs` (Original = intra-bar candle, Transformed/Nuclear = bar-close projection lines). This historical plan text is retained as design context only.
Run only the repository's focused existing checks necessary to demonstrate the completed replacement and verify no prohibited surface changed.

## Writable surface

- No implementation files. `[MODIFY]` only existing focused test assertions in `bins/cryptobot/src/presentation.rs` or `bins/cryptobot/src/main.rs` if earlier units left a demonstrable contract gap.

## Invariants and contracts

- Verification does not alter `src/ta/iching/**`, dependencies, configuration, `.env*`, output, APIs, or chart architecture.
- Check the final contract mechanically where current test placement supports it: source/projected schemas, timestamp/facade failure propagation, `Allow` policy use, and embedded-template pane/series/removal strings.

## Acceptance criteria and required evidence

- `cargo test -p cryptobot presentation` (or the repository-equivalent focused presentation test filter) passes.
- `cargo test -p cryptobot` passes, including updated embedded-template assertions if present.
- `cargo fmt --check -p cryptobot` and `cargo clippy -p cryptobot -- -D warnings` pass when available in the established workspace tooling.
- Final diff/scope inspection confirms only `bins/cryptobot/UX-SPEC.md`, `bins/cryptobot/src/presentation.rs`, and `bins/cryptobot/src/main.rs` changed for implementation; confirms no RSSI/Sharpe presentation/chart behavior and exactly the three locked I-Ching columns/labels/colors/pane.

## Dependencies and stop condition

- Depends on Units 01–03.
- Stop immediately on a containment/boundary breach, unavailable required check, or failure requiring an out-of-scope file. Report the exact failed check and do not broaden scope. No retries, background processes, network, browser, Git, `.env`, host PID/socket, or secret access are permitted.

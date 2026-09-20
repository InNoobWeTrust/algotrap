# Cryptobot I-Ching Energy Panel — L2 Implementation Plan

> **Status: superseded (Sep 2026).** The three-line contract below was replaced by the candlestick trajectory model in `src/ta/iching/trajectory.rs` (`iching_bar_trajectory`). Original is now a candlestick range `[open, close] × [low, high]`; Transformed and Nuclear are projection lines at bar closes; six columns plus legacy energy aliases ship. See PR #22 for the current implementation. Historical text retained as design context.

## Objective

Replace the visible RSSI pane and Sharpe pane with exactly one I-Ching Energy pane. The pane renders three simultaneous solid `LineSeries` from the existing I-Ching facade: Original, Transformed, and Nuclear. This is a bounded replacement; all unrelated indicators, including Reverse RSI overlays in the price pane, Structure Power, ATR Reversion, candles, market APIs, configuration, and `src/ta/iching/**` remain unchanged.

## Discovery and locked integration contract

- Verified producer boundary: `bins/cryptobot/src/presentation.rs` owns `CryptoIndicators`, `CryptoIndicatorRow`, source-frame columns, chart SQL projection, and presentation tests. `Kline.time` is milliseconds.
- Verified consumer boundary: `bins/cryptobot/src/main.rs` owns the embedded Lightweight Charts template. Browser chart time is derived by `Math.floor(d.time / 1000)`.
- Verified facade: `algotrap::ta::plum_blossom_signal_with_policy(DateTime<Utc>, LeapMonthPolicy)` returns `TaResult<IchingSignal>`; Plum Blossom supplies a transformed channel. The presentation layer selects `LeapMonthPolicy::Allow` for **every** candle.
- Locked JSON/chart data columns, nullable numeric at each input candle time: `iching_original_energy`, `iching_transformed_energy`, `iching_nuclear_energy`.
- Timestamp contract: convert each Kline millisecond timestamp to `DateTime<Utc>` in presentation. Invalid or out-of-range timestamps return the presentation transformation error; no fallback timestamp is allowed.
- Signal failure contract: propagate facade errors as presentation transformation errors; do not substitute zero, omit a row, or silently serialize null for a true facade failure. The transformed channel is serialized from Plum Blossom's transformed energy.
- UI contract: pane indices after replacement are price `0`, Structure Power `1`, ATR Reversion `2`, I-Ching Energy `3`. The new pane contains exactly three `LineSeries`, and exactly one watermark/legend identifies all three labels:
  - `I-Ching Original` — solid line, `#4FC3F7` (evaluated main energy)
  - `I-Ching Transformed` — dashed (`lineStyle: 2`) + transparent, `rgba(255,183,77,0.55)` (derived prediction of the next state)
  - `I-Ching Nuclear` — BaselineSeries zero-split area with transparent positive/negative fills (`rgba(206,147,216,…)` / `rgba(156,39,176,…)`)
  These opaque, non-gradient colors are separately WCAG-distinguishable on the existing `#22222240` dark chart background. The natural series scale is `[-31.5, 31.5]`; do not clamp, normalize, or add a derived scale.
- Removal is semantic, not concealment: delete RSSI calculation/state/output/source fields/SQL color and derived values/chart series, bindings, watermark, and RSSI tint CSS/runtime classes. Delete Sharpe calculation/state/output/source fields/SQL color/chart series, binding, and watermark. Preserve Reverse RSI price overlays despite their names.

## Target file tree

| File | Action | Scope |
|---|---|---|
| `docs/plans/cryptobot-iching-panel/plan.md` | `[CREATE]` | This L2 parent plan |
| `docs/plans/cryptobot-iching-panel/phases/cryptobot-iching-panel-01-ux-spec.md` | `[CREATE]` | UX-spec atomic unit |
| `docs/plans/cryptobot-iching-panel/phases/cryptobot-iching-panel-02-presentation.md` | `[CREATE]` | Rust transformation/schema/removal unit |
| `docs/plans/cryptobot-iching-panel/phases/cryptobot-iching-panel-03-chart.md` | `[CREATE]` | Embedded chart replacement unit |
| `docs/plans/cryptobot-iching-panel/phases/cryptobot-iching-panel-04-verification.md` | `[CREATE]` | Focused checks unit |
| `bins/cryptobot/UX-SPEC.md` | `[CREATE]` during implementation | Quick-track UI contract |
| `bins/cryptobot/src/presentation.rs` | `[MODIFY]` during implementation | All backend calculation, projection, and presentation tests |
| `bins/cryptobot/src/main.rs` | `[MODIFY]` during implementation | Embedded template and its focused tests |

No other file is writable during implementation. In particular, `src/ta/iching/**`, dependencies, config, `.env*`, output artifacts, market/API code, and external test infrastructure are out of scope.

## Quick-track UX specification

Quick-track is justified because this is one replacement pane in an established single-page chart: no new navigation, controls, or interaction behavior. Before chart code, materialize `bins/cryptobot/UX-SPEC.md` as the implementation contract. It must specify: the four pane locations; the three exact labels and color tokens/values above; a single watermark/legend listing all three labels with matching colors; natural `[-31.5, 31.5]` energy display; time alignment with candle seconds in the browser; no RSSI tint; no RSSI/Sharpe pane or label; and unchanged price/Reverse RSI/Structure/ATR behavior.

## Ordered functional units

1. [`cryptobot-iching-panel-01-ux-spec.md`](phases/cryptobot-iching-panel-01-ux-spec.md) — create the compact UX contract.
2. [`cryptobot-iching-panel-02-presentation.md`](phases/cryptobot-iching-panel-02-presentation.md) — replace Rust-side RSSI/Sharpe data production with I-Ching energy fields and tests.
3. [`cryptobot-iching-panel-03-chart.md`](phases/cryptobot-iching-panel-03-chart.md) — replace RSSI/Sharpe chart behavior with the three-series I-Ching pane.
4. [`cryptobot-iching-panel-04-verification.md`](phases/cryptobot-iching-panel-04-verification.md) — execute bounded focused quality gates and inspect the final contracts.

## Cross-unit safety and stop rules

- Execution is isolated and bounded to one implementer, no recursion, child agents, background work, shell commands, network access, Git operations, browser use, `.env`/secret access, host PID/socket inspection, or additional writable files. Runtime budget: at most 1 CPU, 1 GiB RAM, and 2 PIDs.
- Stop immediately, make no out-of-scope change, and report if containment is unavailable, any required edit escapes the listed writable surface, a new dependency or `ta::iching` change appears necessary, the facade contract conflicts with the plan, or a required quality gate cannot be run within these constraints. Clean up only newly created implementation artifacts within the declared writable surface on a boundary breach.
- No source implementation or test body belongs in this plan or phase files.

## Completion gate

The feature is complete only when all four units meet their acceptance criteria: serialized records expose exactly the three locked I-Ching fields and no RSSI/Sharpe fields; all valid candle timestamps map one-for-one to all three energies under `Allow`; the chart has one pane at index 3 with exactly three I-Ching `LineSeries`, one identifying watermark, and no RSSI/Sharpe runtime behavior; all focused checks pass.

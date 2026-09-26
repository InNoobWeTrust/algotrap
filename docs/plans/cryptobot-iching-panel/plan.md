# Cryptobot I-Ching Energy Panel — L2 Implementation Plan

> **Status: implemented (Sep 2026).** Reflects the final trajectory + mutual-band model shipped in this PR (src/ta/iching/trajectory.rs, bins/cryptobot/src/presentation.rs, bins/chartlib renderer, bins/cryptobot/UX-SPEC.md).
> Mutual (互卦, formerly Nuclear) renders as three `BaselineSeries` (high/low edge lines + line-only mean); all 13 I-Ching trajectory columns from `iching_bar_trajectory` ship.

## Objective

Replace the visible RSSI pane and Sharpe pane with exactly one I-Ching Energy pane. The pane renders five series from `iching_bar_trajectory`: a stepped `LineSeries` average (本卦 Original, OHLC/4), a dashed stepped `LineSeries` close projection (变卦 Transformed), and three zero-split `BaselineSeries` — high/low edge lines plus line-only mean (互卦 Mutual inner band). This is a bounded replacement; all unrelated indicators, including Reverse RSI overlays in the price pane, Structure Power, ATR Reversion, candles, market APIs, configuration, and `src/ta/iching/**` remain unchanged.

## Discovery and locked integration contract

- Verified producer boundary: `bins/cryptobot/src/presentation.rs` owns `CryptoIndicators`, `CryptoIndicatorRow`, source-frame columns, chart SQL projection, and presentation tests. `Kline.time` is milliseconds.
- Verified consumer boundary: `bins/cryptobot/src/main.rs` owns the embedded Lightweight Charts template. Browser chart time is derived by `Math.floor(d.time / 1000)`.
- Verified facade: `algotrap::ta::plum_blossom_signal_with_policy(DateTime<Utc>, LeapMonthPolicy)` returns `TaResult<IchingSignal>`; Plum Blossom supplies a transformed channel. The presentation layer selects `LeapMonthPolicy::Allow` for **every** candle.
- Locked JSON/chart data columns from `iching_bar_trajectory` — open-cast energy aliases: `iching_original_energy` (= `energy_open`), `iching_transformed_energy` (= `transformed_open`), `iching_mutual_energy` (= `mutual_open`); Original envelope: `iching_open`, `iching_high`, `iching_low`, `iching_close`; terminal cast: `iching_moving_line` (nullable u8), `iching_transformed_close`, `iching_mutual_close`; Mutual band: `iching_mutual_high`, `iching_mutual_low`, `iching_mutual_mean`. All are nullable numeric.
- Timestamp contract: convert each Kline millisecond timestamp to `DateTime<Utc>` in presentation. Invalid or out-of-range timestamps return the presentation transformation error; no fallback timestamp is allowed.
- Signal failure contract: propagate facade errors as presentation transformation errors; do not substitute zero, omit a row, or silently serialize null for a true facade failure. The transformed channel is serialized from Plum Blossom's transformed energy.
- UI contract: pane indices after replacement are price `0`, Structure Power `1`, ATR Reversion `2`, I-Ching Energy `3`. The new pane contains two stepped `LineSeries` and three `BaselineSeries` (zero-split, `baseValue` price 0). Exactly one compact in-pane watermark identifies all three layer roles with their 卦 characters (本/变/互):
  - `本卦 I-Ching Original average` — stepped `LineSeries`, `rgba(79,195,247,0.95)`, 2px; value = (`iching_open`+`iching_high`+`iching_low`+`iching_close`)/4. Intra-bar OHLC range is usually tiny, so candlesticks collapse to flat ticks; the average line avoids this.
  - `变卦 Transformed projection` — stepped dashed `LineSeries` (`lineStyle: 2`), `rgba(255,183,77,0.40)`, 2px; value = `iching_transformed_close` (terminal moving-line cast destination).
  - `互卦 Mutual inner band + mean` — three `BaselineSeries` (v5, `baseValue` price 0): edge lines at `iching_mutual_high` / `iching_mutual_low` (positive teal-green `rgba(38,166,154,0.25)` / negative coral-red `rgba(239,83,80,0.25)`, 1px, fills 0.12→0.00 at zero); line-only mean at `iching_mutual_mean` (lighter tints `rgba(110,231,183,0.35)` / `rgba(252,165,165,0.35)`, 1px, fills 0.00). Lightweight-Charts v5 cannot fill between two dynamic lines — each series fills line→zero; the inner range reads as the subtraction between the edge lines, NOT fill overlap.
  The natural series scale is `[-31.5, 31.5]`; do not clamp, normalize, or add a derived scale.
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

Quick-track is justified because this is one replacement pane in an established single-page chart: no new navigation, controls, or interaction behavior. Before chart code, materialize `bins/cryptobot/UX-SPEC.md` as the implementation contract. It must specify: the four pane locations; the exact series labels (with 卦 characters), color tokens, and rendering semantics above; a compact in-pane watermark naming each channel with its 卦 character (本/变/互); natural `[-31.5, 31.5]` energy display; time alignment with candle seconds in the browser; no RSSI tint; no RSSI/Sharpe pane or label; and unchanged price/Reverse RSI/Structure/ATR behavior.

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

The feature is complete only when all four units meet their acceptance criteria: serialized records expose all 13 I-Ching trajectory columns (energy aliases + Original envelope + terminal cast + Mutual band) and no RSSI/Sharpe fields; all valid candle timestamps map one-for-one to trajectory data under `Allow`; the chart has one pane at index 3 with two stepped `LineSeries` and three `BaselineSeries`, one compact in-pane watermark identifying all three layer roles by their 卦 characters (本/变/互), and no RSSI/Sharpe runtime behavior; all focused checks pass.

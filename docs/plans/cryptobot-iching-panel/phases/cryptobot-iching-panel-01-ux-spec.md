# Unit 01 — Quick-Track UX Specification

## Outcome


> **Status: implemented (Sep 2026).** Reflects the final trajectory + mutual-band model shipped in this PR (src/ta/iching/trajectory.rs, bins/cryptobot/src/presentation.rs, bins/chartlib renderer, bins/cryptobot/UX-SPEC.md).
Create the bounded UI contract before chart implementation.

## Writable surface

- `[CREATE]` `bins/cryptobot/UX-SPEC.md` only.

## Invariants and contracts

- Quick-track rationale: one replacement pane within the established one-page chart; no navigation, controls, or interaction changes.
- Define pane order: price `0`, Structure Power `1`, ATR Reversion `2`, I-Ching Energy `3`.
- Define five series in pane `3` (two stepped `LineSeries` + three `BaselineSeries` zero-split at `baseValue` price 0) with their prescribed rendering semantics:
  - `本卦 I-Ching Original average` — stepped `LineSeries`, `rgba(79,195,247,0.95)`, 2px; value = (`iching_open`+`iching_high`+`iching_low`+`iching_close`)/4. Intra-bar OHLC range is usually tiny, so candlesticks collapse to flat ticks; the average line is used instead.
  - `变卦 Transformed projection` — stepped dashed `LineSeries` (`lineStyle: 2`), `rgba(255,183,77,0.40)`, 2px; value = `iching_transformed_close` (terminal moving-line cast destination, not another current observation).
  - `互卦 Mutual inner band + mean` — three `BaselineSeries` (Lightweight-Charts v5, `baseValue` price 0, zero-split): edge lines at `iching_mutual_high` / `iching_mutual_low` (positive teal-green `rgba(38,166,154,0.25)` / negative coral-red `rgba(239,83,80,0.25)`, 1px, fills 0.12→0.00 at zero); line-only mean at `iching_mutual_mean` (lighter tints `rgba(110,231,183,0.35)` / `rgba(252,165,165,0.35)`, 1px, fills 0.00). Lightweight-Charts v5 cannot fill between two dynamic lines — each series fills line→zero; the inner range reads as the subtraction between the edge lines, NOT fill overlap.
- Define one compact in-pane watermark in pane `3` identifying all three layer roles with their 卦 characters (本/变/互), natural energy range `[-31.5, 31.5]`, and candle-second time alignment.
- State that RSSI pane/tint and Sharpe pane are absent, while Reverse RSI price overlays and all unrelated chart behavior remain unchanged.

## Acceptance criteria and required evidence

- The file contains the quick-track justification, pane map, exact labels, exact color tokens/values, watermark/legend rule, time/range rule, removal rule, and unchanged-surface rule.
- Review confirms no implementation source, dependency, configuration, or interaction specification beyond this replacement panel was introduced.

## Dependencies and stop condition

- Prerequisite: none. Required before Unit 03.
- Stop when the UX contract is complete; do not edit chart source in this unit.

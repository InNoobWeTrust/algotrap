# Unit 01 — Quick-Track UX Specification

## Outcome


> **Status: superseded (Sep 2026).** The I-Ching pane now renders the candlestick trajectory model from `src/ta/iching/trajectory.rs` (Original = intra-bar candle, Transformed/Nuclear = bar-close projection lines). This historical plan text is retained as design context only.
Create the bounded UI contract before chart implementation.

## Writable surface

- `[CREATE]` `bins/cryptobot/UX-SPEC.md` only.

## Invariants and contracts

- Quick-track rationale: one replacement pane within the established one-page chart; no navigation, controls, or interaction changes.
- Define pane order: price `0`, Structure Power `1`, ATR Reversion `2`, I-Ching Energy `3`.
- Define three simultaneous series in pane `3` with their prescribed rendering semantics:
  - `I-Ching Original` / `#4FC3F7` — solid line representing the evaluated main energy.
  - `I-Ching Transformed` / `rgba(255,183,77,0.55)` — dashed line (`lineStyle: 2`) with reduced opacity, representing the predicted next state / derived transformation (secondary visual weight).
  - `I-Ching Nuclear` — BaselineSeries zero-split with transparent positive (`rgba(206,147,216,0.34)`) and negative (`rgba(156,39,176,0.34)`) fills, representing the nuclear underlying energy band.
- Define a single I-Ching watermark/legend that identifies all three exact labels with their colors, natural energy range `[-31.5, 31.5]`, and candle-second time alignment.
- State that RSSI pane/tint and Sharpe pane are absent, while Reverse RSI price overlays and all unrelated chart behavior remain unchanged.

## Acceptance criteria and required evidence

- The file contains the quick-track justification, pane map, exact labels, exact color tokens/values, watermark/legend rule, time/range rule, removal rule, and unchanged-surface rule.
- Review confirms no implementation source, dependency, configuration, or interaction specification beyond this replacement panel was introduced.

## Dependencies and stop condition

- Prerequisite: none. Required before Unit 03.
- Stop when the UX contract is complete; do not edit chart source in this unit.

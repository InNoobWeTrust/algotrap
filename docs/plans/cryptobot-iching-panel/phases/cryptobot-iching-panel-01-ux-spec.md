# Unit 01 — Quick-Track UX Specification

## Outcome

Create the bounded UI contract before chart implementation.

## Writable surface

- `[CREATE]` `bins/cryptobot/UX-SPEC.md` only.

## Invariants and contracts

- Quick-track rationale: one replacement pane within the established one-page chart; no navigation, controls, or interaction changes.
- Define pane order: price `0`, Structure Power `1`, ATR Reversion `2`, I-Ching Energy `3`.
- Define exactly three simultaneous solid line series in pane `3`: `I-Ching Original` / `#4FC3F7`, `I-Ching Transformed` / `#FFB74D`, `I-Ching Nuclear` / `#CE93D8`.
- Define a single I-Ching watermark/legend that identifies all three exact labels with their colors, natural energy range `[-31.5, 31.5]`, and candle-second time alignment.
- State that RSSI pane/tint and Sharpe pane are absent, while Reverse RSI price overlays and all unrelated chart behavior remain unchanged.

## Acceptance criteria and required evidence

- The file contains the quick-track justification, pane map, exact labels, exact color tokens/values, watermark/legend rule, time/range rule, removal rule, and unchanged-surface rule.
- Review confirms no implementation source, dependency, configuration, or interaction specification beyond this replacement panel was introduced.

## Dependencies and stop condition

- Prerequisite: none. Required before Unit 03.
- Stop when the UX contract is complete; do not edit chart source in this unit.

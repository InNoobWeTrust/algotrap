# Phase 05 — Telegram producer fixed render
| Status | Dependencies | Outcome |
|---|---|---|
| Planned | 01–03 | Retain required local values, adapt one selected dataset, fixed-render and capture at readiness. |
## Scope
| In | Out |
|---|---|
| Local Directional/I-Ching calculations, DTO adapter, fixed integration, Browserless readiness wait, render-only visual removals. | Cryptobot, Chartlib changes, Telegram delivery/messaging, RSSI/Sharpe analytics/decision changes. |
Use unchanged [Phase 02](02-chart-document-contracts.md). Locally calculate/retain Directional and Original/Transformed/Nuclear before adaptation; never default them. Preserve local RSSI/Sharpe/memory/prediction/configuration/tools/decision semantics; remove only their pane/series, RSSI tint, ATR arrows.
## File operations
```text
[MODIFY] bins/telegrambot/Cargo.toml
[MODIFY] Cargo.lock                   # generated telegrambot -> chartlib dependency resolution
[MODIFY] bins/telegrambot/src/presentation.rs
[MODIFY] bins/telegrambot/src/chart.rs
[MODIFY] bins/telegrambot/src/browserless.rs  # ready-only screenshot wait and failed/timeout rejection
[MODIFY] bins/telegrambot/src/main.rs
[MODIFY] bins/telegrambot/src/commands.rs
[MODIFY] bins/telegrambot/src/llm/tools.rs
[MODIFY] bins/telegrambot/src/bin/test_chart_render.rs  # migrate legacy renderer test to fixed Chartlib contract
[DELETE] bins/telegrambot/src/chart_template.html
[CLEANUP] bins/telegrambot/src/chart.rs
```
No `[CREATE]` operations.
Tests for fixed-document rendering and readiness belong in the existing `bins/telegrambot/src/chart.rs` test module; no test-binary file is created.

The existing `bins/telegrambot/src/bin/test_chart_render.rs` is migrated to the same fixed Chartlib document contract because it is part of the package test command and otherwise calls retired legacy rendering helpers. It must not preserve, reintroduce, or test RSSI tint, Sharpe/RSSI panes, ATR arrows, or legacy template functions. Direct `rustfmt --edition 2024` is permitted only on Phase 05 modified Rust files when the workspace-wide check reports pre-existing out-of-scope drift.

Telegram calculates `structure_directional = 3.0 * structure_power - 2.0 * structure_power_sma` locally from its existing LLM-tuned Structure values. It is required only when both source values are present; otherwise the existing aligned warm-up null semantics apply. `browserless.rs` replaces the fixed `waitForTimeout: 2000` behavior with a DOM wait that accepts only `data-chart-state="ready"` and returns an error when the chart becomes `failed` or the wait times out.

## TDD / verification
| RED | GREEN | REFACTOR |
|---|---|---|
| Prerequisite/slot/no-default tests; fixed mode and ready-only wait tests. | Local outputs, adapter, one fixed document, wait `ready`. | Remove template/render visuals only after callers pass. |
```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 cargo test -p telegrambot --locked
```
## Acceptance / stop
- One caller-selected embedded dataset; no picker/fetch/registry/fallback/switch; Browserless rejects pending/failed.
- **Stop:** no Cryptobot/chartlib edits. Dispatch [06](06-cross-bot-validation-rollout.md).

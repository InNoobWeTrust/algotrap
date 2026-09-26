# Phase 06 — Cross-bot validation rollout
| Status | Dependencies | Outcome |
|---|---|---|
| Planned | 02–05 | Prove parity/lifecycle, stage rollback, then clean up accepted legacy rendering. |
## Scope
| In | Out |
|---|---|
| DTO/render parity, lifecycle/rollback evidence, legacy renderer cleanup. | New contract/render/product/calculation/Docker behavior. |
## File operations
```text
[MODIFY] bins/cryptobot/src/main.rs
[MODIFY] bins/telegrambot/src/chart.rs
[CLEANUP] bins/cryptobot/src/main.rs
[CLEANUP] bins/telegrambot/src/chart.rs
```
No `[CREATE]` or `[DELETE]` operations; cleanup is conditional on accepted parity/rollback evidence.
## TDD / verification
| RED | GREEN | REFACTOR |
|---|---|---|
| Existing consumer tests cover v1 parity, panes/slots, ready lifecycle, rollback selection. | Independently stage both cutovers with rollback retained. | Remove redundant renderer only after acceptance. |
```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 cargo check --workspace --locked
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 cargo test -p chartlib --locked
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 cargo test -p cryptobot --locked
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 cargo test -p telegrambot --locked
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 make duckdb-test
```
## Acceptance / stop
- Both producers validate/render identical v1 shape/pane order; lifecycle and rollback are evidenced.
- **Stop:** legacy rendering only is removed; rollout complete.

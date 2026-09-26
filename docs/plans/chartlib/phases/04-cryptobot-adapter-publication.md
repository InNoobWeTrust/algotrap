# Phase 04 — Cryptobot adapter publication
| Status | Dependencies | Outcome |
|---|---|---|
| Planned | 01–03 | Adapt calculated Cryptobot values, publish valid registry/pair datasets, adopt interactive rendering. |
## Scope
| In | Out |
|---|---|
| Existing-value adaptation, registry/pair writing, interactive adoption, source-evidenced Docker context. | Chartlib changes, calculation changes, Telegram edits. |
Link to unchanged [Phase 02](02-chart-document-contracts.md): copy all slots/candles/gaps in order; preserve calculation/gap semantics. One valid pair file per entry; omit invalid pairs atomically.
## File operations
```text
[MODIFY] bins/cryptobot/Cargo.toml
[MODIFY] Cargo.lock                   # generated cryptobot -> chartlib dependency resolution
[MODIFY] bins/cryptobot/src/presentation.rs
[MODIFY] bins/cryptobot/src/main.rs
[MODIFY] bins/cryptobot/deployment/Dockerfile  # source evidence previously listed it
[CLEANUP] bins/cryptobot/src/main.rs           # accepted legacy interactive path only
```
No `[CREATE]` or `[DELETE]` operations.
## TDD / verification
| RED | GREEN | REFACTOR |
|---|---|---|
| Adapter tests prove all 15 scalar slots/candles/gaps copied; publication rejects invalid pairs. | Dependency, adapter, validated writer, renderer handoff. | Retire only replaced renderer. |
```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 cargo test -p cryptobot --locked
```
## Acceptance / stop
- Registry has exactly one safe matching valid dataset per entry; invalid pairs absent; interactive renderer adopted.
- **Stop:** no Telegram/chartlib source. Dispatch [06](06-cross-bot-validation-rollout.md).

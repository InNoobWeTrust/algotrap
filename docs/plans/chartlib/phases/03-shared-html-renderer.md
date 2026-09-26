# Phase 03 — Shared HTML renderer
| Status | Dependencies | Outcome |
|---|---|---|
| Planned | 01, 02 | Render valid v1 documents as pinned, ready-aware interactive/fixed four-pane HTML. |
## Scope
| In | Out |
|---|---|
| Rendering, pinned assets, modes/panes/readiness/static tests. | DTO changes, adapters/migrations, calculations, publication, Docker. |
Use the unchanged [Phase 02 contract](02-chart-document-contracts.md); validate before composition and fail closed.
## File operations
```text
[MODIFY] bins/chartlib/src/lib.rs
[CREATE] bins/chartlib/src/render.rs
[CREATE] bins/chartlib/src/templates/interactive.html
[CREATE] bins/chartlib/src/templates/fixed.html
[CREATE] bins/chartlib/tests/rendering.rs
```
No `[DELETE]` or `[CLEANUP]` operations.
## Locked interface
```rust
pub fn render_interactive_chart_html() -> String;
pub fn render_fixed_chart_html(document: &FixedChartDocument) -> Result<String, ChartContractError>;
```
Interactive references `registry.json`. Fixed embeds one document and has no picker/fetch/registry/fallback/delayed switch. Use exactly `<script src="https://cdn.jsdelivr.net/npm/lightweight-charts@5.0.8/dist/lightweight-charts.standalone.production.js" integrity="sha384-8J8e9bGIwf7e9BLO5rwf4zJwNRKcypGnvuGzORD/t4TrFA1gWbl3Hsi/RvyWwBKl" crossorigin="anonymous"></script>`; no other CDN or unversioned asset. Render Price, Structure Power, ATR Reversion, I-Ching; only Phase 02 slots. Gaps: bullish `rgba(33,150,243,0.12/.4)`, bearish `rgba(255,152,0,0.12/.4)`, flat `rgba(158,158,158,0.12/.4)`. DOM states are `pending`, `ready`, `failed`; capture accepts only `ready`.
## TDD / verification
| RED | GREEN | REFACTOR |
|---|---|---|
| Assert asset pin, panes/slots, modes, gaps, no fixed fallback, state transitions. | Implement templates/renderers. | Keep consumer branches absent. |
```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 cargo test -p chartlib --locked
```
## Acceptance / stop
- Static tests prove exact pane/mode/readiness behavior; invalid fixed input produces no HTML.
- **Stop:** no consumer/contract change. Dispatch [04](04-cryptobot-adapter-publication.md) and [05](05-telegram-producer-fixed-render.md).

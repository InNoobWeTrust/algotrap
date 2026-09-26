# Phase 01 — Workspace chartlib scaffold

| Status | Dependency | Outcome |
|---|---|---|
| Planned | — | Add only the compilable `chartlib` workspace package. |

## Scope
| In | Out |
|---|---|
| Workspace membership, package manifest, minimal crate root. | DTOs, renderer/assets, consumers, migrations, Docker, calculations. |

## File operations
```text
[MODIFY] Cargo.toml                 # workspace member
[MODIFY] Cargo.lock                 # generated dependency lock
[CREATE] bins/chartlib/Cargo.toml   # package/dependencies
[CREATE] bins/chartlib/src/lib.rs   # minimal root; no chart API
```
No `[DELETE]` or `[CLEANUP]` operations.

## TDD and verification
| RED | GREEN | REFACTOR |
|---|---|---|
| No test: compilation slice only. | Add member, manifest, root. | Keep all contract/render exports absent. |
```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 cargo check --workspace --locked
```
## Acceptance / stop
- Workspace resolves `chartlib`; check passes; no other surface changes.
- **Stop:** no contract is defined. Dispatch [02](02-chart-document-contracts.md).

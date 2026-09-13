# docs/archive — Superseded Documentation Index

This directory contains historical and superseded documentation that has been moved here to preserve the decision record without cluttering active guidance. **None of these documents reflect current implementation.** For current architecture, see:

- [docs/architecture.md](../architecture.md) — Root workspace architecture (current)
- [src/README.md](../../src/README.md) — Library source navigation (current)

---

## Why Documents Are Archived

Documents are moved here when:

1. A plan or migration has **completed** and the original proposal is no longer actionable.
2. The architecture **evolved** such that the design described no longer matches the live codebase.
3. Code the document describes is **absent from the current main working tree**.

---

## Index by Category

### Plans

| Document | Original Path | Date | Status | Reason |
|---|---|---|---|---|
| [polars-to-duckdb-migration-plan.md](plans/polars-to-duckdb-migration-plan.md) | `docs/polars-to-duckdb-migration-plan.md` | 2026-04-24 | Completed | Migration is complete; Polars has been removed; compute now runs through the typed Kernel/Processor aggregate with `DuckDBQuery` as an optional post-compute SQL projection layer. This document is a historical record, not the target architecture. |

### Research

| Document | Original Path | Date | Status | Reason |
|---|---|---|---|---|
| (none active) | — | — | — | Historical Polars-to-DuckDB evaluation artifacts have been removed; Git history retains the record. |

---

## Current Documentation

For living documentation, use these instead:

| Topic | Document |
|---|---|
| Workspace architecture and component boundaries | [docs/architecture.md](../architecture.md) |
| Library source navigation and dependency direction | [src/README.md](../../src/README.md) |
| Aggregate flow and DuckDB projection (current) | [docs/architecture/stream-and-duckdb-data-flow.md](../architecture/stream-and-duckdb-data-flow.md) |
| Engineering quality gates | [docs/engineering/quality-gates.md](../engineering/quality-gates.md) |
| Changelogs | `docs/changelogs/` (currently empty; historical entries retained in Git history) |

---

## Preservation Policy

- Historical body content is **never rewritten** in archived documents.
- Each archived file carries a banner at the top stating its superseded status and linking to the current architecture references.
- Source paths listed in this index are the paths where the files originally lived before archiving.
- Archive entries that reference removed current symbols or superseded architecture paths are retired rather than updated; Git history is the record.
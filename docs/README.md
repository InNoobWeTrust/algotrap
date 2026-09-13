# docs/README.md — Documentation Navigation Hub

> **Purpose**: Canonical entry point for all documentation. Use this hub to locate active guidance,
> specifications, changelogs, and historical context. Do not navigate `docs/` blindly —
> `docs/archive/` contains superseded material that does not reflect current implementation.

---

## Start Here

If you are new to the codebase, read in this order:

1. **[`docs/architecture.md`](./architecture.md)** — workspace structure, component diagram, binary roles, dependency rules.
2. **[`src/README.md`](../src/README.md)** — library module map, end-to-end data flow, public entry points.
3. **[`docs/architecture/stream-and-duckdb-data-flow.md`](./architecture/stream-and-duckdb-data-flow.md)** — detailed compute and projection wiring (read when working in the `engine`, `ta`, or `query` modules). The high-level data-flow stages are summarised in `docs/architecture.md` §6.

> **`docs/archive/` is not current guidance.** All files there are superseded or completed.

---

## Document Status Conventions

| Badge | Meaning |
|---|---|
| **Current** | Reflects the live codebase; updated when the implementation changes. |
| **Implemented** | Specification fully realised; read for acceptance criteria, not future direction. |
| **Planned** | Proposed feature, not implemented; blocked on a stated prerequisite. Read for intent and research value, not live behaviour. |
| **Superseded** | Architecture evolved past this document; preserved for historical context only. |
| **Completed** | A plan or migration that has finished; no longer actionable. |

## Documentation Boundary

`docs/` contains **durable architecture, contracts, and decision-level guidance** — the enduring
shape of the workspace, layer boundaries, and decisions that outlive any single implementation.
It deliberately **excludes** volatile implementation detail, specifically:

- symbol inventories and type/variant counts
- CLI commands and shell invocations
- environment variables and their defaults
- in-app file paths and source-tree locations
- build and container invocations
- schedules and retry settings

**Where the detail lives instead:**

- Detailed code/API ownership → co-located `src/**/README.md` files and per-binary READMEs
  (kept in sync with the source; the authoritative API surface).
- Operational procedures → `docs/engineering/` (e.g., quality gates, pre-commit sequences).
- Concrete data-flow wiring → `docs/architecture/stream-and-duckdb-data-flow.md`.

`docs/` should **link to detail rather than copy it**: when a deeper document exists, follow it
instead of re-listing the detail here.

---

## Current Architecture

`docs/architecture.md` is the **system and workspace overview**: crate layout, binary roles,
library layer boundaries, and the prohibited dependency edges enforced by module structure and
tests. It is the authoritative orientation document for contributors and AI agents.

`docs/architecture/stream-and-duckdb-data-flow.md` is the **detailed compute and projection wiring**:
the aggregate flow, frame and projection contracts, and error boundaries. It captures the wiring
that `docs/architecture.md` deliberately keeps at the stage level.

| Document | Status |
|---|---|---|
| [`architecture.md`](./architecture.md) | Current |
| [`architecture/stream-and-duckdb-data-flow.md`](./architecture/stream-and-duckdb-data-flow.md) | Current |

---

## Module Interfaces

See [`src/README.md`](../src/README.md) for the canonical library module map and the
co-located [`src/**/README.md`](../src/README.md) files for authoritative per-module ownership
and API references.

---

## Durable Specifications

Specs that have been fully implemented. They are stable references for acceptance
criteria and architectural intent — not open proposals.

| Document | Status |
|---|---|---|
| [`specs/computed-frame-contract.md`](./specs/computed-frame-contract.md) | Implemented |

---

## Planned Features

Proposed features that are **not yet implemented**. They are blocked on root
library stabilization (typed analysis contract, result-frame semantics, projection seam,
and timestamp resolution).
Read these for product intent and domain research; do not treat their module layouts,
APIs, schemas, or verification commands as live contracts.

| Document | Status |
|---|---|---|
| [`architecture/iching-lunar-calendar-integration.md`](./architecture/iching-lunar-calendar-integration.md) | Planned |
| [`prds/research/iching-datetime-market-cycles.md`](./prds/research/iching-datetime-market-cycles.md) | Planned |

---

## Engineering / Operations

| Document | Status |
|---|---|---|
| [`engineering/quality-gates.md`](./engineering/quality-gates.md) | Current |

---

## Application Docs

Per-binary READMEs covering deployment, configuration, and runtime behaviour.

| Binary | README |
|---|---|
| `bins/cryptobot` | [`bins/cryptobot/README.md`](../bins/cryptobot/README.md) |
| `bins/telegrambot` | [`bins/telegrambot/README.md`](../bins/telegrambot/README.md) |

---

## Changelogs

| File | Covers |
|---|---|
| (none active) | The historical bot-deploy changelog has been removed; Git history retains the record. |

---

## Historical Archive

**`docs/archive/` is not current guidance.** Files there are superseded or completed — preserved
only to record the decision trail. Do not rely on them to understand the current architecture or
implementation.

See [`archive/README.md`](./archive/README.md) for the full indexed inventory with original
paths, dates, and reasons for archiving.

| Category |
|---|
| `archive/plans/` |
| `archive/research/` |

---

## Agent Governance

Repository-local agent guidance lives in [`../.agents/AGENTS.md`](../.agents/AGENTS.md).
Start there, then follow this hub for durable architecture, contracts, and decisions.

Project knowledge belongs in canonical `docs/`, per-binary READMEs/docs, and
`src/**/README.md`. Do not add session handoffs or stubs as project
documentation; Git history retains removed session material.

# `ta` — synchronous technical analysis

## Purpose

This module provides the typed, synchronous analysis domain. It turns model values into
stateful indicator results through explicit kernels, operators, and pure gap-zone analysis,
without binding that computation to an async runtime, database, or application workflow.

## Responsibility contract

**Owns**

- The typed kernel transition vocabulary: prior state, successor state, and row output.
- In-memory processing state and the operator families that implement technical analysis.
- Analysis errors and numerical safety at operator boundaries.

**Does not own**

- Stream adaptation, scheduling, or runtime concerns.
- Application aggregate definitions, result-frame projection, or presentation policy.
- SQL, DuckDB access, persistence, or external market-data I/O.

## Stable boundaries

`ta` depends on `model` and is consumed by the adapter and engine layers; it must not depend on
either of those downstream concerns. A transition is explicit: a successor state is adopted only
after the transition succeeds, so a failed aggregate step cannot commit partial state.

Numeric inputs and derived outputs must remain finite, and invalid periods or missing required
values are reported through the analysis error contract. Non-finite results are errors, never
silent substitutions. Applications own the aggregate composition and later decide how typed rows
become result frames.

## Navigation

- [`../../docs/architecture.md`](../../docs/architecture.md) — dependency direction and ownership rules
- [`../../docs/architecture/stream-and-duckdb-data-flow.md`](../../docs/architecture/stream-and-duckdb-data-flow.md) — analysis-to-frame boundary
- [`../model/README.md`](../model/README.md) — canonical analysis inputs
- [`../engine/README.md`](../engine/README.md) — downstream result-frame contract
- [`../README.md`](../README.md) — library-level navigation

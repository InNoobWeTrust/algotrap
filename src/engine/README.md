# `engine` — result-frame boundary

## Purpose

This module is the boundary between synchronous analysis results and downstream consumers. It
owns materialized frames, a backend-neutral read contract, and the public error vocabulary used
when analysis results are projected, accessed, or passed to query adapters.

## Responsibility contract

**Owns**

- The application-facing frame produced from stamped, typed analysis rows.
- The owned frame returned after an optional query projection.
- The read-only frame abstraction and the unified market error contract.
- Frame-level validation and analysis-to-frame utility boundaries.

**Does not own**

- Technical-analysis kernels or their state transitions.
- Stream adaptation, external I/O, SQL execution, or persistent storage.
- Application aggregate definitions, rendering, scheduling, or delivery policy.

## Stable boundaries

Applications own the aggregate and projector, including explicit column order and row alignment.
The engine owns the resulting frame data and exposes it without requiring consumers to know the
storage backend. Query adapters depend on engine types; engine must not depend on query adapters,
preserving SQL as an optional downstream concern.

Frame shape and column alignment are errors when inconsistent. Null remains null across frame
access and serialization. Non-finite analysis output is propagated with context and is never
silently converted to zero or another numeric value. Analysis errors are normalized into the
engine error contract for downstream callers.

## Navigation

- [`../../docs/architecture.md`](../../docs/architecture.md) — ownership and dependency direction
- [`../../docs/specs/computed-frame-contract.md`](../../docs/specs/computed-frame-contract.md) — canonical frame consumer contract
- [`../../docs/architecture/stream-and-duckdb-data-flow.md`](../../docs/architecture/stream-and-duckdb-data-flow.md) — projector and query boundaries
- [`../ta/README.md`](../ta/README.md) — upstream analysis contract
- [`../query/README.md`](../query/README.md) — downstream SQL projection contract

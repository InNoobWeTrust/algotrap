# `query` — SQL projection boundary

## Purpose

This module provides query adapters that shape already-computed engine frames. SQL is an
optional downstream projection step: it can select, filter, order, and materialize frame data,
but it is not part of technical-analysis execution or persistent storage.

## Responsibility contract

**Owns**

- The query value and adapter abstraction for projecting an engine frame.
- The DuckDB-backed implementation and translation of backend failures into `MarketError`.
- Preservation of frame nullability and the backend-neutral result-frame contract.

**Does not own**

- Aggregate execution, technical-analysis computation, stream adaptation, or model values.
- Application query selection, rendering, scheduling, or persistent database state.
- Safety for untrusted or user-authored query text.

## Stable boundaries

`query` depends on `engine` and is downstream of the application projector. It accepts a
completed frame and returns an owned frame compatible with the engine consumer contract. Query
adapters must not reach back into analysis or introduce a second computation model.

### Source-controlled SQL trust boundary

The raw query value represents SQL that the application owns and reviews in source control. The
query adapter passes that text through without sanitization, escaping, validation, or
parameterization. It must never be constructed from user input, environment variables, or other
external data. Applications that need an untrusted-query capability require a separate, explicit
design rather than weakening this contract.

Backend errors and invalid frame access remain observable through the shared engine error
contract; null cells remain null rather than being coerced into values.

## Navigation

- [`../../docs/architecture.md`](../../docs/architecture.md) — query ownership and dependency direction
- [`../../docs/architecture/stream-and-duckdb-data-flow.md`](../../docs/architecture/stream-and-duckdb-data-flow.md) — canonical frame-to-query boundary and SQL trust model
- [`../../docs/specs/computed-frame-contract.md`](../../docs/specs/computed-frame-contract.md) — result-frame consumer semantics
- [`../engine/README.md`](../engine/README.md) — upstream frame and error contract
- [`../README.md`](../README.md) — library-level navigation

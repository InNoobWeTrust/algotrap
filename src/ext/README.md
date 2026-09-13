# `ext` — external integration boundary

## Purpose

This module contains outbound integrations that cross the process boundary: market-data
sources, notification sinks, and optional browser automation. It translates external payloads
into library values or forwards application-owned delivery requests.

## Responsibility contract

**Owns**

- Source-specific transport, authentication, decoding, and response normalization.
- Conversion of market-data responses into canonical model values.
- Optional integration capabilities whose dependencies are isolated behind feature boundaries.

**Does not own**

- Domain model definitions, technical-analysis computation, or result-frame construction.
- Application credentials policy, scheduling, retry policy, rendering, or delivery decisions.
- Persistence or cross-source business rules.

## Stable boundaries

Market-data adapters return canonical candle collections in chronological oldest-first order,
regardless of the ordering used by an upstream service. Source-specific timestamp and payload
normalization ends at this boundary; downstream analysis consumes the model contract rather than
provider formats.

External failures remain observable to callers. Retry, backoff, scheduling, and decisions about
whether a notification or automation action is required belong to the application layer. An
integration must not introduce dependencies on analysis, engine, or query modules.

## Navigation

- [`../../docs/architecture.md`](../../docs/architecture.md) — external-integration ownership and dependency direction
- [`../model/README.md`](../model/README.md) — canonical values produced by market-data adapters
- [`../ta/README.md`](../ta/README.md) — downstream analysis boundary
- [`../README.md`](../README.md) — library-level navigation

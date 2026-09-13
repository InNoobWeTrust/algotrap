# `model` — canonical domain values

## Purpose

This module defines the small, dependency-free values shared across the library: market
candles, timeframe identity, and directional outcomes. These values are the stable vocabulary
at the centre of acquisition, analysis, and application projection.

## Responsibility contract

**Owns**

- The shape and meaning of the shared domain values.
- Their serialization, display, and parsing conventions at external boundaries.
- The canonical relationship between a timeframe and its ordering weight.

**Does not own**

- Market-data acquisition, persistence, caching, or source-specific normalization.
- Numerical validation, technical-analysis computation, or signal policy.
- Stream processing, result-frame construction, or application orchestration.

## Stable boundaries

`model` is a leaf in the library dependency graph. External adapters produce these values;
the analysis layer consumes them; applications decide how results are projected and presented.
Input candle collections passed into analysis are expected to be chronologically ordered
oldest-first. The model preserves that contract but does not perform runtime ordering or
finite-value validation; those concerns belong to the producing or consuming boundary.

Changes to field meaning, wire representation, or timeframe identity are compatibility changes
for every upstream adapter and downstream consumer and must be treated as domain-contract
changes.

## Navigation

- [`../../docs/architecture.md`](../../docs/architecture.md) — workspace boundaries and dependency direction
- [`../ext/README.md`](../ext/README.md) — external sources that produce domain values
- [`../ta/README.md`](../ta/README.md) — synchronous analysis that consumes candles
- [`../README.md`](../README.md) — library-level navigation

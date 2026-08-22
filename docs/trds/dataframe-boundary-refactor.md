# TRD: Dataframe Engine Boundary Refactor

> **Status**: implemented
> **Owner**: user
> **Created**: 2026-04-05
> **Updated**: 2026-08-21

## Parent Research

`docs/prds/research/dataframe-engine-build-strategy.md` records the historical
evaluation that led to the implementation below.

## Final Architecture

**DuckDB is the sole dataframe engine. Polars has been removed. There is no
Polars fallback or alternate dataframe backend.**

The boundary separates responsibility as follows:

| Layer | Responsibility |
|---|---|
| TA | Pure plans and indicator kernels over market data; validation and deterministic output calculation. |
| Engine | DuckDB sessions, UDF/table-function invocation, trusted presentation SQL, execution scheduling, result decoding, and materialization. |
| Consumers | Depend on `MarketFrameEngine` to compute frames and `ComputedFrame` to read results; they do not access DuckDB or TA execution internals directly. |

`src/engine/mod.rs` exposes the shared DuckDB-backed engine factory. Its
`DuckDBEngine` is the implementation of `MarketFrameEngine`; its
`DuckDBComputedFrame` is the materialized implementation of `ComputedFrame`.

## Implemented Interfaces

### `MarketFrameEngine`

`MarketFrameEngine: Send + Sync` is a consumer interface, not an engine-plugin
selection mechanism. It provides:

- `engine_identity(&self) -> &str`, which is `"duckdb"` for the current engine;
- `compute_telegram(&self, klines, ticker, indicators, config)`;
- `compute_crypto(&self, klines, ticker)`.

Both compute methods accept validated ticker/indicator inputs as applicable and
return `Result<Box<dyn ComputedFrame>, MarketError>`. DuckDB creates the
materialized frame, while TA plans/kernels calculate indicator data invoked by
the engine.

### `ComputedFrame`

`ComputedFrame: Send + Sync` is the materialized-table consumer interface. It
provides ordered column names, row count/emptiness, trailing-row slicing, typed
cell access, record-oriented JSON, and column existence checks. Exact behavior
is specified in `docs/specs/computed-frame-contract.md`.

## Execution and Safety Constraints

- DuckDB remains in-process; no network database or credentials are introduced.
- Engine SQL is produced by private, validated static builders. Consumer input
  is not interpolated as arbitrary SQL.
- DuckDB table functions/UDF integration, session lifecycle, and cross-request
  scheduling belong to the engine layer.
- Invalid input, failed materialization, unsupported result access, and
  non-finite indicator outputs use typed `MarketError` failures.
- DuckDB result allocations are decoded into owned column buffers before the
  `ComputedFrame` crosses the engine boundary.

## Historical Context (Completed Migration)

Before this refactor, the repository used **Polars** expressions and dataframes
throughout TA and downstream consumers. The original draft proposed retaining a
Polars adapter as an interim implementation and evaluating DuckDB later. That
proposal is historical only: the adapter, fallback, and staged dual-engine plan
were not retained in the finished architecture.

Historical Polars references are deliberately time-scoped here so they do not
describe current contracts or dependencies.

## Acceptance Outcome

1. TA owns pure plans and kernels rather than dataframe-engine expressions.
2. DuckDB owns engine execution, UDF/table-function integration, scheduling, and
   materialization.
3. `MarketFrameEngine` and `ComputedFrame` are the interfaces used by consumers.
4. Polars is removed, and no Polars fallback exists.

# TRD: Dataframe Engine Boundary Refactor

> **Status**: draft
> **Owner**: user
> **Created**: 2026-04-05

## Parent PRD

`docs/prds/research/dataframe-engine-build-strategy.md` — Provisional parent research artifact. Addresses goals: reduce cold-build pain for DataFrame-heavy services, validate whether a preinstalled compute engine is feasible, and avoid repeating full source builds for heavy analytics code.

## Technical Overview

This repo is not currently structured around a generic dataframe engine boundary. It is structured around Polars-native computation and Polars-native materialized tables. The core library in `src/ta/` returns `polars::Expr`, both bots materialize `DataFrame` via `df.lazy().with_columns(...).collect()`, and multiple downstream consumers accept `&DataFrame` directly.

That means a DuckDB migration is possible, but only after the codebase is refactored to separate three concerns that are currently collapsed together: indicator computation, materialized table access, and JSON/chart/LLM export. The first technical objective is not "swap Polars for DuckDB." It is "introduce an engine boundary that makes multiple compute backends possible without destabilizing downstream features."

This TRD proposes a staged refactor with Polars retained as implementation 1. The refactor starts at the leaves, where charting, LLM tools, and JSON export mostly need columnar access and serialized records rather than Polars expressions. After downstream consumers are decoupled, compute entrypoints can be hidden behind an engine interface. Only then is it safe to prototype a DuckDB-backed implementation for a bounded slice, starting with `cryptobot`.

## Architecture Decisions

### ADR-1: Introduce an internal engine boundary before evaluating DuckDB

- **Context**: The current shared TA layer in `src/ta/` exposes Polars `Expr` as its public contract. A direct dependency swap would require rewriting the entire indicator stack and every downstream consumer in one move.
- **Decision**: Add an internal engine boundary with two layers:
  - a materialized table contract for downstream consumers
  - a compute engine contract for producing that table from `Kline` input
- **Rationale**: This reduces migration blast radius, allows Polars to remain the first implementation, and creates a safe seam for a future DuckDB prototype.
- **Alternatives Considered**:
  - Directly replace Polars with DuckDB under current APIs — rejected because the current API surface is Polars-specific, especially `Expr` and `DataFrame`.
  - Keep optimizing Docker builds forever — rejected as the only long-term answer because it does not satisfy the requirement for a preinstalled compute boundary.

### ADR-2: Start the refactor from downstream consumers, not from `src/ta/`

- **Context**: Some code only needs a table-like view of computed candles, while `src/ta/` is the highest-coupling Polars surface in the repo.
- **Decision**: Refactor leaf consumers first:
  - `src/df_utils.rs`
  - `bins/telegrambot/src/chart.rs`
  - `bins/telegrambot/src/llm/tools.rs`
  - selected `HashMap<Timeframe, DataFrame>` call sites in `bins/telegrambot/src/main.rs` and `bins/cryptobot/src/main.rs`
- **Rationale**: Leaf-first migration creates immediate isolation value without forcing a high-risk rewrite of indicator math. It follows the safe-change order identified by module auditing.
- **Alternatives Considered**:
  - Rewrite indicators in SQL first — rejected because downstream code would still be tightly bound to `DataFrame`, leaving no rollback-safe path.
  - Wrap `DataFrame` with a type alias only — rejected because aliases do not block direct Polars method calls and provide no real abstraction.

### ADR-3: Keep Polars as the baseline implementation while designing for DuckDB as implementation 2

- **Context**: The system must continue shipping while the boundary is introduced. The current indicators and rendering flows already work in production.
- **Decision**: Implement the new interfaces first using Polars internally. Only after parity and verification should a DuckDB-backed prototype be attempted, initially for `cryptobot` only.
- **Rationale**: This preserves a known-good execution path, makes regressions measurable, and avoids forcing a large architectural migration and a backend migration in the same step.
- **Alternatives Considered**:
  - Build the new abstractions around DuckDB semantics from day one — rejected because it would bias the boundary toward SQL-specific assumptions before validating workload fit.
  - Move to Python Polars via `pyo3` — rejected because it introduces runtime and packaging complexity without solving the architectural coupling in this repo.

### ADR-5: Prefer Rust UDFs for DuckDB indicator migration where plain SQL is awkward

- **Context**: A later DuckDB implementation will need to preserve custom indicator behavior currently expressed through Polars `Expr` composition in `src/ta/`. Some of that logic maps cleanly to SQL window functions, while some of it is bespoke enough that a pure SQL rewrite would create avoidable churn.
- **Decision**: If a DuckDB backend is prototyped, prefer a mixed approach:
  - use SQL for straightforward projections, windows, and filtering
  - use Rust UDFs or equivalent in-process extension points for indicator logic that would otherwise require a high-churn rewrite
- **Rationale**: This lowers migration risk for indicator logic and keeps the focus on changing the engine boundary rather than gratuitously changing indicator semantics. It also preserves the option to keep complex technical-analysis math in Rust while still moving materialization onto DuckDB.
- **Alternatives Considered**:
  - Rewrite all indicator logic into SQL immediately — rejected because it increases semantic drift risk and migration cost.
  - Reject DuckDB unless every indicator can be expressed as SQL only — rejected because that sets an unnecessarily strict bar for backend evaluation.

### ADR-4: Use engine-neutral row/column access as the downstream contract

- **Context**: Chart rendering, LLM summaries, JSON export, and some alert logic primarily read named columns, slices, and last values from already-computed candles.
- **Decision**: Introduce a `ComputedFrame` abstraction that supports:
  - deterministic column lookup by name
  - row slicing for recent-candle windows
  - record-oriented JSON export
  - typed accessors for common scalar extraction
- **Rationale**: This is the minimum useful contract shared by current downstream consumers. It is narrow enough to support both Polars-backed and DuckDB-backed implementations.
- **Alternatives Considered**:
  - Expose Arrow tables directly everywhere — rejected for now because the current consumers are written around convenience operations, not Arrow kernels.
  - Expose SQL result sets directly — rejected because charting and LLM tooling need a stable in-process table API, not query handles.

## System Components

- **Indicator Engine Contract**: New internal trait responsible for transforming `&[Kline]` plus ticker-specific settings into a computed frame. Initial implementation remains Polars-backed. Planned home: shared library module such as `src/engine/`.
- **Computed Frame Contract**: Engine-neutral table wrapper or trait used by charts, LLM tools, and JSON serialization. Planned home: shared library module such as `src/frame/`.
- **Polars Engine Adapter**: Adapter that encapsulates current `to_dataframe()`, `lazy()`, `with_columns()`, and `collect()` flows in `bins/telegrambot/src/data.rs` and `bins/cryptobot/src/main.rs`.
- **Downstream Consumer Adapters**: Thin changes in charting, LLM tooling, and dataframe utilities so those modules depend on `ComputedFrame` instead of `polars::DataFrame`.
- **DuckDB Evaluation Slice**: A bounded prototype for `cryptobot` only after the abstraction is in place. This component is deferred until parity against the Polars-backed contract is established.
  A likely implementation path is DuckDB SQL for table shaping plus Rust UDFs for custom indicator logic that should stay close to existing Rust semantics.

## API Contracts / Interfaces

The names below are proposed contracts, not final Rust syntax. The important requirement is that callers stop depending on `polars::DataFrame` and `polars::Expr` outside the engine boundary.

### `MarketFrameEngine`

```rust
trait MarketFrameEngine {
    type Error;

    fn compute_telegram(
        &self,
        klines: &[Kline],
        ticker: &crate::config::TickerConf,
        indicators: &crate::memory::IndicatorConfig,
    ) -> Result<ComputedFrame, Self::Error>;

    fn compute_crypto(
        &self,
        klines: &[Kline],
        ticker: &crate::TickerConf,
    ) -> Result<ComputedFrame, Self::Error>;
}
```

Input:
  - `klines`: ordered market candles
  - ticker config: symbol-specific thresholds and defaults
  - indicator config: telegrambot-only dynamic indicator config

Output:
  - `ComputedFrame`: materialized candles plus derived columns

Errors:
  - engine build/materialization failure
  - missing required output columns
  - unsupported engine capability for requested indicator set

### `ComputedFrame`

```rust
trait ComputedFrame {
    fn len(&self) -> usize;
    fn columns(&self) -> &[String];
    fn slice_last(&self, count: usize) -> Result<Box<dyn ComputedFrameView>, FrameError>;
    fn f64_at(&self, column: &str, row: usize) -> Result<Option<f64>, FrameError>;
    fn string_at(&self, column: &str, row: usize) -> Result<Option<String>, FrameError>;
    fn to_json_records(&self) -> Result<serde_json::Value, FrameError>;
}
```

Input:
  - column name, row index, window size

Output:
  - typed values, slices, or JSON records

Errors:
  - unknown column
  - type mismatch
  - row out of bounds

### Migration Constraint Contract

Every implementation of `ComputedFrame` must preserve these observable behaviors:

- Column names and aliases currently consumed by charting and LLM tools remain stable.
- Recent-row slicing semantics match current `DataFrame::slice` use in bot flows.
- JSON serialization remains record-oriented and deterministic enough for templates and prompts.
- Missing optional columns degrade gracefully where current code already defaults, such as RSSI fallback in `last_rssi_from_df`.

## Data Models

### `RawKline`

Existing source model in `src/model/kline.rs`.

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `open` | `f64` | required | Candle open price |
| `high` | `f64` | required | Candle high price |
| `low` | `f64` | required | Candle low price |
| `close` | `f64` | required | Candle close price |
| `volume` | `f64` | required | Candle volume |
| `time` | timestamp-like number | required | Source market timestamp |

### `ComputedFrame`

Engine-neutral materialized table of raw OHLCV plus derived indicator columns.

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `rows` | ordered record set | non-empty for normal operation | Materialized candles in display/analysis order |
| `schema` | column metadata | required | Named columns and logical types |
| `engine` | enum/string | required | Producing backend, initially `polars` |

### `IndicatorSet`

Logical set of derived columns required by a consumer flow.

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `consumer` | enum | `telegrambot` or `cryptobot` | Declares the expected output shape |
| `required_columns` | list of strings | non-empty | Columns that must exist for parity |
| `optional_columns` | list of strings | may be empty | Columns with graceful fallback behavior |

## Security Assessment

> Applied `security-reviewer` lens to this TRD section.

### Authentication & Authorization

- **Auth model**: No new external API is introduced by this refactor. The engine boundary is internal process code only.
- **Access control**: The refactor must not widen who can trigger Browserless, Telegram, BingX, or LLM actions. Existing bot command and runtime boundaries remain unchanged.
- **Session management**: Not applicable at the engine layer.
- **Privilege boundaries**: The engine boundary must stay pure compute and serialization. It must not gain shell execution, SQL-over-network access, or arbitrary file I/O as part of migration convenience.

### Data Protection

- **Data classification**: The computed data is market data plus derived indicators. It is not high-sensitivity data, but prompt payloads and memory files can contain operational context.
- **Encryption at rest**: Out of scope for this TRD; existing deployment controls remain responsible.
- **Encryption in transit**: Out of scope for the engine layer; external clients keep existing transport behavior.
- **Data retention & deletion**: The refactor must not introduce new on-disk caches or temporary exports of computed frames without explicit design.
- **Secrets management**: No engine implementation may require embedding database credentials or API keys in code. If DuckDB is evaluated, it must start as an in-process embedded dependency, not a credentialed remote service.

### Input Validation & Injection Prevention

- **Input boundaries**: `Kline` arrays, ticker configs, and indicator configs are the primary inputs. Implementations must validate required columns before downstream use.
- **Injection vectors**: A DuckDB-backed implementation must not build SQL by interpolating untrusted column names or user strings into queries. Use fixed query templates and validated identifiers only.
- **File uploads**: Not applicable.

### Infrastructure & Configuration

- **Network boundaries**: The engine boundary must remain local to the process. A first migration slice should not introduce a networked query service.
- **Default credentials**: No credentials should be added.
- **Container/deployment security**: Avoid "helpful" runtime installs of Python packages or external DB engines inside mutable containers. If the motivation is build speed, solve it with a defined embedded runtime or prebuilt package strategy, not ad hoc startup scripts.

### Supply Chain & Dependencies

- **Dependency policy**: Adding DuckDB or Arrow crates increases supply-chain surface and must be pinned in `Cargo.lock`.
- **Third-party integrations**: Do not introduce a Python runtime or remote database as part of the first migration slice. Those change the trust boundary materially.

### Failure Modes

- **Fail-closed vs fail-open**: Missing required columns or engine-compute mismatches must fail closed before charting, LLM analysis, or notification logic runs. Silent partial frames are unacceptable.
- **Audit logging**: Engine identity and parity failures should be logged at startup or compute time for diagnosis, without logging secrets.
- **Incident response**: The fallback path during migration is the Polars-backed engine. Do not remove it until parity and deployment stability are verified.

## Non-Functional Requirements

- **Performance**: The Polars-backed boundary refactor must not increase end-to-end compute time by more than 10% for current telegrambot and cryptobot flows on equal inputs. Any DuckDB prototype must document cold-start time, steady-state compute time, and JSON export time against the current Polars baseline.
- **Scalability**: The new contracts must support both existing bots and at least one alternate backend without changing downstream call sites again.
- **Observability**: Compute logs must include engine name, consumer type, and explicit parity failures for missing columns or unsupported indicators.
- **Reliability**: Refactoring must preserve current bot behavior for chart rendering, LLM tool summaries, and alert generation. Polars remains the rollback-safe default until a second backend is proven.

## Child BDD Specs

- `docs/specs/computed-frame-contract.md` — Define observable behavior for slicing, column access, and JSON export.
- `docs/specs/polars-engine-adapter.md` — Verify that current telegrambot and cryptobot flows work through the new interfaces without behavior regressions.
- `docs/specs/cryptobot-duckdb-prototype.md` — Validate a bounded DuckDB implementation against the contract after Polars parity exists.

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: self-review with `security-reviewer` lens
> **Date**: 2026-04-05

This TRD must survive adversarial challenge before advancing to BDD specs.
The challenge included a security pass on the migration boundary and dependency choices.

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Assumptions | Why not skip the abstraction and rewrite `src/ta/` directly in DuckDB SQL? | Because the current repo has multiple downstream `DataFrame` consumers and a shared `Expr` contract. A direct rewrite would combine interface churn, indicator rewrites, and backend migration in one high-risk step. | author-won |
| 2 | Scope | Is this just architecture theater that delays the real migration? | No. The proposed boundary creates an immediately useful outcome: downstream consumers stop hard-coding Polars. That is a prerequisite for any credible backend evaluation. | author-won |
| 3 | Security | Could a DuckDB path accidentally introduce SQL injection or a new networked service boundary? | Yes, if implemented carelessly. The TRD explicitly forbids interpolated SQL and networked-engine scope in the first slice, and requires fail-closed validation for required columns. | author-won |
| 4 | Longevity | What if DuckDB turns out to be a poor fit for the indicator workload? | The boundary preserves Polars as implementation 1. The refactor still pays off because it narrows downstream coupling and improves future engine experimentation. | author-won |

### Challenge Summary

- **Challenges raised**: 4
- **Author victories**: 4
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED

### Revisions Made (if any)

- Tightened the scope to prohibit a direct backend swap and prohibit networked-engine expansion in the first migration slice.
- Added explicit fail-closed and SQL-construction constraints after security review.

## Notes

- Internal evidence for this TRD comes from:
  - `src/df_utils.rs` serializing `polars::DataFrame` directly
  - `bins/telegrambot/src/data.rs` and `bins/cryptobot/src/main.rs` materializing `DataFrame` via `with_columns(...).collect()`
  - `bins/telegrambot/src/chart.rs` and `bins/telegrambot/src/llm/tools.rs` consuming `&DataFrame` directly
  - `src/ta/*` exposing Polars `Expr` as the shared indicator contract
- This TRD intentionally does not commit to DuckDB as the final backend. It commits to the refactor that makes a credible evaluation possible.
- DuckDB Rust UDFs strengthen the migration case for indicators by reducing how much bespoke logic would need to be rewritten into SQL, but they do not remove the need for the boundary refactor described here.

## Implementation Status (2026-04-23)

> **Phase 1: Engine Boundary Module** — COMPLETED

The engine abstraction layer has been implemented in `src/engine/`:

| File | Status | Purpose |
|------|--------|---------|
| `src/engine/mod.rs` | ✅ | Public exports for engine module |
| `src/engine/error.rs` | ✅ | `MarketError` enum with `ErrorKind` variants |
| `src/engine/traits.rs` | ✅ | `MarketFrameEngine` and `ComputedFrame` traits |
| `src/engine/validation.rs` | ✅ | `Ticker`, `ValidatedTicker`, `ValidatedIndicator` |
| `src/engine/type_mapper.rs` | ✅ | DuckDB type mapping utilities |
| `src/engine/json_serializer.rs` | ✅ | JSON serialization with NaN policy |
| `src/engine/kline_batch.rs` | ✅ | `RawKlineBatch`, `BatchLimits` |
| `src/engine/polars_engine.rs` | ✅ | `PolarsEngine` implementation (indicators stubbed) |
| `src/engine/duckdb_engine.rs` | ✅ | `DuckDBEngine` stub |

**Updated**: `src/lib.rs` now exports `pub mod engine;`

**Compilation**: ✅ `cargo check` passes (no errors)

### Current State

The `PolarsEngine` currently provides:
- `compute_telegram()` — converts Kline → DataFrame (indicator computation stubbed)
- `compute_crypto()` — converts Kline → DataFrame (indicator computation stubbed)
- `ComputedFrame` implementation wrapping `DataFrame`

### What's Working

1. **Engine trait boundary**: `MarketFrameEngine` trait with `Send + Sync` enforcement
2. **ComputedFrame trait**: `len()`, `columns()`, `slice_last()`, `f64_at()`, `string_at()`, `to_json_records()`, `has_column()`
3. **Error handling**: `MarketError` with typed variants
4. **Input validation**: `ValidatedTicker` with ASCII-only validation
5. **DuckDB type mapping**: `DuckDbType` enum with Polars type conversion

### What's Stubbed

- `PolarsEngine::compute_telegram()` — does not apply telegram indicators yet
- `PolarsEngine::compute_crypto()` — does not apply crypto indicators yet
- `DuckDBEngine` — returns error indicating not implemented

## Next Steps

### Phase 2: Implement Indicator Logic in PolarsEngine

**Priority**: HIGH

**Tasks**:
1. Integrate telegram indicator expressions from `bins/telegrambot/src/data.rs::indicators()` into `PolarsEngine::compute_telegram()`
2. Integrate crypto indicator expressions from `bins/cryptobot/src/main.rs::indicators()` into `PolarsEngine::compute_crypto()`
3. Wire `IndicatorConfig` through the engine boundary

**Files to modify**:
- `src/engine/polars_engine.rs`

**Success criteria**:
- `PolarsEngine::compute_telegram()` produces same columns as `data.rs::process_data()`
- `PolarsEngine::compute_crypto()` produces same columns as `cryptobot::main.rs::process_data()`

### Phase 3: Update Downstream Consumers

**Priority**: HIGH

**Tasks**:
1. Update `bins/telegrambot/src/data.rs` to use `PolarsEngine` instead of inline `process_data()`
2. Update `bins/telegrambot/src/chart.rs` to use `ComputedFrame` instead of `&DataFrame`
3. Update `bins/telegrambot/src/llm/tools.rs` to use `ComputedFrame` instead of `&DataFrame`
4. Update `bins/telegrambot/src/main.rs` helper functions
5. Update `bins/cryptobot/src/main.rs` to use `PolarsEngine` and `ComputedFrame`

**Files to modify**:
- `bins/telegrambot/src/data.rs`
- `bins/telegrambot/src/chart.rs`
- `bins/telegrambot/src/llm/tools.rs`
- `bins/telegrambot/src/main.rs`
- `bins/cryptobot/src/main.rs`

**Success criteria**:
- No direct `polars::DataFrame` or `&DataFrame` in downstream consumer signatures
- All consumers use `ComputedFrame` interface

### Phase 4: DuckDB + Rust UDF Evaluation Slice

**Priority**: MEDIUM

**Prerequisites**: Phase 2 and Phase 3 complete with Polars parity verified

**Tasks**:
1. Implement `DuckDBEngine::compute_crypto()` for `cryptobot` only
2. Use DuckDB SQL for table shaping (OHLCV projection, window functions)
3. Implement Rust UDFs for complex indicators (RevRSI, band reversion, gap zones)
4. Document cold-start and steady-state performance vs Polars baseline

**Files to create/modify**:
- `src/engine/duckdb_engine.rs` — implement fully
- New Rust UDF modules as needed

**Child BDD Specs Required**:
- `docs/specs/cryptobot-duckdb-prototype.md`

---

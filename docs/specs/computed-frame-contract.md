# BDD Spec: ComputedFrame Contract

> **Status**: implemented
> **Owner**: user
> **Created**: 2026-04-23
> **Parent**: `docs/trds/dataframe-boundary-refactor.md`

## Scope

This specification describes the live `ComputedFrame: Send + Sync` consumer
trait and the current `DuckDBComputedFrame` implementation. DuckDB is the sole
engine; Polars is not a current implementation or fallback.

`DuckDBComputedFrame` owns decoded, columnar result buffers. DuckDB allocations
do not cross the consumer boundary.

## Live Trait Surface

```rust
trait ComputedFrame: Send + Sync {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn columns(&self) -> Vec<String>;
    fn slice_last(&self, count: usize) -> Result<Box<dyn ComputedFrame>, MarketError>;
    fn f64_at(&self, column: &str, row: usize) -> Result<Option<f64>, MarketError>;
    fn string_at(&self, column: &str, row: usize) -> Result<Option<String>, MarketError>;
    fn to_json_records(&self) -> Result<Vec<serde_json::Map<String, serde_json::Value>>, MarketError>;
    fn has_column(&self, column: &str) -> bool;
}
```

## Feature: Frame Shape

### Scenario: length and emptiness report materialized rows

**Given** a frame with 50 materialized rows
**When** a consumer calls `len()` and `is_empty()`
**Then** it receives `50` and `false`, respectively.

### Scenario: columns preserves materialized column order

**Given** a frame materialized with ordered columns `time`, `open`, and `close`
**When** a consumer calls `columns()`
**Then** it receives owned names in that order.

### Scenario: has_column checks the materialized schema

**Given** a frame with `close` but not `volume`
**When** a consumer calls `has_column`
**Then** `has_column("close")` is true and `has_column("volume")` is false.

## Feature: Trailing-Row Slicing

### Scenario: slice_last returns the trailing materialized rows

**Given** a frame with 100 rows
**When** a consumer calls `slice_last(10)`
**Then** it receives a new `ComputedFrame` containing rows 90 through 99 in
their existing order.

### Scenario: slice_last saturates and permits an empty slice

**Given** a frame with 5 rows
**When** a consumer calls `slice_last(10)`
**Then** it receives all 5 rows.
**And when** it calls `slice_last(0)`
**Then** it receives an empty frame.

## Feature: Typed Cell Access

### Scenario: numeric access supports DuckDB numeric buffers

**Given** a numeric DuckDB result column holding an `f64`, `i64`, or `u64` value
**When** a consumer calls `f64_at(column, row)` within bounds
**Then** it receives that value as `Ok(Some(f64))`.

### Scenario: null numeric or string cells remain null

**Given** a null cell in a numeric, string, or all-null DuckDB result column
**When** the corresponding typed accessor is called within bounds
**Then** it receives `Ok(None)`; null is not an access error.

### Scenario: string access requires a string-compatible buffer

**Given** a UTF-8 result column
**When** a consumer calls `string_at(column, row)` within bounds
**Then** it receives `Ok(Some(String))` unless that cell is null.

### Scenario: invalid cell access fails with DataAccessError

**Given** a missing column, an out-of-bounds row, or an incompatible column type
**When** a consumer calls `f64_at` or `string_at`
**Then** it receives `Err(MarketError)` whose kind is `DataAccessError`.

Numeric access accepts `Float64`, `Int64`, and `UInt64` buffers; it rejects
boolean and UTF-8 buffers. String access accepts only UTF-8 buffers (besides
the all-null buffer behavior above).

## Feature: Record-Oriented JSON

### Scenario: JSON export creates one object per row

**Given** a materialized frame
**When** a consumer calls `to_json_records()`
**Then** it receives `Vec<Map<String, Value>>` with one map per row and one key
per materialized column.

### Scenario: JSON values reflect owned buffer values

**Given** DuckDB-decoded float, signed integer, unsigned integer, boolean,
UTF-8, and null buffers
**When** JSON records are exported
**Then** cells are emitted as JSON number, boolean, string, or null,
respectively. Non-finite floating values serialize as JSON null.

## Non-Goals

- `ComputedFrame` does not expose DuckDB query handles, Arrow tables, or mutable
  dataframe operations.
- It does not promise a Polars-compatible API, Polars slicing semantics, or a
  Polars fallback.
- Optional consumer defaults, such as an RSSI default, are consumer behavior and
  are not implemented by `ComputedFrame` accessors.

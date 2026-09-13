# BDD Spec: ComputedFrame Contract

> **Status**: implemented
> **Owner**: user
> **Created**: 2026-04-23
> **Updated**: 2026-09-09
> **Parent**: `docs/architecture/stream-and-duckdb-data-flow.md`

## Scope

This specification describes the live `ComputedFrame: Send + Sync` consumer
trait and the two concrete implementations that satisfy it:

- **`SourceFrame`** — produced by the application projector after aggregate computation
  (`src/engine/frame.rs`). Owns `SourceColumnData::Number(Vec<Option<f64>>)`,
  `SourceColumnData::Boolean(Vec<Option<bool>>)`, and
  `SourceColumnData::Text(Vec<Option<String>>)` columns. Column values come directly
  from the typed aggregate output fields.
- **`QueryResultFrame`** — produced by `DuckDBQuery::project` after an in-process SQL
  projection (`src/engine/frame.rs`). Owns decoded DuckDB result columns
  (`Float64`, `Int64`, `UInt64`, `Boolean`, `Utf8`, `Null`). DuckDB allocations
  do not cross the consumer boundary.

Both types implement `ComputedFrame` and are `Send + Sync`. Neither exposes
DuckDB handles, Arrow tables, or mutable dataframe operations.

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

## Frame Shape

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

## Trailing-Row Slicing

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

## Typed Cell Access

### SourceFrame cell types

`SourceFrame` columns carry three types sourced directly from the typed aggregate output contract:

| `SourceColumnData` variant | Null element | `f64_at` | `string_at` |
|---|---|---|---|
| `Number(Vec<Option<f64>>)` | `None` → `Ok(None)` | `Ok(Some(f64))` or `Ok(None)` | `Err(DataAccessError)` — not UTF-8 |
| `Boolean(Vec<Option<bool>>)` | `None` → `Ok(None)` | `Err(DataAccessError)` — not numeric | `Err(DataAccessError)` — not UTF-8 |
| `Text(Vec<Option<String>>)` | `None` → `Ok(None)` | `Err(DataAccessError)` — not numeric | `Ok(Some(String))` or `Ok(None)` |

`string_at` succeeds for `Text` columns and returns `Err(DataAccessError)` for
missing or non-text `SourceFrame` columns. Boolean columns are accessible via
`to_json_records()` but not via `f64_at` or `string_at`.

### QueryResultFrame cell types

`QueryResultFrame` columns carry decoded DuckDB result types:

| `QueryResultColumn` variant | Null element | `f64_at` | `string_at` |
|---|---|---|---|
| `Null` | always null | `Ok(None)` | `Ok(None)` |
| `Float64(Vec<Option<f64>>)` | `Ok(None)` | `Ok(Some(f64))` or `Ok(None)` | `Err(DataAccessError)` |
| `Int64(Vec<Option<i64>>)` | `Ok(None)` | cast to f64 via `as f64` | `Err(DataAccessError)` |
| `UInt64(Vec<Option<u64>>)` | `Ok(None)` | cast to f64 via `as f64` | `Err(DataAccessError)` |
| `Boolean(Vec<Option<bool>>)` | `Ok(None)` | `Err(DataAccessError)` | `Err(DataAccessError)` |
| `Utf8(Vec<Option<String>>)` | `Ok(None)` | `Err(DataAccessError)` | `Ok(Some(String))` or `Ok(None)` |

The `Null` variant is produced for SQL `NULL`-typed columns; all cells return
`Ok(None)` regardless of accessor type.

### Scenario: numeric access on SourceFrame Number column

**Given** a `Number` column in a `SourceFrame` holding `Some(42.5)` at row 0
**When** a consumer calls `f64_at("col", 0)`
**Then** it receives `Ok(Some(42.5))`.

### Scenario: numeric access on QueryResultFrame numeric buffers

**Given** a `QueryResultFrame` `Float64` column with `Some(1.5)`, an `Int64` column
with `Some(-2)`, and a `UInt64` column with `Some(3)` at row 0
**When** a consumer calls `f64_at` for each column at row 0
**Then** it receives `Ok(Some(1.5))`, `Ok(Some(-2.0))`, and `Ok(Some(3.0))`
respectively.

### Scenario: null cells remain null across all types

**Given** a null cell in a `Number`, `Float64`, `Int64`, `UInt64`, `Boolean`,
`Utf8`, or `Null` column
**When** the corresponding typed accessor is called within bounds
**Then** it receives `Ok(None)`; null is not an access error.

### Scenario: string access on QueryResultFrame UTF-8 column

**Given** a `Utf8` column in a `QueryResultFrame` with `Some("hello")` at row 0
**When** a consumer calls `string_at("col", 0)`
**Then** it receives `Ok(Some("hello".to_string()))`.

### Scenario: string access on SourceFrame Text column succeeds

**Given** a `Text` column in a `SourceFrame` with `Some("hello")` at row 0
**When** a consumer calls `string_at("col", 0)`
**Then** it receives `Ok(Some("hello".to_string()))`.

### Scenario: invalid cell access fails with DataAccessError

**Given** a missing column, an out-of-bounds row, or an incompatible column type
**When** a consumer calls `f64_at` or `string_at`
**Then** it receives `Err(MarketError)` whose kind is `DataAccessError`.

## Record-Oriented JSON

### Scenario: JSON export creates one object per row

**Given** a materialized frame
**When** a consumer calls `to_json_records()`
**Then** it receives `Vec<Map<String, Value>>` with one map per row and one key
per materialized column.

### Scenario: JSON values reflect owned buffer values — SourceFrame

**Given** a `SourceFrame` with `Number`, `Boolean`, and `Text` columns
**When** JSON records are exported
**Then** non-null `Number` cells are emitted as `Value::Number`, non-null
`Boolean` cells as `Value::Bool`, non-null `Text` cells as `Value::String`, and
null cells as `Value::Null`. A non-finite `f64` in a `Number` column returns
`Err(MarketError::ComputationError)` — it is not silently serialized as null.

### Scenario: JSON values reflect owned buffer values — QueryResultFrame

**Given** a `QueryResultFrame` with `Float64`, `Int64`, `UInt64`, `Boolean`, `Utf8`,
and `Null` columns
**When** JSON records are exported
**Then** non-null `Float64`/`Int64`/`UInt64` cells are emitted as
`Value::Number`, non-null `Boolean` as `Value::Bool`, non-null `Utf8` as
`Value::String`, and all null cells (including the `Null` column) as
`Value::Null`. A non-finite `Float64` cell returns
`Err(MarketError::ComputationError)` — it is not silently serialized as null.

## Non-Goals

- `ComputedFrame` does not expose DuckDB query handles, Arrow tables, or mutable
  dataframe operations.
- It does not promise a Polars-compatible API, Polars slicing semantics, or a
  Polars fallback.
- `SourceFrame` has no integer column variant; aggregate outputs are `Number`
  (`f64` nullable), `Boolean` (`bool` nullable), or `Text` (`String` nullable).
- Optional consumer defaults (e.g. an RSSI default) are consumer behavior and
  are not implemented by `ComputedFrame` accessors.

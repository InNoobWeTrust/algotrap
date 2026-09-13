//! DuckDB table function that scans a completed output frame.
//!
//! Newcomer guide to the three phases DuckDB drives:
//!
//! - `bind` runs once per query: it takes the parked frame (see `invocation`),
//!   unwraps sole ownership, snapshots its schema as [`ColumnDescriptor`]s, and
//!   tells DuckDB each column name plus its logical type. The owned frame is
//!   stored in `BindData` so it outlives every later scan call.
//! - `init` runs once per scan: it pins parallelism to one thread. That single
//!   thread is what makes the plain-`usize` cursor in `invocation` sound —
//!   concurrent `func` calls would hand out overlapping row windows and break
//!   insertion order.
//! - `func` runs once per output chunk: it claims the next row window, copies
//!   that slice into DuckDB vectors via `write_column`, and reports an empty
//!   chunk when the cursor is exhausted.
//!
//! Type mapping is fixed: `Number → DOUBLE`, `Boolean → BOOLEAN`,
//! `Text → VARCHAR`, preserving `None` as SQL `NULL`.

use std::error::Error;
use std::sync::Arc;

use duckdb::Connection;
use duckdb::core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId};
use duckdb::vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab};

use super::invocation;
use crate::engine::error::MarketError;
use crate::engine::frame::{SourceColumnData, SourceFrame};

/// SQL name users write (`SELECT … FROM computed()`). Kept argument-free so the
/// only data source is the parked frame — no parameters means no injection
/// surface beyond the already-trusted `RawQuery`.
const TABLE_FUNCTION_NAME: &str = "computed";
/// Single-threaded scan pin. Required because `invocation` tracks progress in a
/// plain `thread_local!` cursor with no lock; more threads would race, reorder,
/// or duplicate rows. Duplicate `time` ties therefore resolve in frame
/// insertion order, which gap-zone consumers rely on.
const MAX_SCAN_THREADS: u64 = 1;

struct ComputedFrameTable;

/// Bind-time snapshot: the owned frame plus its name→kind map.
/// `bind` builds this once so `func` never re-inspects the frame schema per
/// chunk — it just walks this small `Vec` and copies slices.
struct ComputedFrameBindData {
    columns: Vec<ColumnDescriptor>,
    frame: SourceFrame,
}

/// One column's SQL name and engine-side kind, captured at `bind` time.
/// The kind selects both the DuckDB `LogicalTypeId` and the `write_column`
/// copy path, keeping schema declaration and row emission in lockstep.
struct ColumnDescriptor {
    name: String,
    kind: ColumnKind,
}

/// Engine-side column kind driving the DuckDB type map.
/// Kept as a tiny closed enum so adding a future column type forces updates in
/// both `bind` (type declaration) and `write_column` (value copy) together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnKind {
    Number,
    Boolean,
    Text,
}

impl VTab for ComputedFrameTable {
    type BindData = ComputedFrameBindData;
    type InitData = ();

    /// Bind phase (once per query): reclaims the parked frame, snapshots its
    /// schema, and declares each column to DuckDB. `Arc::try_unwrap` proves no
    /// other owner still holds the frame; rejecting parameters keeps
    /// `computed()` argument-free so SQL cannot smuggle in extra inputs.
    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn Error>> {
        if bind.get_parameter_count() != 0 {
            return Err(
                MarketError::invocation_lifecycle("computed accepts no SQL parameters").into(),
            );
        }
        let frame = Arc::try_unwrap(invocation::take_frame_for_bind()?).map_err(|_| {
            MarketError::invocation_lifecycle(
                "computed bind expected sole ownership of its completed frame",
            )
        })?;
        let columns = frame
            .column_names()
            .into_iter()
            .map(|name| {
                let column = frame.column(name).ok_or_else(|| {
                    MarketError::computation("completed frame changed while being registered")
                })?;
                let kind = match column {
                    SourceColumnData::Number(_) => ColumnKind::Number,
                    SourceColumnData::Boolean(_) => ColumnKind::Boolean,
                    SourceColumnData::Text(_) => ColumnKind::Text,
                };
                Ok(ColumnDescriptor {
                    name: name.to_owned(),
                    kind,
                })
            })
            .collect::<Result<Vec<_>, MarketError>>()?;
        for column in &columns {
            bind.add_result_column(
                &column.name,
                match column.kind {
                    ColumnKind::Number => LogicalTypeId::Double.into(),
                    ColumnKind::Boolean => LogicalTypeId::Boolean.into(),
                    ColumnKind::Text => LogicalTypeId::Varchar.into(),
                },
            );
        }
        Ok(ComputedFrameBindData { columns, frame })
    }

    /// Init phase (once per scan): pins DuckDB to a single scan thread.
    /// Without this, DuckDB could call `func` concurrently and the
    /// thread-local cursor in `invocation` would hand out overlapping windows.
    fn init(init: &InitInfo) -> Result<Self::InitData, Box<dyn Error>> {
        init.set_max_threads(MAX_SCAN_THREADS);
        Ok(())
    }

    /// Scan phase (once per output chunk): copies the next row window into
    /// DuckDB vectors. `None` from `claim_next_chunk` means “all rows
    /// emitted”, so we return an empty chunk to end the scan; otherwise we
    /// copy exactly `rows` values per column and set the chunk length once.
    fn func(
        function: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn Error>> {
        let bind_data = function.get_bind_data();
        let capacity = output.flat_vector(0).capacity();
        let Some(start) = invocation::claim_next_chunk(bind_data.frame.len(), capacity) else {
            output.set_len(0);
            return Ok(());
        };
        let rows = capacity.min(bind_data.frame.len() - start);
        for (index, descriptor) in bind_data.columns.iter().enumerate() {
            let column = bind_data.frame.column(&descriptor.name).ok_or_else(|| {
                MarketError::computation("bound projection column is missing from completed frame")
            })?;
            write_column(output, index, column, start, rows)?;
        }
        output.set_len(rows);
        Ok(())
    }

    /// Declares that `computed` takes zero SQL arguments.
    /// Returning an empty vec (rather than `None`) tells DuckDB “no overloads
    /// accepted”, so `SELECT * FROM computed(1)` fails in `bind` instead of
    /// silently ignoring the argument.
    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(Vec::new())
    }
}

/// Installs the `computed()` table function on this connection if missing.
/// Checked via `duckdb_functions()` so nested `project` calls on the same
/// thread-local connection pay no re-registration cost; registration itself is
/// per-connection, which is why every `project` calls it after `with_session`.
pub(super) fn register(connection: &Connection) -> Result<(), MarketError> {
    let is_registered = connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM duckdb_functions() WHERE function_name = 'computed')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(map_duckdb_error)?;
    if is_registered {
        return Ok(());
    }
    connection
        .register_table_function::<ComputedFrameTable>(TABLE_FUNCTION_NAME)
        .map_err(map_duckdb_error)
}

/// Copies one `[start, start + rows)` frame slice into a DuckDB output vector.
/// Bounds: callers derive `rows` from `min(vector capacity, remaining rows)`,
/// so `start..start + rows` always lies inside the column — never extend it.
/// Nulls: each `None` sets the vector slot to SQL `NULL` first, and only
/// `Some` values are written afterwards, so a stale double/bool never shows
/// through a null. Strings: `Inserter::insert` copies the bytes into DuckDB
/// storage during the call, which is why borrowing the frame-owned `String` as
/// `&str` here is sound and nothing escapes the `func` invocation. The `unsafe`
/// slice writes are sound for the same capacity reason: `rows` never exceeds
/// the vector DuckDB handed us.
fn write_column(
    output: &DataChunkHandle,
    column_index: usize,
    column: &SourceColumnData,
    start: usize,
    rows: usize,
) -> Result<(), MarketError> {
    match column {
        SourceColumnData::Number(values) => {
            let values = &values[start..start + rows];
            let mut vector = output.flat_vector(column_index);
            for (row, value) in values.iter().enumerate() {
                if value.is_none() {
                    vector.set_null(row);
                }
            }
            // SAFETY: rows is bounded by DuckDB's vector capacity.
            let target = unsafe { vector.as_mut_slice_with_len::<f64>(rows) };
            for (row, value) in values.iter().enumerate() {
                if let Some(value) = value {
                    target[row] = *value;
                }
            }
        }
        SourceColumnData::Boolean(values) => {
            let values = &values[start..start + rows];
            let mut vector = output.flat_vector(column_index);
            for (row, value) in values.iter().enumerate() {
                if value.is_none() {
                    vector.set_null(row);
                }
            }
            // SAFETY: rows is bounded by DuckDB's vector capacity.
            let target = unsafe { vector.as_mut_slice_with_len::<bool>(rows) };
            for (row, value) in values.iter().enumerate() {
                if let Some(value) = value {
                    target[row] = *value;
                }
            }
        }
        SourceColumnData::Text(values) => {
            let values = &values[start..start + rows];
            let mut vector = output.flat_vector(column_index);
            for (row, value) in values.iter().enumerate() {
                if value.is_none() {
                    vector.set_null(row);
                }
            }
            // DuckDB copies string bytes during insertion, so the frame-owned
            // `String` borrow never escapes this table-function call.
            for (row, value) in values.iter().enumerate() {
                if let Some(value) = value {
                    vector.insert(row, value.as_str());
                }
            }
        }
    }
    Ok(())
}

/// Maps a DuckDB setup error into the engine's `data_access` channel.
/// Used only for connection/table-function failures (not bad SQL or bad rows),
/// so callers can tell “the adapter broke” apart from “the data was invalid”.
fn map_duckdb_error(error: duckdb::Error) -> MarketError {
    MarketError::data_access(format!("DuckDB frame-table registration failed: {error}"))
}

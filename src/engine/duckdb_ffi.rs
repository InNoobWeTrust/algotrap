//! Dynamically loaded DuckDB C API bindings and safe wrappers.

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use libloading::Library;
use once_cell::sync::Lazy;
#[cfg(test)]
use serde_json::{Map, Value};

use crate::engine::duckdb_engine::{ColumnBuffer, DuckDBComputedFrame};
use crate::engine::duckdb_ta_table_function::TaInvocation;
use crate::engine::error::MarketError;
#[cfg(test)]
use crate::engine::traits::ComputedFrame;

pub const DUCKDB_SUCCESS: DuckDbState = 0;

pub const DUCKDB_TYPE_BOOLEAN: DuckDbType = 1;
pub const DUCKDB_TYPE_TINYINT: DuckDbType = 2;
pub const DUCKDB_TYPE_SMALLINT: DuckDbType = 3;
pub const DUCKDB_TYPE_INTEGER: DuckDbType = 4;
pub const DUCKDB_TYPE_BIGINT: DuckDbType = 5;
pub const DUCKDB_TYPE_UTINYINT: DuckDbType = 6;
pub const DUCKDB_TYPE_USMALLINT: DuckDbType = 7;
pub const DUCKDB_TYPE_UINTEGER: DuckDbType = 8;
pub const DUCKDB_TYPE_UBIGINT: DuckDbType = 9;
pub const DUCKDB_TYPE_FLOAT: DuckDbType = 10;
pub const DUCKDB_TYPE_DOUBLE: DuckDbType = 11;
pub const DUCKDB_TYPE_DATE: DuckDbType = 13;
pub const DUCKDB_TYPE_VARCHAR: DuckDbType = 17;

#[allow(non_camel_case_types)]
pub type DuckDbState = libc::c_uint;
#[allow(non_camel_case_types)]
pub type DuckDbType = libc::c_uint;
#[allow(non_camel_case_types)]
pub type DuckDbIdx = u64;

#[repr(C)]
pub struct DuckDBDatabase {
    _private: [u8; 0],
}

#[repr(C)]
pub struct DuckDBConnection {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Debug)]
pub struct DuckDBColumn {
    deprecated_data: *mut libc::c_void,
    deprecated_nullmask: *mut bool,
    deprecated_type: DuckDbType,
    deprecated_name: *mut libc::c_char,
    internal_data: *mut libc::c_void,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DuckDBResult {
    deprecated_column_count: DuckDbIdx,
    deprecated_row_count: DuckDbIdx,
    deprecated_rows_changed: DuckDbIdx,
    deprecated_columns: *mut DuckDBColumn,
    deprecated_error_message: *mut libc::c_char,
    internal_data: *mut libc::c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DuckDBString {
    value: [u8; 16],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DuckDBDate {
    days: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DuckDBDateStruct {
    year: i32,
    month: i8,
    day: i8,
}

impl Default for DuckDBResult {
    fn default() -> Self {
        Self {
            deprecated_column_count: 0,
            deprecated_row_count: 0,
            deprecated_rows_changed: 0,
            deprecated_columns: ptr::null_mut(),
            deprecated_error_message: ptr::null_mut(),
            internal_data: ptr::null_mut(),
        }
    }
}

pub(crate) type DuckDbDatabaseHandle = *mut DuckDBDatabase;
pub(crate) type DuckDbConnectionHandle = *mut DuckDBConnection;

#[repr(C)]
pub(crate) struct DuckDBTableFunction {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct DuckDBBindInfo {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct DuckDBInitInfo {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct DuckDBFunctionInfo {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct DuckDBDataChunk {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct DuckDBVector {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct DuckDBLogicalType {
    _private: [u8; 0],
}
#[repr(C)]
#[cfg(test)]
pub(crate) struct DuckDBValue {
    _private: [u8; 0],
}
pub(crate) type DuckDbTableFunctionHandle = *mut DuckDBTableFunction;
pub(crate) type DuckDbBindInfoHandle = *mut DuckDBBindInfo;
pub(crate) type DuckDbInitInfoHandle = *mut DuckDBInitInfo;
pub(crate) type DuckDbFunctionInfoHandle = *mut DuckDBFunctionInfo;
pub(crate) type DuckDbDataChunkHandle = *mut DuckDBDataChunk;
pub(crate) type DuckDbVectorHandle = *mut DuckDBVector;
pub(crate) type DuckDbLogicalTypeHandle = *mut DuckDBLogicalType;
#[cfg(test)]
pub(crate) type DuckDbValueHandle = *mut DuckDBValue;
pub(crate) type DuckDbDeleteCallback = unsafe extern "C" fn(*mut libc::c_void);
pub(crate) type DuckDbTableFunctionBind = unsafe extern "C" fn(DuckDbBindInfoHandle);
pub(crate) type DuckDbTableFunctionInit = unsafe extern "C" fn(DuckDbInitInfoHandle);
pub(crate) type DuckDbTableFunction =
    unsafe extern "C" fn(DuckDbFunctionInfoHandle, DuckDbDataChunkHandle);

type DuckdbOpenFn =
    unsafe extern "C" fn(*const libc::c_char, *mut DuckDbDatabaseHandle) -> DuckDbState;
type DuckdbCloseFn = unsafe extern "C" fn(*mut DuckDbDatabaseHandle);
type DuckdbConnectFn =
    unsafe extern "C" fn(DuckDbDatabaseHandle, *mut DuckDbConnectionHandle) -> DuckDbState;
type DuckdbDisconnectFn = unsafe extern "C" fn(*mut DuckDbConnectionHandle);
type DuckdbQueryFn = unsafe extern "C" fn(
    DuckDbConnectionHandle,
    *const libc::c_char,
    *mut DuckDBResult,
) -> DuckDbState;
type DuckdbDestroyResultFn = unsafe extern "C" fn(*mut DuckDBResult);
type DuckdbResultErrorFn = unsafe extern "C" fn(*mut DuckDBResult) -> *const libc::c_char;
type DuckdbColumnCountFn = unsafe extern "C" fn(*mut DuckDBResult) -> DuckDbIdx;
type DuckdbColumnNameFn = unsafe extern "C" fn(*mut DuckDBResult, DuckDbIdx) -> *const libc::c_char;
type DuckdbColumnTypeFn = unsafe extern "C" fn(*mut DuckDBResult, DuckDbIdx) -> DuckDbType;
type DuckdbResultChunkCountFn = unsafe extern "C" fn(DuckDBResult) -> DuckDbIdx;
type DuckdbResultGetChunkFn =
    unsafe extern "C" fn(DuckDBResult, DuckDbIdx) -> DuckDbDataChunkHandle;
type DuckdbDestroyDataChunkFn = unsafe extern "C" fn(*mut DuckDbDataChunkHandle);
type DuckdbLibraryVersionFn = unsafe extern "C" fn() -> *const libc::c_char;
pub(crate) type DuckdbCreateTableFunctionFn = unsafe extern "C" fn() -> DuckDbTableFunctionHandle;
pub(crate) type DuckdbDestroyTableFunctionFn = unsafe extern "C" fn(*mut DuckDbTableFunctionHandle);
pub(crate) type DuckdbTableFunctionSetNameFn =
    unsafe extern "C" fn(DuckDbTableFunctionHandle, *const libc::c_char);
pub(crate) type DuckdbTableFunctionSetExtraInfoFn =
    unsafe extern "C" fn(DuckDbTableFunctionHandle, *mut libc::c_void, DuckDbDeleteCallback);
#[cfg(test)]
pub(crate) type DuckdbTableFunctionAddParameterFn =
    unsafe extern "C" fn(DuckDbTableFunctionHandle, DuckDbLogicalTypeHandle);
pub(crate) type DuckdbTableFunctionSetBindFn =
    unsafe extern "C" fn(DuckDbTableFunctionHandle, DuckDbTableFunctionBind);
pub(crate) type DuckdbTableFunctionSetInitFn =
    unsafe extern "C" fn(DuckDbTableFunctionHandle, DuckDbTableFunctionInit);
pub(crate) type DuckdbTableFunctionSetFunctionFn =
    unsafe extern "C" fn(DuckDbTableFunctionHandle, DuckDbTableFunction);
pub(crate) type DuckdbRegisterTableFunctionFn =
    unsafe extern "C" fn(DuckDbConnectionHandle, DuckDbTableFunctionHandle) -> DuckDbState;
pub(crate) type DuckdbBindGetParameterCountFn =
    unsafe extern "C" fn(DuckDbBindInfoHandle) -> DuckDbIdx;
#[cfg(test)]
pub(crate) type DuckdbBindGetParameterFn =
    unsafe extern "C" fn(DuckDbBindInfoHandle, DuckDbIdx) -> DuckDbValueHandle;
pub(crate) type DuckdbBindGetExtraInfoFn =
    unsafe extern "C" fn(DuckDbBindInfoHandle) -> *mut libc::c_void;
pub(crate) type DuckdbBindSetCardinalityFn =
    unsafe extern "C" fn(DuckDbBindInfoHandle, DuckDbIdx, bool);
pub(crate) type DuckdbBindAddResultColumnFn =
    unsafe extern "C" fn(DuckDbBindInfoHandle, *const libc::c_char, DuckDbLogicalTypeHandle);
pub(crate) type DuckdbBindSetBindDataFn =
    unsafe extern "C" fn(DuckDbBindInfoHandle, *mut libc::c_void, DuckDbDeleteCallback);
pub(crate) type DuckdbBindSetErrorFn =
    unsafe extern "C" fn(DuckDbBindInfoHandle, *const libc::c_char);
pub(crate) type DuckdbInitGetBindDataFn =
    unsafe extern "C" fn(DuckDbInitInfoHandle) -> *mut libc::c_void;
pub(crate) type DuckdbInitSetInitDataFn =
    unsafe extern "C" fn(DuckDbInitInfoHandle, *mut libc::c_void, DuckDbDeleteCallback);
pub(crate) type DuckdbInitSetErrorFn =
    unsafe extern "C" fn(DuckDbInitInfoHandle, *const libc::c_char);
pub(crate) type DuckdbFunctionGetBindDataFn =
    unsafe extern "C" fn(DuckDbFunctionInfoHandle) -> *mut libc::c_void;
pub(crate) type DuckdbFunctionGetInitDataFn =
    unsafe extern "C" fn(DuckDbFunctionInfoHandle) -> *mut libc::c_void;
pub(crate) type DuckdbFunctionSetErrorFn =
    unsafe extern "C" fn(DuckDbFunctionInfoHandle, *const libc::c_char);
#[cfg(test)]
pub(crate) type DuckdbGetListSizeFn = unsafe extern "C" fn(DuckDbValueHandle) -> DuckDbIdx;
#[cfg(test)]
pub(crate) type DuckdbGetListChildFn =
    unsafe extern "C" fn(DuckDbValueHandle, DuckDbIdx) -> DuckDbValueHandle;
#[cfg(test)]
pub(crate) type DuckdbGetDoubleFn = unsafe extern "C" fn(DuckDbValueHandle) -> f64;
#[cfg(test)]
pub(crate) type DuckdbGetInt64Fn = unsafe extern "C" fn(DuckDbValueHandle) -> i64;
#[cfg(test)]
pub(crate) type DuckdbIsNullValueFn = unsafe extern "C" fn(DuckDbValueHandle) -> bool;
#[cfg(test)]
pub(crate) type DuckdbDestroyValueFn = unsafe extern "C" fn(*mut DuckDbValueHandle);
pub(crate) type DuckdbCreateLogicalTypeFn =
    unsafe extern "C" fn(DuckDbType) -> DuckDbLogicalTypeHandle;
#[cfg(test)]
pub(crate) type DuckdbCreateListTypeFn =
    unsafe extern "C" fn(DuckDbLogicalTypeHandle) -> DuckDbLogicalTypeHandle;
pub(crate) type DuckdbDestroyLogicalTypeFn = unsafe extern "C" fn(*mut DuckDbLogicalTypeHandle);
pub(crate) type DuckdbDataChunkGetVectorFn =
    unsafe extern "C" fn(DuckDbDataChunkHandle, DuckDbIdx) -> DuckDbVectorHandle;
pub(crate) type DuckdbDataChunkGetSizeFn = unsafe extern "C" fn(DuckDbDataChunkHandle) -> DuckDbIdx;
pub(crate) type DuckdbDataChunkSetSizeFn = unsafe extern "C" fn(DuckDbDataChunkHandle, DuckDbIdx);
pub(crate) type DuckdbVectorSizeFn = unsafe extern "C" fn() -> DuckDbIdx;
pub(crate) type DuckdbVectorGetDataFn =
    unsafe extern "C" fn(DuckDbVectorHandle) -> *mut libc::c_void;
pub(crate) type DuckdbVectorEnsureValidityWritableFn = unsafe extern "C" fn(DuckDbVectorHandle);
pub(crate) type DuckdbVectorGetValidityFn = unsafe extern "C" fn(DuckDbVectorHandle) -> *mut u64;
type DuckdbStringLengthFn = unsafe extern "C" fn(DuckDBString) -> u32;
type DuckdbStringDataFn = unsafe extern "C" fn(*mut DuckDBString) -> *const libc::c_char;
type DuckdbFromDateFn = unsafe extern "C" fn(DuckDBDate) -> DuckDBDateStruct;

/// Dynamically loaded DuckDB C API.
pub struct DuckDbApi {
    _lib: Library,
    pub duckdb_open: DuckdbOpenFn,
    pub duckdb_close: DuckdbCloseFn,
    pub duckdb_connect: DuckdbConnectFn,
    pub duckdb_disconnect: DuckdbDisconnectFn,
    pub duckdb_query: DuckdbQueryFn,
    pub duckdb_destroy_result: DuckdbDestroyResultFn,
    pub duckdb_result_error: DuckdbResultErrorFn,
    pub duckdb_column_count: DuckdbColumnCountFn,
    pub duckdb_column_name: DuckdbColumnNameFn,
    pub duckdb_column_type: DuckdbColumnTypeFn,
    pub(crate) duckdb_result_chunk_count: DuckdbResultChunkCountFn,
    pub(crate) duckdb_result_get_chunk: DuckdbResultGetChunkFn,
    pub(crate) duckdb_destroy_data_chunk: DuckdbDestroyDataChunkFn,
    #[allow(
        dead_code,
        reason = "Retained DuckDB FFI binding is exercised by the runtime compatibility test."
    )]
    pub duckdb_library_version: DuckdbLibraryVersionFn,
    pub(crate) duckdb_create_table_function: DuckdbCreateTableFunctionFn,
    pub(crate) duckdb_destroy_table_function: DuckdbDestroyTableFunctionFn,
    pub(crate) duckdb_table_function_set_name: DuckdbTableFunctionSetNameFn,
    pub(crate) duckdb_table_function_set_extra_info: DuckdbTableFunctionSetExtraInfoFn,
    #[cfg(test)]
    pub(crate) duckdb_table_function_add_parameter: DuckdbTableFunctionAddParameterFn,
    pub(crate) duckdb_table_function_set_bind: DuckdbTableFunctionSetBindFn,
    pub(crate) duckdb_table_function_set_init: DuckdbTableFunctionSetInitFn,
    pub(crate) duckdb_table_function_set_function: DuckdbTableFunctionSetFunctionFn,
    pub(crate) duckdb_register_table_function: DuckdbRegisterTableFunctionFn,
    pub(crate) duckdb_bind_get_parameter_count: DuckdbBindGetParameterCountFn,
    #[cfg(test)]
    pub(crate) duckdb_bind_get_parameter: DuckdbBindGetParameterFn,
    pub(crate) duckdb_bind_get_extra_info: DuckdbBindGetExtraInfoFn,
    pub(crate) duckdb_bind_set_cardinality: DuckdbBindSetCardinalityFn,
    pub(crate) duckdb_bind_add_result_column: DuckdbBindAddResultColumnFn,
    pub(crate) duckdb_bind_set_bind_data: DuckdbBindSetBindDataFn,
    pub(crate) duckdb_bind_set_error: DuckdbBindSetErrorFn,
    pub(crate) duckdb_init_get_bind_data: DuckdbInitGetBindDataFn,
    pub(crate) duckdb_init_set_init_data: DuckdbInitSetInitDataFn,
    pub(crate) duckdb_init_set_error: DuckdbInitSetErrorFn,
    pub(crate) duckdb_function_get_bind_data: DuckdbFunctionGetBindDataFn,
    pub(crate) duckdb_function_get_init_data: DuckdbFunctionGetInitDataFn,
    pub(crate) duckdb_function_set_error: DuckdbFunctionSetErrorFn,
    #[cfg(test)]
    pub(crate) duckdb_get_list_size: DuckdbGetListSizeFn,
    #[cfg(test)]
    pub(crate) duckdb_get_list_child: DuckdbGetListChildFn,
    #[cfg(test)]
    pub(crate) duckdb_get_double: DuckdbGetDoubleFn,
    #[cfg(test)]
    pub(crate) duckdb_get_int64: DuckdbGetInt64Fn,
    #[cfg(test)]
    pub(crate) duckdb_is_null_value: DuckdbIsNullValueFn,
    #[cfg(test)]
    pub(crate) duckdb_destroy_value: DuckdbDestroyValueFn,
    pub(crate) duckdb_create_logical_type: DuckdbCreateLogicalTypeFn,
    #[cfg(test)]
    pub(crate) duckdb_create_list_type: DuckdbCreateListTypeFn,
    pub(crate) duckdb_destroy_logical_type: DuckdbDestroyLogicalTypeFn,
    pub(crate) duckdb_data_chunk_get_vector: DuckdbDataChunkGetVectorFn,
    pub(crate) duckdb_data_chunk_get_size: DuckdbDataChunkGetSizeFn,
    pub(crate) duckdb_data_chunk_set_size: DuckdbDataChunkSetSizeFn,
    pub(crate) duckdb_vector_size: DuckdbVectorSizeFn,
    pub(crate) duckdb_vector_get_data: DuckdbVectorGetDataFn,
    pub(crate) duckdb_vector_ensure_validity_writable: DuckdbVectorEnsureValidityWritableFn,
    pub(crate) duckdb_vector_get_validity: DuckdbVectorGetValidityFn,
    duckdb_string_t_length: DuckdbStringLengthFn,
    duckdb_string_t_data: DuckdbStringDataFn,
    duckdb_from_date: DuckdbFromDateFn,
}

static DUCKDB_API: Lazy<Result<Arc<DuckDbApi>, MarketError>> = Lazy::new(DuckDbApi::new);

/// Returns the lazily initialized, process-wide DuckDB API singleton.
pub(crate) fn duckdb_api() -> Result<Arc<DuckDbApi>, MarketError> {
    DUCKDB_API.as_ref().map(Arc::clone).map_err(Clone::clone)
}

struct DatabaseHandle {
    api: Arc<DuckDbApi>,
    raw: DuckDbDatabaseHandle,
}

impl Drop for DatabaseHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                (self.api.duckdb_close)(&mut self.raw);
            }
        }
    }
}

struct ConnectionHandle {
    api: Arc<DuckDbApi>,
    raw: DuckDbConnectionHandle,
}

impl Drop for ConnectionHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                (self.api.duckdb_disconnect)(&mut self.raw);
            }
        }
    }
}

/// A live in-memory DuckDB session: one database, one connection, and the
/// table-function registration surface used by the engine.
///
/// Both the database and the connection are owned here, so registered
/// table-function state is torn down exactly when the session is dropped.
pub(crate) struct DuckDbSession {
    _api: Arc<DuckDbApi>,
    database: Option<DatabaseHandle>,
    connection: Option<ConnectionHandle>,
    invocation_slot: Option<Arc<InvocationSlot>>,
    #[cfg(test)]
    registration_count: usize,
    #[cfg(test)]
    lifecycle_probe: Arc<TestLifecycleProbe>,
}

impl DuckDbSession {
    fn open(api: Arc<DuckDbApi>) -> Result<Self, MarketError> {
        Self::open_with_registration(api, true, None)
    }

    #[cfg(test)]
    fn open_literal(api: Arc<DuckDbApi>) -> Result<Self, MarketError> {
        Self::open_with_registration(api, false, None)
    }

    fn open_with_registration(
        api: Arc<DuckDbApi>,
        register_invocation_function: bool,
        extra_info_drop_counter: Option<Arc<std::sync::atomic::AtomicUsize>>,
    ) -> Result<Self, MarketError> {
        #[cfg(not(test))]
        let _ = extra_info_drop_counter;
        let database = api.open_database(None)?;
        let connection = api.connect(database.raw)?;
        #[cfg(test)]
        #[allow(
            clippy::arc_with_non_send_sync,
            reason = "the contract requires shared native extra-info ownership while RefCell keeps the slot !Sync and thread-local"
        )]
        let lifecycle_probe = Arc::new(TestLifecycleProbe::new(extra_info_drop_counter));
        #[allow(
            clippy::arc_with_non_send_sync,
            reason = "the contract requires shared native extra-info ownership while RefCell keeps the slot !Sync and thread-local"
        )]
        #[cfg(test)]
        let invocation_slot = Arc::new(InvocationSlot::new(Arc::clone(&lifecycle_probe)));
        #[allow(
            clippy::arc_with_non_send_sync,
            reason = "the contract requires shared native extra-info ownership while RefCell keeps the slot !Sync and thread-local"
        )]
        #[cfg(not(test))]
        let invocation_slot = Arc::new(InvocationSlot::new());
        if register_invocation_function {
            crate::engine::duckdb_ta_table_function::register(
                api.as_ref(),
                connection.raw,
                Arc::clone(&invocation_slot),
            )?;
            #[cfg(test)]
            lifecycle_probe.record_registration();
        }
        Ok(Self {
            _api: api,
            database: Some(database),
            connection: Some(connection),
            invocation_slot: Some(invocation_slot),
            #[cfg(test)]
            registration_count: usize::from(register_invocation_function),
            #[cfg(test)]
            lifecycle_probe,
        })
    }

    /// Opens a test session that records DuckDB's extra-info destructor callback.
    #[cfg(test)]
    fn open_with_extra_info_drop_counter(
        api: Arc<DuckDbApi>,
        counter: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Result<Self, MarketError> {
        Self::open_with_registration(api, true, Some(counter))
    }

    /// Installs one engine-owned job, runs the static query, and clears state on all exits.
    pub(crate) fn query_invocation_frame(
        &mut self,
        invocation: TaInvocation,
        query: &crate::engine::duckdb_engine::TrustedEngineQuery,
    ) -> Result<DuckDBComputedFrame, MarketError> {
        let _invocation_scope = self.invocation_slot().install(invocation)?;
        self.query_to_columnar_frame(query)
    }

    #[cfg(test)]
    pub(crate) fn register_ta_literal(&self) -> Result<(), MarketError> {
        crate::engine::duckdb_ta_table_function::register_literal(
            self.connection().api.as_ref(),
            self.connection().raw,
        )?;
        self.query_to_columnar_frame(
            &crate::engine::duckdb_engine::TrustedEngineQuery::from_test_sql(
                "SELECT * FROM ta_indicator_frame(
                    [1.0]::DOUBLE[], [1.0]::DOUBLE[], [1.0]::DOUBLE[], [1.0]::DOUBLE[], [1.0]::DOUBLE[],
                    [1]::BIGINT[], [NULL]::DOUBLE[],
                    1::BIGINT, 1::BIGINT, 1::BIGINT, 1::BIGINT, 1::BIGINT,
                    1::BIGINT, 1::BIGINT, 1::BIGINT, 1::BIGINT, 1::BIGINT
                ) LIMIT 0"
                    .into(),
            ),
        )?;
        Ok(())
    }

    /// Runs the query and decodes each DuckDB result chunk into owned columns.
    ///
    /// The returned frame contains no DuckDB allocation or pointer. Result and chunk
    /// guards release every native handle on normal return, error, or unwinding.
    pub(crate) fn query_to_columnar_frame(
        &self,
        query: &crate::engine::duckdb_engine::TrustedEngineQuery,
    ) -> Result<DuckDBComputedFrame, MarketError> {
        let sql = query.sql();
        validate_internal_query(sql)?;
        let connection = self.connection();
        let result = connection.api.run_query(connection.raw, sql)?;
        connection.api.decode_result_chunks(&result)
    }

    #[cfg(test)]
    pub(crate) fn registration_count(&self) -> usize {
        self.registration_count
    }

    #[cfg(test)]
    pub(crate) fn raw_connection(&self) -> DuckDbConnectionHandle {
        self.connection().raw
    }

    #[cfg(test)]
    pub(crate) fn has_pending_invocation(&self) -> bool {
        self.invocation_slot().has_pending()
    }

    /// Runs the query and materializes JSON records only for legacy test consumers.
    #[cfg(test)]
    pub(crate) fn query_to_json_records(
        &self,
        query: &crate::engine::duckdb_engine::TrustedEngineQuery,
    ) -> Result<Vec<Map<String, Value>>, MarketError> {
        self.query_to_columnar_frame(query)?.to_json_records()
    }

    /// Runs the query and returns the materialized result as JSON text.
    #[cfg(test)]
    pub(crate) fn query_to_json(
        &self,
        query: &crate::engine::duckdb_engine::TrustedEngineQuery,
    ) -> Result<String, MarketError> {
        serde_json::to_string(&self.query_to_json_records(query)?).map_err(|err| {
            MarketError::computation(format!("failed to serialize DuckDB result to JSON: {err}"))
        })
    }

    fn connection(&self) -> &ConnectionHandle {
        self.connection
            .as_ref()
            .expect("DuckDbSession connection is unavailable after teardown")
    }

    fn invocation_slot(&self) -> &Arc<InvocationSlot> {
        self.invocation_slot
            .as_ref()
            .expect("DuckDB session invocation slot is available until session drop")
    }
}

impl Drop for DuckDbSession {
    fn drop(&mut self) {
        // DuckDB requires every connection to be disconnected before its database closes.
        // Option::take makes this invariant independent of Rust field declaration order.
        #[cfg(test)]
        self.lifecycle_probe
            .record_phase(SessionDropPhase::SessionDropStarted);
        #[cfg(test)]
        self.lifecycle_probe
            .record_phase(SessionDropPhase::ConnectionDisconnectStarted);
        drop(self.connection.take());
        #[cfg(test)]
        self.lifecycle_probe
            .record_phase(SessionDropPhase::ConnectionDropped);
        drop(self.database.take());
        #[cfg(test)]
        self.lifecycle_probe
            .record_phase(SessionDropPhase::DatabaseDropped);
        // Release the session's independent Arc only after DuckDB has disconnected
        // and closed the database (which runs the C extra-info destructor).
        drop(self.invocation_slot.take());
        #[cfg(test)]
        self.lifecycle_probe
            .record_phase(SessionDropPhase::InvocationSlotReleased);
    }
}

/// Session-local handoff state retained by DuckDB table-function extra info.
///
/// `RefCell` intentionally makes this type `!Sync`; the enclosing session is
/// confined to a `thread_local!` slot and never crosses a thread boundary.
pub(crate) struct InvocationSlot {
    pending: RefCell<Option<TaInvocation>>,
    #[cfg(test)]
    lifecycle_probe: Arc<TestLifecycleProbe>,
}

impl InvocationSlot {
    #[cfg(test)]
    fn new(lifecycle_probe: Arc<TestLifecycleProbe>) -> Self {
        Self {
            pending: RefCell::new(None),
            lifecycle_probe,
        }
    }

    #[cfg(not(test))]
    fn new() -> Self {
        Self {
            pending: RefCell::new(None),
        }
    }

    fn install(&self, invocation: TaInvocation) -> Result<InvocationScope<'_>, MarketError> {
        let mut pending = self.pending.borrow_mut();
        if pending.is_some() {
            return Err(MarketError::invocation_lifecycle(
                "ta_indicator_frame already has a pending invocation",
            ));
        }
        *pending = Some(invocation);
        Ok(InvocationScope { slot: self })
    }

    pub(crate) fn take_for_bind(&self) -> Result<TaInvocation, String> {
        self.pending
            .borrow_mut()
            .take()
            .ok_or_else(|| "ta_indicator_frame is missing its engine invocation".into())
    }

    #[cfg(test)]
    fn has_pending(&self) -> bool {
        self.pending.borrow().is_some()
    }

    pub(crate) fn record_extra_info_destructor(&self) {
        #[cfg(test)]
        self.lifecycle_probe.record_extra_info_destructor();
    }
}

impl Default for InvocationSlot {
    fn default() -> Self {
        #[cfg(test)]
        return Self::new(Arc::new(TestLifecycleProbe::new(None)));
        #[cfg(not(test))]
        Self::new()
    }
}

struct InvocationScope<'a> {
    slot: &'a InvocationSlot,
}

impl Drop for InvocationScope<'_> {
    fn drop(&mut self) {
        self.slot.pending.borrow_mut().take();
    }
}

thread_local! {
    // WHY: DuckDB connections are thread-confined; this is the sole owner of each
    // reusable connection and its one static table-function registration. A session
    // exists only while an explicit scope on this thread owns it.
    static THREAD_SESSION: RefCell<ThreadSessionSlot> = const { RefCell::new(ThreadSessionSlot::new()) };
}

/// Cross-thread test evidence for scopes that finish inside short-lived workers.
#[cfg(test)]
static COMPLETED_THREAD_SESSION_LIFECYCLES: Lazy<std::sync::Mutex<Vec<ThreadSessionLifecycle>>> =
    Lazy::new(|| std::sync::Mutex::new(Vec::new()));

struct ThreadSessionSlot {
    session: Option<DuckDbSession>,
    scope_depth: usize,
    #[cfg(test)]
    completed_lifecycles: Vec<ThreadSessionLifecycle>,
}

impl ThreadSessionSlot {
    const fn new() -> Self {
        Self {
            session: None,
            scope_depth: 0,
            #[cfg(test)]
            completed_lifecycles: Vec::new(),
        }
    }
}

/// Owns the current thread's DuckDB session for one nestable execution scope.
///
/// The outermost guard destroys the session when it is dropped, including during
/// unwinding. Nested guards only reduce the depth, so work within one outer scope
/// reuses one connection and one table-function registration.
struct ThreadSessionScopeGuard;

impl Drop for ThreadSessionScopeGuard {
    fn drop(&mut self) {
        THREAD_SESSION.with(|thread_session| {
            let mut slot = thread_session.borrow_mut();
            assert!(slot.scope_depth > 0, "thread session scope depth underflow");
            slot.scope_depth -= 1;
            if slot.scope_depth != 0 {
                return;
            }

            let session = slot.session.take();
            #[cfg(test)]
            if let Some(session) = session {
                let had_pending_invocation = session.has_pending_invocation();
                assert!(
                    !had_pending_invocation,
                    "thread session scope exited with a pending invocation"
                );
                let lifecycle_probe = Arc::clone(&session.lifecycle_probe);
                drop(session);
                slot.completed_lifecycles
                    .push(lifecycle_probe.snapshot(had_pending_invocation));
            }
            #[cfg(not(test))]
            drop(session);
        });
    }
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ThreadSessionState {
    pub(crate) raw_connection: Option<usize>,
    pub(crate) registration_count: usize,
    pub(crate) has_pending_invocation: bool,
    pub(crate) scope_depth: usize,
}

/// Test-only evidence captured for one thread-local DuckDB session registration.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThreadSessionLifecycle {
    pub(crate) registration_count: usize,
    pub(crate) extra_info_destructor_count: usize,
    pub(crate) had_pending_invocation: bool,
    phases: Vec<SessionDropPhase>,
}

#[cfg(test)]
impl ThreadSessionLifecycle {
    pub(crate) fn assert_explicit_clear_completed(&self) {
        assert!(
            !self.had_pending_invocation,
            "thread-local session retained a pending invocation"
        );
        assert_eq!(
            self.extra_info_destructor_count, self.registration_count,
            "every registration must receive exactly one C extra-info destructor callback"
        );
        if self.registration_count != 0 {
            assert_eq!(
                self.phases,
                vec![
                    SessionDropPhase::SessionDropStarted,
                    SessionDropPhase::ConnectionDisconnectStarted,
                    SessionDropPhase::ConnectionDropped,
                    SessionDropPhase::ExtraInfoDestroyed,
                    SessionDropPhase::DatabaseDropped,
                    SessionDropPhase::InvocationSlotReleased,
                ],
                "C extra-info destruction must occur after connection teardown and before session teardown completes"
            );
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionDropPhase {
    SessionDropStarted,
    ConnectionDisconnectStarted,
    ExtraInfoDestroyed,
    ConnectionDropped,
    DatabaseDropped,
    InvocationSlotReleased,
}

#[cfg(test)]
struct TestLifecycleProbe {
    registration_count: std::sync::atomic::AtomicUsize,
    extra_info_destructor_count: std::sync::atomic::AtomicUsize,
    phases: std::sync::Mutex<Vec<SessionDropPhase>>,
    extra_info_drop_counter: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

#[cfg(test)]
impl TestLifecycleProbe {
    fn new(extra_info_drop_counter: Option<Arc<std::sync::atomic::AtomicUsize>>) -> Self {
        Self {
            registration_count: std::sync::atomic::AtomicUsize::new(0),
            extra_info_destructor_count: std::sync::atomic::AtomicUsize::new(0),
            phases: std::sync::Mutex::new(Vec::new()),
            extra_info_drop_counter,
        }
    }

    fn record_registration(&self) {
        self.registration_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn record_extra_info_destructor(&self) {
        self.extra_info_destructor_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(counter) = &self.extra_info_drop_counter {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        self.record_phase(SessionDropPhase::ExtraInfoDestroyed);
    }

    fn record_phase(&self, phase: SessionDropPhase) {
        self.phases.lock().unwrap().push(phase);
    }

    fn snapshot(&self, had_pending_invocation: bool) -> ThreadSessionLifecycle {
        ThreadSessionLifecycle {
            registration_count: self
                .registration_count
                .load(std::sync::atomic::Ordering::SeqCst),
            extra_info_destructor_count: self
                .extra_info_destructor_count
                .load(std::sync::atomic::Ordering::SeqCst),
            had_pending_invocation,
            phases: self.phases.lock().unwrap().clone(),
        }
    }
}

/// Runs a closure inside an explicit, nestable thread-local DuckDB session scope.
///
/// This is deliberately restricted to the engine module. Public engine calls create
/// their own scope, while future batch workers can hold one outer scope to reuse the
/// connection and static UDF registration across related invocations.
pub(super) fn with_thread_session_scope<T>(
    operation: impl FnOnce() -> Result<T, MarketError>,
) -> Result<T, MarketError> {
    THREAD_SESSION.with(|thread_session| {
        thread_session.borrow_mut().scope_depth += 1;
    });
    let _scope_guard = ThreadSessionScopeGuard;
    operation()
}

/// Runs a closure with the current scope's exclusive reusable DuckDB session.
///
/// The closure prevents the session borrow from escaping thread-local storage.
pub(crate) fn with_thread_session<T>(
    operation: impl FnOnce(&mut DuckDbSession) -> Result<T, MarketError>,
) -> Result<T, MarketError> {
    THREAD_SESSION.with(|thread_session| {
        let mut slot = thread_session.try_borrow_mut().map_err(|_| {
            MarketError::thread_safety("reentrant access to the thread-local DuckDB session")
        })?;
        if slot.scope_depth == 0 {
            return Err(MarketError::thread_safety(
                "DuckDB session access requires an explicit session scope",
            ));
        }
        if slot.session.is_none() {
            slot.session = Some(DuckDbSession::open(duckdb_api()?)?);
        }
        operation(
            slot.session
                .as_mut()
                .expect("thread-local DuckDB session initialized"),
        )
    })
}

#[cfg(test)]
pub(crate) fn thread_session_state() -> Result<ThreadSessionState, MarketError> {
    THREAD_SESSION.with(|thread_session| {
        let slot = thread_session.try_borrow().map_err(|_| {
            MarketError::thread_safety("reentrant access to the thread-local DuckDB session")
        })?;
        Ok(ThreadSessionState {
            raw_connection: slot
                .session
                .as_ref()
                .map(|session| session.raw_connection() as usize),
            registration_count: slot
                .session
                .as_ref()
                .map_or(0, DuckDbSession::registration_count),
            has_pending_invocation: slot
                .session
                .as_ref()
                .is_some_and(DuckDbSession::has_pending_invocation),
            scope_depth: slot.scope_depth,
        })
    })
}

#[cfg(test)]
pub(crate) fn clear_thread_session() -> Result<ThreadSessionLifecycle, MarketError> {
    THREAD_SESSION.with(|thread_session| {
        let mut slot = thread_session.try_borrow_mut().map_err(|_| {
            MarketError::thread_safety("reentrant access to the thread-local DuckDB session")
        })?;
        assert_eq!(
            slot.scope_depth, 0,
            "cannot force-clear an active session scope"
        );
        let lifecycle = slot.session.take().map_or_else(
            || ThreadSessionLifecycle {
                registration_count: 0,
                extra_info_destructor_count: 0,
                had_pending_invocation: false,
                phases: Vec::new(),
            },
            |session| {
                let had_pending_invocation = session.has_pending_invocation();
                let lifecycle_probe = Arc::clone(&session.lifecycle_probe);
                drop(session);
                lifecycle_probe.snapshot(had_pending_invocation)
            },
        );
        Ok(lifecycle)
    })
}

#[cfg(test)]
pub(crate) fn take_completed_thread_session_lifecycles()
-> Result<Vec<ThreadSessionLifecycle>, MarketError> {
    THREAD_SESSION.with(|thread_session| {
        let mut slot = thread_session.try_borrow_mut().map_err(|_| {
            MarketError::thread_safety("reentrant access to the thread-local DuckDB session")
        })?;
        Ok(std::mem::take(&mut slot.completed_lifecycles))
    })
}

/// Drains test lifecycle evidence collected by all scoped worker threads.
#[cfg(test)]
pub(crate) fn take_all_completed_thread_session_lifecycles() -> Vec<ThreadSessionLifecycle> {
    std::mem::take(
        &mut *COMPLETED_THREAD_SESSION_LIFECYCLES
            .lock()
            .expect("completed thread-session lifecycle mutex poisoned"),
    )
}

/// Publishes lifecycle evidence drained from one finished batch worker.
#[cfg(test)]
pub(crate) fn record_completed_thread_session_lifecycles(lifecycles: Vec<ThreadSessionLifecycle>) {
    COMPLETED_THREAD_SESSION_LIFECYCLES
        .lock()
        .expect("completed thread-session lifecycle mutex poisoned")
        .extend(lifecycles);
}

struct QueryResult<'a> {
    api: &'a DuckDbApi,
    raw: DuckDBResult,
}

impl QueryResult<'_> {
    fn as_mut_ptr(&self) -> *mut DuckDBResult {
        &self.raw as *const DuckDBResult as *mut DuckDBResult
    }
}

impl Drop for QueryResult<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.duckdb_destroy_result)(&mut self.raw);
        }
    }
}

struct DataChunkHandle<'a> {
    api: &'a DuckDbApi,
    raw: DuckDbDataChunkHandle,
}

impl Drop for DataChunkHandle<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                (self.api.duckdb_destroy_data_chunk)(&mut self.raw);
            }
        }
    }
}

impl DuckDbApi {
    pub fn new() -> Result<Arc<Self>, MarketError> {
        let lib = Self::load_library()?;

        unsafe {
            Ok(Arc::new(Self {
                duckdb_open: Self::load_symbol(&lib, b"duckdb_open\0")?,
                duckdb_close: Self::load_symbol(&lib, b"duckdb_close\0")?,
                duckdb_connect: Self::load_symbol(&lib, b"duckdb_connect\0")?,
                duckdb_disconnect: Self::load_symbol(&lib, b"duckdb_disconnect\0")?,
                duckdb_query: Self::load_symbol(&lib, b"duckdb_query\0")?,
                duckdb_destroy_result: Self::load_symbol(&lib, b"duckdb_destroy_result\0")?,
                duckdb_result_error: Self::load_symbol(&lib, b"duckdb_result_error\0")?,
                duckdb_column_count: Self::load_symbol(&lib, b"duckdb_column_count\0")?,
                duckdb_column_name: Self::load_symbol(&lib, b"duckdb_column_name\0")?,
                duckdb_column_type: Self::load_symbol(&lib, b"duckdb_column_type\0")?,
                duckdb_result_chunk_count: Self::load_symbol(&lib, b"duckdb_result_chunk_count\0")?,
                duckdb_result_get_chunk: Self::load_symbol(&lib, b"duckdb_result_get_chunk\0")?,
                duckdb_destroy_data_chunk: Self::load_symbol(&lib, b"duckdb_destroy_data_chunk\0")?,
                duckdb_library_version: Self::load_symbol(&lib, b"duckdb_library_version\0")?,
                duckdb_create_table_function: Self::load_symbol(
                    &lib,
                    b"duckdb_create_table_function\0",
                )?,
                duckdb_destroy_table_function: Self::load_symbol(
                    &lib,
                    b"duckdb_destroy_table_function\0",
                )?,
                duckdb_table_function_set_name: Self::load_symbol(
                    &lib,
                    b"duckdb_table_function_set_name\0",
                )?,
                duckdb_table_function_set_extra_info: Self::load_symbol(
                    &lib,
                    b"duckdb_table_function_set_extra_info\0",
                )?,
                #[cfg(test)]
                duckdb_table_function_add_parameter: Self::load_symbol(
                    &lib,
                    b"duckdb_table_function_add_parameter\0",
                )?,
                duckdb_table_function_set_bind: Self::load_symbol(
                    &lib,
                    b"duckdb_table_function_set_bind\0",
                )?,
                duckdb_table_function_set_init: Self::load_symbol(
                    &lib,
                    b"duckdb_table_function_set_init\0",
                )?,
                duckdb_table_function_set_function: Self::load_symbol(
                    &lib,
                    b"duckdb_table_function_set_function\0",
                )?,
                duckdb_register_table_function: Self::load_symbol(
                    &lib,
                    b"duckdb_register_table_function\0",
                )?,
                duckdb_bind_get_parameter_count: Self::load_symbol(
                    &lib,
                    b"duckdb_bind_get_parameter_count\0",
                )?,
                #[cfg(test)]
                duckdb_bind_get_parameter: Self::load_symbol(&lib, b"duckdb_bind_get_parameter\0")?,
                duckdb_bind_get_extra_info: Self::load_symbol(
                    &lib,
                    b"duckdb_bind_get_extra_info\0",
                )?,
                duckdb_bind_set_cardinality: Self::load_symbol(
                    &lib,
                    b"duckdb_bind_set_cardinality\0",
                )?,
                duckdb_bind_add_result_column: Self::load_symbol(
                    &lib,
                    b"duckdb_bind_add_result_column\0",
                )?,
                duckdb_bind_set_bind_data: Self::load_symbol(&lib, b"duckdb_bind_set_bind_data\0")?,
                duckdb_bind_set_error: Self::load_symbol(&lib, b"duckdb_bind_set_error\0")?,
                duckdb_init_get_bind_data: Self::load_symbol(&lib, b"duckdb_init_get_bind_data\0")?,
                duckdb_init_set_init_data: Self::load_symbol(&lib, b"duckdb_init_set_init_data\0")?,
                duckdb_init_set_error: Self::load_symbol(&lib, b"duckdb_init_set_error\0")?,
                duckdb_function_get_bind_data: Self::load_symbol(
                    &lib,
                    b"duckdb_function_get_bind_data\0",
                )?,
                duckdb_function_get_init_data: Self::load_symbol(
                    &lib,
                    b"duckdb_function_get_init_data\0",
                )?,
                duckdb_function_set_error: Self::load_symbol(&lib, b"duckdb_function_set_error\0")?,
                #[cfg(test)]
                duckdb_get_list_size: Self::load_symbol(&lib, b"duckdb_get_list_size\0")?,
                #[cfg(test)]
                duckdb_get_list_child: Self::load_symbol(&lib, b"duckdb_get_list_child\0")?,
                #[cfg(test)]
                duckdb_get_double: Self::load_symbol(&lib, b"duckdb_get_double\0")?,
                #[cfg(test)]
                duckdb_get_int64: Self::load_symbol(&lib, b"duckdb_get_int64\0")?,
                #[cfg(test)]
                duckdb_is_null_value: Self::load_symbol(&lib, b"duckdb_is_null_value\0")?,
                #[cfg(test)]
                duckdb_destroy_value: Self::load_symbol(&lib, b"duckdb_destroy_value\0")?,
                duckdb_create_logical_type: Self::load_symbol(
                    &lib,
                    b"duckdb_create_logical_type\0",
                )?,
                #[cfg(test)]
                duckdb_create_list_type: Self::load_symbol(&lib, b"duckdb_create_list_type\0")?,
                duckdb_destroy_logical_type: Self::load_symbol(
                    &lib,
                    b"duckdb_destroy_logical_type\0",
                )?,
                duckdb_data_chunk_get_vector: Self::load_symbol(
                    &lib,
                    b"duckdb_data_chunk_get_vector\0",
                )?,
                duckdb_data_chunk_get_size: Self::load_symbol(
                    &lib,
                    b"duckdb_data_chunk_get_size\0",
                )?,
                duckdb_data_chunk_set_size: Self::load_symbol(
                    &lib,
                    b"duckdb_data_chunk_set_size\0",
                )?,
                duckdb_vector_size: Self::load_symbol(&lib, b"duckdb_vector_size\0")?,
                duckdb_vector_get_data: Self::load_symbol(&lib, b"duckdb_vector_get_data\0")?,
                duckdb_vector_ensure_validity_writable: Self::load_symbol(
                    &lib,
                    b"duckdb_vector_ensure_validity_writable\0",
                )?,
                duckdb_vector_get_validity: Self::load_symbol(
                    &lib,
                    b"duckdb_vector_get_validity\0",
                )?,
                duckdb_string_t_length: Self::load_symbol(&lib, b"duckdb_string_t_length\0")?,
                duckdb_string_t_data: Self::load_symbol(&lib, b"duckdb_string_t_data\0")?,
                duckdb_from_date: Self::load_symbol(&lib, b"duckdb_from_date\0")?,
                _lib: lib,
            }))
        }
    }

    #[allow(
        dead_code,
        reason = "Retained for the DuckDB runtime compatibility test."
    )]
    pub(crate) fn library_version(&self) -> String {
        let ptr = unsafe { (self.duckdb_library_version)() };
        c_str_or_default(ptr, "unknown DuckDB version")
    }

    /// Executes an opaque query issued exclusively by the engine's static query builders.
    ///
    /// The trusted-query type is the authorization boundary; SQL validation is defense in depth.
    ///
    /// Test-only: this opens a dedicated in-memory session, registers the retained
    /// literal-array table function baseline, and decodes the result into JSON.
    #[cfg(test)]
    pub(crate) fn query_to_json(
        &self,
        query: &crate::engine::duckdb_engine::TrustedEngineQuery,
    ) -> Result<String, MarketError> {
        let session = duckdb_api()?.open_literal_session()?;
        session.register_ta_literal()?;
        session.query_to_json(query)
    }

    /// Opens a dedicated in-memory DuckDB session bound to `self`.
    #[cfg(test)]
    pub(crate) fn open_session(self: &Arc<Self>) -> Result<DuckDbSession, MarketError> {
        DuckDbSession::open(Arc::clone(self))
    }

    #[cfg(test)]
    pub(crate) fn open_literal_session(self: &Arc<Self>) -> Result<DuckDbSession, MarketError> {
        DuckDbSession::open_literal(Arc::clone(self))
    }

    fn load_library() -> Result<Library, MarketError> {
        let mut errors = Vec::new();

        for candidate in library_candidates()? {
            let loaded = unsafe { Library::new(candidate.as_str()) };
            match loaded {
                Ok(lib) => return Ok(lib),
                Err(err) => errors.push(format!("{} ({err})", candidate)),
            }
        }

        Err(MarketError::data_access(format!(
            "failed to load DuckDB shared library; tried: {}",
            errors.join(", ")
        )))
    }

    unsafe fn load_symbol<T: Copy>(lib: &Library, symbol: &[u8]) -> Result<T, MarketError> {
        unsafe { lib.get::<T>(symbol) }
            .map(|loaded| *loaded)
            .map_err(|err| {
                MarketError::data_access(format!(
                    "failed to load DuckDB symbol {}: {err}",
                    String::from_utf8_lossy(symbol).trim_end_matches('\0')
                ))
            })
    }

    fn open_database(
        self: &Arc<Self>,
        database_path: Option<&str>,
    ) -> Result<DatabaseHandle, MarketError> {
        let path_cstring = database_path
            .filter(|path| !path.trim().is_empty())
            .map(|path| into_c_string(path, "database path"))
            .transpose()?;
        let path_ptr = path_cstring
            .as_ref()
            .map_or(ptr::null(), |path| path.as_ptr());
        let mut raw_database: DuckDbDatabaseHandle = ptr::null_mut();

        let state = unsafe { (self.duckdb_open)(path_ptr, &mut raw_database) };
        if state != DUCKDB_SUCCESS || raw_database.is_null() {
            return Err(MarketError::data_access(format!(
                "failed to open DuckDB database at {}",
                database_path.unwrap_or(":memory:")
            )));
        }

        Ok(DatabaseHandle {
            api: Arc::clone(self),
            raw: raw_database,
        })
    }

    fn connect(
        self: &Arc<Self>,
        database: DuckDbDatabaseHandle,
    ) -> Result<ConnectionHandle, MarketError> {
        let mut raw_connection: DuckDbConnectionHandle = ptr::null_mut();
        let state = unsafe { (self.duckdb_connect)(database, &mut raw_connection) };

        if state != DUCKDB_SUCCESS || raw_connection.is_null() {
            return Err(MarketError::data_access("failed to open DuckDB connection"));
        }

        Ok(ConnectionHandle {
            api: Arc::clone(self),
            raw: raw_connection,
        })
    }

    fn run_query(
        &self,
        connection: DuckDbConnectionHandle,
        sql: &str,
    ) -> Result<QueryResult<'_>, MarketError> {
        let sql = into_c_string(sql, "SQL query")?;
        let mut raw_result = DuckDBResult::default();
        let state = unsafe { (self.duckdb_query)(connection, sql.as_ptr(), &mut raw_result) };
        let result = QueryResult {
            api: self,
            raw: raw_result,
        };

        if state != DUCKDB_SUCCESS {
            return Err(MarketError::data_access(self.query_error(&result)));
        }

        Ok(result)
    }

    fn query_error(&self, result: &QueryResult<'_>) -> String {
        let error = unsafe { (self.duckdb_result_error)(result.as_mut_ptr()) };
        c_str_or_default(error, "DuckDB query failed without an error message")
    }

    fn decode_result_chunks(
        &self,
        result: &QueryResult<'_>,
    ) -> Result<DuckDBComputedFrame, MarketError> {
        let column_count = self.result_column_count(result)?;
        let schemas = self.result_schemas(result, column_count)?;
        let mut buffers = schemas
            .iter()
            .map(|(name, value_type)| {
                empty_column_buffer(*value_type).map_err(|error| {
                    error.with_context(format!("column {name}, DuckDB type {value_type}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let chunk_count = unsafe { (self.duckdb_result_chunk_count)(result.raw) };

        for chunk_index in 0..chunk_count {
            let raw_chunk = unsafe { (self.duckdb_result_get_chunk)(result.raw, chunk_index) };
            if raw_chunk.is_null() {
                return Err(MarketError::computation(format!(
                    "DuckDB returned a null result chunk at chunk {chunk_index}"
                )));
            }
            let chunk = DataChunkHandle {
                api: self,
                raw: raw_chunk,
            };
            let row_count =
                usize::try_from(unsafe { (self.duckdb_data_chunk_get_size)(chunk.raw) }).map_err(
                    |_| {
                        MarketError::computation(format!(
                            "DuckDB chunk {chunk_index} row count does not fit into usize"
                        ))
                    },
                )?;
            for (column_index, ((name, value_type), buffer)) in
                schemas.iter().zip(&mut buffers).enumerate()
            {
                self.decode_chunk_column(
                    &chunk,
                    chunk_index,
                    column_index,
                    name,
                    *value_type,
                    row_count,
                    buffer,
                )?;
            }
        }

        DuckDBComputedFrame::from_column_buffers(
            schemas.into_iter().map(|(name, _)| name).collect(),
            buffers,
        )
    }

    fn result_schemas(
        &self,
        result: &QueryResult<'_>,
        column_count: usize,
    ) -> Result<Vec<(String, DuckDbType)>, MarketError> {
        (0..column_count)
            .map(|column_index| {
                let column = DuckDbIdx::try_from(column_index).map_err(|_| {
                    MarketError::computation(format!(
                        "DuckDB column index {column_index} does not fit into idx_t"
                    ))
                })?;
                let name = self.result_column_name(result, column)?;
                let value_type = unsafe { (self.duckdb_column_type)(result.as_mut_ptr(), column) };
                Ok((name, value_type))
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_chunk_column(
        &self,
        chunk: &DataChunkHandle<'_>,
        chunk_index: DuckDbIdx,
        column_index: usize,
        column_name: &str,
        value_type: DuckDbType,
        row_count: usize,
        buffer: &mut ColumnBuffer,
    ) -> Result<(), MarketError> {
        let vector = unsafe {
            (self.duckdb_data_chunk_get_vector)(
                chunk.raw,
                DuckDbIdx::try_from(column_index).map_err(|_| {
                    MarketError::computation(format!(
                        "DuckDB chunk {chunk_index}, column {column_name}: index does not fit idx_t"
                    ))
                })?,
            )
        };
        if vector.is_null() {
            return Err(MarketError::computation(format!(
                "DuckDB returned a null vector at chunk {chunk_index}, column {column_index} ({column_name})"
            )));
        }
        let validity = unsafe { (self.duckdb_vector_get_validity)(vector) };
        let data = unsafe { (self.duckdb_vector_get_data)(vector) };
        if row_count > 0 && data.is_null() {
            return Err(MarketError::computation(format!(
                "DuckDB returned null data at chunk {chunk_index}, column {column_index} ({column_name})"
            )));
        }
        match (value_type, buffer) {
            (DUCKDB_TYPE_BOOLEAN, ColumnBuffer::Boolean(values)) => {
                append_vector(values, data.cast::<bool>(), validity, row_count)
            }
            (DUCKDB_TYPE_TINYINT, ColumnBuffer::Int64(values)) => {
                append_cast_vector(values, data.cast::<i8>(), validity, row_count)
            }
            (DUCKDB_TYPE_SMALLINT, ColumnBuffer::Int64(values)) => {
                append_cast_vector(values, data.cast::<i16>(), validity, row_count)
            }
            (DUCKDB_TYPE_INTEGER, ColumnBuffer::Int64(values)) => {
                append_cast_vector(values, data.cast::<i32>(), validity, row_count)
            }
            (DUCKDB_TYPE_BIGINT, ColumnBuffer::Int64(values)) => {
                append_vector(values, data.cast::<i64>(), validity, row_count)
            }
            (DUCKDB_TYPE_UTINYINT, ColumnBuffer::UInt64(values)) => {
                append_cast_vector(values, data.cast::<u8>(), validity, row_count)
            }
            (DUCKDB_TYPE_USMALLINT, ColumnBuffer::UInt64(values)) => {
                append_cast_vector(values, data.cast::<u16>(), validity, row_count)
            }
            (DUCKDB_TYPE_UINTEGER, ColumnBuffer::UInt64(values)) => {
                append_cast_vector(values, data.cast::<u32>(), validity, row_count)
            }
            (DUCKDB_TYPE_UBIGINT, ColumnBuffer::UInt64(values)) => {
                append_vector(values, data.cast::<u64>(), validity, row_count)
            }
            (DUCKDB_TYPE_FLOAT, ColumnBuffer::Float64(values)) => append_float_vector(
                values,
                data.cast::<f32>(),
                validity,
                row_count,
                chunk_index,
                column_name,
            )?,
            (DUCKDB_TYPE_DOUBLE, ColumnBuffer::Float64(values)) => append_float_vector(
                values,
                data.cast::<f64>(),
                validity,
                row_count,
                chunk_index,
                column_name,
            )?,
            (DUCKDB_TYPE_VARCHAR, ColumnBuffer::Utf8(values)) => self.append_string_vector(
                values,
                data.cast(),
                validity,
                row_count,
                chunk_index,
                column_name,
            )?,
            (DUCKDB_TYPE_DATE, ColumnBuffer::Utf8(values)) => {
                self.append_date_vector(values, data.cast(), validity, row_count)?
            }
            _ => {
                return Err(MarketError::computation(format!(
                    "DuckDB chunk {chunk_index}, column {column_index} ({column_name}) has an unsupported or mismatched type {value_type}"
                )));
            }
        }
        Ok(())
    }

    fn append_string_vector(
        &self,
        values: &mut Vec<Option<String>>,
        data: *mut DuckDBString,
        validity: *mut u64,
        row_count: usize,
        chunk_index: DuckDbIdx,
        column_name: &str,
    ) -> Result<(), MarketError> {
        if row_count == 0 {
            return Ok(());
        }
        let source = unsafe { std::slice::from_raw_parts_mut(data, row_count) };
        for (row, value) in source.iter_mut().enumerate() {
            if !is_valid(validity, row) {
                values.push(None);
                continue;
            }
            let length = unsafe { (self.duckdb_string_t_length)(*value) } as usize;
            let pointer = unsafe { (self.duckdb_string_t_data)(value) };
            if pointer.is_null() {
                return Err(MarketError::computation(format!(
                    "DuckDB returned null string data at chunk {chunk_index}, column {column_name}, row {row}"
                )));
            }
            let text = String::from_utf8_lossy(unsafe {
                std::slice::from_raw_parts(pointer.cast::<u8>(), length)
            })
            .into_owned();
            values.push(Some(text));
        }
        Ok(())
    }

    fn append_date_vector(
        &self,
        values: &mut Vec<Option<String>>,
        data: *const DuckDBDate,
        validity: *mut u64,
        row_count: usize,
    ) -> Result<(), MarketError> {
        if row_count == 0 {
            return Ok(());
        }
        let source = unsafe { std::slice::from_raw_parts(data, row_count) };
        for (row, value) in source.iter().enumerate() {
            if !is_valid(validity, row) {
                values.push(None);
                continue;
            }
            let date = unsafe { (self.duckdb_from_date)(*value) };
            values.push(Some(format!(
                "{:04}-{:02}-{:02}",
                date.year, date.month, date.day
            )));
        }
        Ok(())
    }

    fn result_column_count(&self, result: &QueryResult<'_>) -> Result<usize, MarketError> {
        let count = unsafe { (self.duckdb_column_count)(result.as_mut_ptr()) };
        usize::try_from(count).map_err(|_| {
            MarketError::computation(format!(
                "DuckDB column count does not fit into usize: {count}"
            ))
        })
    }

    fn result_column_name(
        &self,
        result: &QueryResult<'_>,
        column: DuckDbIdx,
    ) -> Result<String, MarketError> {
        let raw_name = unsafe { (self.duckdb_column_name)(result.as_mut_ptr(), column) };
        if raw_name.is_null() {
            return Err(MarketError::computation(format!(
                "DuckDB returned a null column name for column index {column}"
            )));
        }
        Ok(unsafe { CStr::from_ptr(raw_name) }
            .to_string_lossy()
            .into_owned())
    }
}

fn empty_column_buffer(value_type: DuckDbType) -> Result<ColumnBuffer, MarketError> {
    match value_type {
        DUCKDB_TYPE_BOOLEAN => Ok(ColumnBuffer::Boolean(Vec::new())),
        DUCKDB_TYPE_TINYINT | DUCKDB_TYPE_SMALLINT | DUCKDB_TYPE_INTEGER | DUCKDB_TYPE_BIGINT => {
            Ok(ColumnBuffer::Int64(Vec::new()))
        }
        DUCKDB_TYPE_UTINYINT
        | DUCKDB_TYPE_USMALLINT
        | DUCKDB_TYPE_UINTEGER
        | DUCKDB_TYPE_UBIGINT => Ok(ColumnBuffer::UInt64(Vec::new())),
        DUCKDB_TYPE_FLOAT | DUCKDB_TYPE_DOUBLE => Ok(ColumnBuffer::Float64(Vec::new())),
        DUCKDB_TYPE_VARCHAR | DUCKDB_TYPE_DATE => Ok(ColumnBuffer::Utf8(Vec::new())),
        _ => Err(MarketError::computation(format!(
            "DuckDB result type {value_type} is not supported by the columnar adapter"
        ))),
    }
}

fn append_vector<T: Copy>(
    target: &mut Vec<Option<T>>,
    data: *const T,
    validity: *mut u64,
    row_count: usize,
) {
    if row_count == 0 {
        return;
    }
    let source = unsafe { std::slice::from_raw_parts(data, row_count) };
    for (row, value) in source.iter().enumerate() {
        target.push(is_valid(validity, row).then_some(*value));
    }
}

fn append_cast_vector<T: Copy, U: From<T>>(
    target: &mut Vec<Option<U>>,
    data: *const T,
    validity: *mut u64,
    row_count: usize,
) {
    if row_count == 0 {
        return;
    }
    let source = unsafe { std::slice::from_raw_parts(data, row_count) };
    for (row, value) in source.iter().enumerate() {
        target.push(is_valid(validity, row).then_some((*value).into()));
    }
}

fn append_float_vector<T: Copy + Into<f64>>(
    target: &mut Vec<Option<f64>>,
    data: *const T,
    validity: *mut u64,
    row_count: usize,
    chunk_index: DuckDbIdx,
    column_name: &str,
) -> Result<(), MarketError> {
    if row_count == 0 {
        return Ok(());
    }
    let source = unsafe { std::slice::from_raw_parts(data, row_count) };
    for (row, value) in source.iter().enumerate() {
        if !is_valid(validity, row) {
            target.push(None);
            continue;
        }
        let value = (*value).into();
        if !value.is_finite() {
            return Err(MarketError::computation(format!(
                "DuckDB returned a non-finite float at chunk {chunk_index}, column {column_name}, row {row}"
            )));
        }
        target.push(Some(value));
    }
    Ok(())
}

fn is_valid(validity: *mut u64, row: usize) -> bool {
    validity.is_null() || unsafe { *validity.add(row / 64) & (1 << (row % 64)) != 0 }
}

fn validate_internal_query(sql: &str) -> Result<(), MarketError> {
    if sql.trim().is_empty() {
        return Err(MarketError::validation(
            "DuckDB internal SQL must not be empty",
        ));
    }
    if sql.contains(';') {
        return Err(MarketError::validation(
            "DuckDB internal SQL must contain exactly one statement without a semicolon",
        ));
    }
    let normalized = sql.trim_start().to_ascii_uppercase();
    if !(normalized.starts_with("SELECT") || normalized.starts_with("WITH")) {
        return Err(MarketError::validation(
            "DuckDB internal SQL must be a read-only SELECT or WITH query",
        ));
    }
    for prohibited in [
        "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "COPY", "ATTACH",
    ] {
        if normalized
            .split(|character: char| !character.is_ascii_alphabetic())
            .any(|token| token == prohibited)
        {
            return Err(MarketError::validation(format!(
                "DuckDB internal SQL contains prohibited {prohibited} statement keyword"
            )));
        }
    }
    Ok(())
}

fn into_c_string(value: &str, label: &str) -> Result<CString, MarketError> {
    CString::new(value)
        .map_err(|_| MarketError::validation(format!("{label} contains an interior NUL byte")))
}

fn c_str_or_default(ptr: *const libc::c_char, fallback: &str) -> String {
    if ptr.is_null() {
        fallback.to_string()
    } else {
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }
}

fn library_candidates() -> Result<Vec<String>, MarketError> {
    let mut candidates = Vec::new();

    if let Some(path) = duckdb_library_override()? {
        candidates.push(path.display().to_string());
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push("libduckdb.dylib".to_string());
    }

    #[cfg(target_os = "linux")]
    {
        candidates.push("libduckdb.so".to_string());
        candidates.push("libduckdb.so.1".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        candidates.push("duckdb.dll".to_string());
        candidates.push("libduckdb.dll".to_string());
    }

    Ok(candidates)
}

fn duckdb_library_override() -> Result<Option<PathBuf>, MarketError> {
    let Ok(value) = std::env::var("DUCKDB_LIBRARY_PATH") else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Err(MarketError::configuration(
            "DUCKDB_LIBRARY_PATH is set but empty; provide an absolute path to a regular DuckDB shared library",
        ));
    }

    validate_library_path(Path::new(&value)).map(Some)
}

fn validate_library_path(path: &Path) -> Result<PathBuf, MarketError> {
    if !path.is_absolute() {
        return Err(MarketError::configuration(format!(
            "DUCKDB_LIBRARY_PATH must be an absolute path, got {}",
            path.display()
        )));
    }
    let metadata = std::fs::metadata(path).map_err(|error| {
        MarketError::configuration(format!(
            "DUCKDB_LIBRARY_PATH must name a readable regular library file at {}: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(MarketError::configuration(format!(
            "DUCKDB_LIBRARY_PATH must name a regular library file, got {}",
            path.display()
        )));
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        DuckDbApi, DuckDbSession, InvocationSlot, validate_internal_query, validate_library_path,
    };
    use crate::engine::duckdb_engine::TrustedEngineQuery;
    use crate::engine::duckdb_ta_table_function::TaInvocation;
    use crate::engine::error::ErrorKind;
    use crate::engine::traits::ComputedFrame;
    use crate::model::kline::Kline;
    use crate::ta::indicator::IndicatorSettings;
    use serde_json::json;

    #[test]
    fn platform_library_name_is_a_fallback_candidate() {
        let missing = std::path::Path::new("relative/libduckdb.dylib");
        let error = validate_library_path(missing).unwrap_err();
        assert_eq!(error.kind, ErrorKind::ConfigurationError);
        assert!(error.message.contains("absolute path"));
    }

    #[test]
    fn invocation_slot_rejects_a_second_pending_job_and_clears_on_scope_drop() {
        let slot = InvocationSlot::default();
        let kline = Kline {
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.5,
            volume: 1_000.0,
            time: 1_700_000_000_000,
            adjclose: None,
        };
        let first = TaInvocation::new(vec![kline], IndicatorSettings::default()).unwrap();
        let second = TaInvocation::new(vec![kline], IndicatorSettings::default()).unwrap();
        let scope = slot.install(first).unwrap();

        let error = match slot.install(second) {
            Ok(_) => panic!("a second pending invocation must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.kind, ErrorKind::InvocationLifecycleError);
        assert!(slot.has_pending());
        drop(scope);
        assert!(!slot.has_pending());
    }

    #[test]
    fn library_override_requires_an_existing_regular_file_without_loading_it() {
        let regular_file = std::env::current_dir().unwrap().join("Cargo.toml");
        assert_eq!(validate_library_path(&regular_file).unwrap(), regular_file);

        let directory = std::env::current_dir().unwrap();
        let error = validate_library_path(&directory).unwrap_err();
        assert_eq!(error.kind, ErrorKind::ConfigurationError);
        assert!(error.message.contains("regular library file"));
    }

    #[test]
    fn internal_query_contract_rejects_empty_multi_statement_and_destructive_sql() {
        for sql in [
            "",
            "SELECT 1; SELECT 2",
            "DROP TABLE prices",
            "WITH x AS (SELECT 1) DELETE FROM x",
        ] {
            assert!(
                validate_internal_query(sql).is_err(),
                "{sql:?} must be rejected"
            );
        }
        validate_internal_query("WITH prices AS (SELECT 1 AS close) SELECT close FROM prices")
            .expect("single read-only internal query must be accepted");
    }

    #[test]
    fn sessions_disconnect_before_close_and_repeated_telegram_projection_is_safe() {
        let api = DuckDbApi::new().expect("DuckDB shared library must load");
        let kline = Kline {
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.5,
            volume: 1_000.0,
            time: 1_700_000_000_000,
            adjclose: None,
        };

        for _ in 0..25 {
            let mut session = api.open_session().expect("DuckDB session must open");
            let frame = session
                .query_invocation_frame(
                    TaInvocation::new(vec![kline], IndicatorSettings::default()).unwrap(),
                    &TrustedEngineQuery::from_test_sql(
                        "SELECT time, close FROM ta_indicator_frame()".into(),
                    ),
                )
                .expect("Telegram projection query must execute");
            assert_eq!(frame.len(), 1);
            drop(session);
        }
    }

    #[test]
    fn registered_descriptor_is_destroyed_and_extra_info_survives_until_session_close() {
        let api = DuckDbApi::new().expect("DuckDB shared library must load");
        let destructor_runs = Arc::new(AtomicUsize::new(0));
        let kline = Kline {
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.5,
            volume: 1_000.0,
            time: 1_700_000_000_000,
            adjclose: None,
        };

        {
            let mut session = DuckDbSession::open_with_extra_info_drop_counter(
                Arc::clone(&api),
                Arc::clone(&destructor_runs),
            )
            .expect("DuckDB session must open");
            session
                .query_invocation_frame(
                    TaInvocation::new(vec![kline], IndicatorSettings::default()).unwrap(),
                    &TrustedEngineQuery::from_test_sql(
                        "SELECT time FROM ta_indicator_frame()".into(),
                    ),
                )
                .expect("registered UDF must use its copied extra info");
        }
        assert_eq!(
            destructor_runs.load(Ordering::SeqCst),
            1,
            "DuckDB must invoke its extra-info destructor exactly once"
        );

        let mut next_session = api.open_session().expect("subsequent session must open");
        let frame = next_session
            .query_invocation_frame(
                TaInvocation::new(vec![kline], IndicatorSettings::default()).unwrap(),
                &TrustedEngineQuery::from_test_sql("SELECT close FROM ta_indicator_frame()".into()),
            )
            .expect("subsequent UDF query must not use freed extra info");
        assert_eq!(frame.len(), 1);
    }

    #[test]
    #[ignore = "requires an installed architecture-matched DuckDB shared library"]
    fn duckdb_runtime_contract() {
        let api = DuckDbApi::new().expect("DuckDB shared library must load");
        assert!(!api.library_version().trim().is_empty());
        let rows: serde_json::Value = serde_json::from_str(
            &api.query_to_json(&TrustedEngineQuery::from_test_sql(
                "SELECT TRUE AS enabled, 7::BIGINT AS count, 1.5::DOUBLE AS price, 'ok' AS label, NULL::VARCHAR AS absent".into(),
            ))
            .expect("DuckDB query must succeed"),
        )
        .expect("DuckDB result must be JSON");
        assert_eq!(
            rows,
            json!([{"enabled": true, "count": 7, "price": 1.5, "label": "ok", "absent": null}])
        );
    }

    #[test]
    #[ignore = "requires an installed architecture-matched DuckDB shared library"]
    fn columnar_result_adapter_preserves_primitive_null_date_and_json_parity() {
        let api = DuckDbApi::new().expect("DuckDB shared library must load");
        let session = api.open_session().expect("DuckDB session must open");
        let frame = session
            .query_to_columnar_frame(&TrustedEngineQuery::from_test_sql(
                "SELECT 1.5::DOUBLE AS float64, -7::BIGINT AS int64, 18446744073709551615::UBIGINT AS uint64, TRUE AS boolean, 'alpha'::VARCHAR AS utf8, DATE '2024-01-02' AS date_text, NULL::DOUBLE AS nullable".into(),
            ))
            .expect("columnar decoding must succeed after the session result is freed");

        assert_eq!(frame.len(), 1);
        assert_eq!(
            frame.columns(),
            vec![
                "float64",
                "int64",
                "uint64",
                "boolean",
                "utf8",
                "date_text",
                "nullable"
            ]
        );
        assert_eq!(frame.f64_at("float64", 0).unwrap(), Some(1.5));
        assert_eq!(
            frame.string_at("utf8", 0).unwrap().as_deref(),
            Some("alpha")
        );
        assert_eq!(
            frame.string_at("date_text", 0).unwrap().as_deref(),
            Some("2024-01-02")
        );
        assert_eq!(frame.f64_at("nullable", 0).unwrap(), None);
        assert_eq!(
            frame.to_json_records().unwrap(),
            vec![serde_json::Map::from_iter([
                ("float64".into(), json!(1.5)),
                ("int64".into(), json!(-7)),
                ("uint64".into(), json!(18446744073709551615u64)),
                ("boolean".into(), json!(true)),
                ("utf8".into(), json!("alpha")),
                ("date_text".into(), json!("2024-01-02")),
                ("nullable".into(), serde_json::Value::Null),
            ])]
        );
    }

    #[test]
    #[ignore = "requires an installed architecture-matched DuckDB shared library"]
    fn columnar_result_adapter_preserves_empty_and_multi_chunk_order() {
        let api = DuckDbApi::new().expect("DuckDB shared library must load");
        let session = api.open_session().expect("DuckDB session must open");
        let empty = session
            .query_to_columnar_frame(&TrustedEngineQuery::from_test_sql(
                "SELECT 1::DOUBLE AS first, 'x'::VARCHAR AS second WHERE FALSE".into(),
            ))
            .expect("empty result must decode");
        assert!(empty.is_empty());
        assert_eq!(empty.columns(), vec!["first", "second"]);
        assert!(empty.to_json_records().unwrap().is_empty());

        let frame = session
            .query_to_columnar_frame(&TrustedEngineQuery::from_test_sql(
                "SELECT range::BIGINT AS ordinal, CASE WHEN range % 2 = 0 THEN NULL::VARCHAR ELSE 'odd' END AS label FROM range(5000) ORDER BY range".into(),
            ))
            .expect("multi-chunk result must decode");
        assert_eq!(frame.len(), 5_000);
        assert_eq!(frame.columns(), vec!["ordinal", "label"]);
        assert_eq!(frame.f64_at("ordinal", 0).unwrap(), Some(0.0));
        assert_eq!(frame.f64_at("ordinal", 4_999).unwrap(), Some(4_999.0));
        assert_eq!(frame.string_at("label", 0).unwrap(), None);
        assert_eq!(
            frame.string_at("label", 4_999).unwrap().as_deref(),
            Some("odd")
        );
        let records = frame.to_json_records().unwrap();
        assert_eq!(records[2_048]["ordinal"], json!(2_048));
        assert_eq!(records[2_049]["label"], json!("odd"));
    }

    #[test]
    #[ignore = "requires an installed architecture-matched DuckDB shared library"]
    fn columnar_result_adapter_reports_non_finite_column_context() {
        let api = DuckDbApi::new().expect("DuckDB shared library must load");
        let session = api.open_session().expect("DuckDB session must open");
        let error = session
            .query_to_columnar_frame(&TrustedEngineQuery::from_test_sql(
                "SELECT 'NaN'::DOUBLE AS invalid_price".into(),
            ))
            .expect_err("non-finite floats must retain the legacy fail-closed contract");
        assert!(error.message.contains("chunk 0"));
        assert!(error.message.contains("invalid_price"));
    }

    /// Deterministic, report-only proof that owned column access does not require JSON rows.
    #[test]
    #[ignore = "runtime columnar result decode benchmark (1,440 and 100,000 rows)"]
    fn columnar_result_decode_benchmark() {
        use std::time::Instant;

        let api = DuckDbApi::new().expect("DuckDB shared library must load");
        let session = api.open_session().expect("DuckDB session must open");

        for rows in [1_440usize, 100_000usize] {
            let query = TrustedEngineQuery::from_test_sql(format!(
                "SELECT range::BIGINT AS time, 100.0 + range / 100.0 AS close, range % 2 = 0 AS is_atr_gap, '2024-01-01'::VARCHAR AS \"Date\", CASE WHEN range % 3 = 0 THEN NULL::DOUBLE ELSE range / 10.0 END AS atr FROM range({rows}) ORDER BY range"
            ));
            let result = api
                .run_query(session.connection().raw, query.sql())
                .expect("crypto-like benchmark query must succeed");
            let decode_start = Instant::now();
            let frame = api
                .decode_result_chunks(&result)
                .expect("columnar result decode must succeed");
            let decode_elapsed = decode_start.elapsed();

            assert_eq!(frame.len(), rows, "benchmark must decode real rows");
            assert_eq!(
                frame.f64_at("close", rows - 1).unwrap(),
                Some(100.0 + (rows - 1) as f64 / 100.0)
            );
            assert_eq!(
                frame.string_at("Date", 0).unwrap().as_deref(),
                Some("2024-01-01")
            );
            assert_eq!(frame.f64_at("atr", 0).unwrap(), None);

            let json_start = Instant::now();
            let records = frame
                .to_json_records()
                .expect("lazy JSON materialization must succeed");
            let json_elapsed = json_start.elapsed();
            assert_eq!(records.len(), rows);
            println!(
                "columnar_result_decode rows={rows} decode={decode_elapsed:?} to_json_records={json_elapsed:?} columns={} last_close={}",
                frame.columns().len(),
                records[rows - 1]["close"]
            );
        }
    }
}

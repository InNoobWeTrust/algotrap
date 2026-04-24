//! Dynamically loaded DuckDB C API bindings and safe wrappers.

use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Arc;

use libloading::Library;
use once_cell::sync::Lazy;
use serde_json::{Map, Number, Value};

use crate::engine::error::MarketError;

pub const DUCKDB_SUCCESS: DuckDbState = 0;
pub const DUCKDB_ERROR: DuckDbState = 1;

pub const DUCKDB_TYPE_INVALID: DuckDbType = 0;
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
pub const DUCKDB_TYPE_TIMESTAMP: DuckDbType = 12;
pub const DUCKDB_TYPE_DATE: DuckDbType = 13;
pub const DUCKDB_TYPE_TIME: DuckDbType = 14;
pub const DUCKDB_TYPE_INTERVAL: DuckDbType = 15;
pub const DUCKDB_TYPE_HUGEINT: DuckDbType = 16;
pub const DUCKDB_TYPE_VARCHAR: DuckDbType = 17;
pub const DUCKDB_TYPE_BLOB: DuckDbType = 18;
pub const DUCKDB_TYPE_DECIMAL: DuckDbType = 19;
pub const DUCKDB_TYPE_TIMESTAMP_S: DuckDbType = 20;
pub const DUCKDB_TYPE_TIMESTAMP_MS: DuckDbType = 21;
pub const DUCKDB_TYPE_TIMESTAMP_NS: DuckDbType = 22;
pub const DUCKDB_TYPE_ENUM: DuckDbType = 23;
pub const DUCKDB_TYPE_LIST: DuckDbType = 24;
pub const DUCKDB_TYPE_STRUCT: DuckDbType = 25;
pub const DUCKDB_TYPE_MAP: DuckDbType = 26;
pub const DUCKDB_TYPE_UUID: DuckDbType = 27;
pub const DUCKDB_TYPE_UNION: DuckDbType = 28;
pub const DUCKDB_TYPE_BIT: DuckDbType = 29;
pub const DUCKDB_TYPE_TIME_TZ: DuckDbType = 30;
pub const DUCKDB_TYPE_TIMESTAMP_TZ: DuckDbType = 31;
pub const DUCKDB_TYPE_UHUGEINT: DuckDbType = 32;
pub const DUCKDB_TYPE_ARRAY: DuckDbType = 33;
pub const DUCKDB_TYPE_ANY: DuckDbType = 34;
pub const DUCKDB_TYPE_BIGNUM: DuckDbType = 35;
pub const DUCKDB_TYPE_SQLNULL: DuckDbType = 36;
pub const DUCKDB_TYPE_STRING_LITERAL: DuckDbType = 37;
pub const DUCKDB_TYPE_INTEGER_LITERAL: DuckDbType = 38;
pub const DUCKDB_TYPE_TIME_NS: DuckDbType = 39;
pub const DUCKDB_TYPE_GEOMETRY: DuckDbType = 40;

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
pub struct DuckDBArrow {
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
#[derive(Debug)]
pub struct DuckDBResult {
    deprecated_column_count: DuckDbIdx,
    deprecated_row_count: DuckDbIdx,
    deprecated_rows_changed: DuckDbIdx,
    deprecated_columns: *mut DuckDBColumn,
    deprecated_error_message: *mut libc::c_char,
    internal_data: *mut libc::c_void,
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

type DuckDbDatabaseHandle = *mut DuckDBDatabase;
type DuckDbConnectionHandle = *mut DuckDBConnection;
type DuckDbArrowHandle = *mut DuckDBArrow;

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
type DuckdbRowCountFn = unsafe extern "C" fn(*mut DuckDBResult) -> DuckDbIdx;
type DuckdbColumnNameFn = unsafe extern "C" fn(*mut DuckDBResult, DuckDbIdx) -> *const libc::c_char;
type DuckdbColumnTypeFn = unsafe extern "C" fn(*mut DuckDBResult, DuckDbIdx) -> DuckDbType;
type DuckdbValueIsNullFn = unsafe extern "C" fn(*mut DuckDBResult, DuckDbIdx, DuckDbIdx) -> bool;
type DuckdbValueBooleanFn = unsafe extern "C" fn(*mut DuckDBResult, DuckDbIdx, DuckDbIdx) -> bool;
type DuckdbValueInt64Fn = unsafe extern "C" fn(*mut DuckDBResult, DuckDbIdx, DuckDbIdx) -> i64;
type DuckdbValueUint64Fn = unsafe extern "C" fn(*mut DuckDBResult, DuckDbIdx, DuckDbIdx) -> u64;
type DuckdbValueDoubleFn =
    unsafe extern "C" fn(*mut DuckDBResult, DuckDbIdx, DuckDbIdx) -> libc::c_double;
type DuckdbValueVarcharFn =
    unsafe extern "C" fn(*mut DuckDBResult, DuckDbIdx, DuckDbIdx) -> *mut libc::c_char;
type DuckdbFreeFn = unsafe extern "C" fn(*mut libc::c_void);
type DuckdbLibraryVersionFn = unsafe extern "C" fn() -> *const libc::c_char;
type DuckdbQueryArrowFn = unsafe extern "C" fn(
    DuckDbConnectionHandle,
    *const libc::c_char,
    *mut DuckDbArrowHandle,
) -> DuckDbState;
type DuckdbArrowColumnCountFn = unsafe extern "C" fn(DuckDbArrowHandle) -> DuckDbIdx;
type DuckdbArrowRowCountFn = unsafe extern "C" fn(DuckDbArrowHandle) -> DuckDbIdx;
type DuckdbQueryArrowErrorFn = unsafe extern "C" fn(DuckDbArrowHandle) -> *const libc::c_char;
type DuckdbDestroyArrowFn = unsafe extern "C" fn(*mut DuckDbArrowHandle);

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
    pub duckdb_row_count: DuckdbRowCountFn,
    pub duckdb_column_name: DuckdbColumnNameFn,
    pub duckdb_column_type: DuckdbColumnTypeFn,
    pub duckdb_value_is_null: DuckdbValueIsNullFn,
    pub duckdb_value_boolean: DuckdbValueBooleanFn,
    pub duckdb_value_int64: DuckdbValueInt64Fn,
    pub duckdb_value_uint64: DuckdbValueUint64Fn,
    pub duckdb_value_double: DuckdbValueDoubleFn,
    pub duckdb_value_varchar: DuckdbValueVarcharFn,
    pub duckdb_free: DuckdbFreeFn,
    pub duckdb_library_version: DuckdbLibraryVersionFn,
    pub duckdb_query_arrow: DuckdbQueryArrowFn,
    pub duckdb_arrow_column_count: DuckdbArrowColumnCountFn,
    pub duckdb_arrow_row_count: DuckdbArrowRowCountFn,
    pub duckdb_query_arrow_error: DuckdbQueryArrowErrorFn,
    pub duckdb_destroy_arrow: DuckdbDestroyArrowFn,
}

static DUCKDB_API: Lazy<Result<Arc<DuckDbApi>, MarketError>> = Lazy::new(DuckDbApi::new);

/// Returns the lazily initialized, process-wide DuckDB API singleton.
pub fn duckdb_api() -> Result<Arc<DuckDbApi>, MarketError> {
    DUCKDB_API.as_ref().map(Arc::clone).map_err(Clone::clone)
}

pub struct DuckDbArrowResult<'a> {
    api: &'a DuckDbApi,
    raw: DuckDbArrowHandle,
    row_count: usize,
    column_count: usize,
    _connection: ConnectionHandle<'a>,
    _database: DatabaseHandle<'a>,
}

impl DuckDbArrowResult<'_> {
    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn column_count(&self) -> usize {
        self.column_count
    }
}

impl Drop for DuckDbArrowResult<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                (self.api.duckdb_destroy_arrow)(&mut self.raw);
            }
        }
    }
}

struct DatabaseHandle<'a> {
    api: &'a DuckDbApi,
    raw: DuckDbDatabaseHandle,
}

impl Drop for DatabaseHandle<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                (self.api.duckdb_close)(&mut self.raw);
            }
        }
    }
}

struct ConnectionHandle<'a> {
    api: &'a DuckDbApi,
    raw: DuckDbConnectionHandle,
}

impl Drop for ConnectionHandle<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                (self.api.duckdb_disconnect)(&mut self.raw);
            }
        }
    }
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
                duckdb_row_count: Self::load_symbol(&lib, b"duckdb_row_count\0")?,
                duckdb_column_name: Self::load_symbol(&lib, b"duckdb_column_name\0")?,
                duckdb_column_type: Self::load_symbol(&lib, b"duckdb_column_type\0")?,
                duckdb_value_is_null: Self::load_symbol(&lib, b"duckdb_value_is_null\0")?,
                duckdb_value_boolean: Self::load_symbol(&lib, b"duckdb_value_boolean\0")?,
                duckdb_value_int64: Self::load_symbol(&lib, b"duckdb_value_int64\0")?,
                duckdb_value_uint64: Self::load_symbol(&lib, b"duckdb_value_uint64\0")?,
                duckdb_value_double: Self::load_symbol(&lib, b"duckdb_value_double\0")?,
                duckdb_value_varchar: Self::load_symbol(&lib, b"duckdb_value_varchar\0")?,
                duckdb_free: Self::load_symbol(&lib, b"duckdb_free\0")?,
                duckdb_library_version: Self::load_symbol(&lib, b"duckdb_library_version\0")?,
                duckdb_query_arrow: Self::load_symbol(&lib, b"duckdb_query_arrow\0")?,
                duckdb_arrow_column_count: Self::load_symbol(&lib, b"duckdb_arrow_column_count\0")?,
                duckdb_arrow_row_count: Self::load_symbol(&lib, b"duckdb_arrow_row_count\0")?,
                duckdb_query_arrow_error: Self::load_symbol(&lib, b"duckdb_query_arrow_error\0")?,
                duckdb_destroy_arrow: Self::load_symbol(&lib, b"duckdb_destroy_arrow\0")?,
                _lib: lib,
            }))
        }
    }

    pub fn library_version(&self) -> String {
        let ptr = unsafe { (self.duckdb_library_version)() };
        c_str_or_default(ptr, "unknown DuckDB version")
    }

    pub fn execute_sql(&self, sql: &str) -> Result<(), MarketError> {
        self.execute_sql_on_path(None, sql)
    }

    pub fn execute_sql_on_path(
        &self,
        database_path: Option<&str>,
        sql: &str,
    ) -> Result<(), MarketError> {
        validate_sql(sql)?;
        let database = self.open_database(database_path)?;
        let connection = self.connect(database.raw)?;
        self.run_query(connection.raw, sql)?;
        Ok(())
    }

    pub fn query_to_json(&self, sql: &str) -> Result<String, MarketError> {
        self.query_to_json_on_path(None, sql)
    }

    pub fn query_to_json_on_path(
        &self,
        database_path: Option<&str>,
        sql: &str,
    ) -> Result<String, MarketError> {
        validate_sql(sql)?;
        let database = self.open_database(database_path)?;
        let connection = self.connect(database.raw)?;
        let result = self.run_query(connection.raw, sql)?;

        let row_count = self.result_row_count(&result)?;
        let column_count = self.result_column_count(&result)?;
        let mut rows = Vec::with_capacity(row_count);

        for row in 0..row_count as DuckDbIdx {
            let mut object = Map::with_capacity(column_count);

            for column in 0..column_count as DuckDbIdx {
                let name = self.result_column_name(&result, column)?;
                let value = self.result_json_value(&result, column, row)?;
                object.insert(name, value);
            }

            rows.push(Value::Object(object));
        }

        serde_json::to_string(&rows).map_err(|err| {
            MarketError::computation(format!("failed to serialize DuckDB result to JSON: {err}"))
        })
    }

    pub fn query_to_arrow(&self, sql: &str) -> Result<DuckDbArrowResult<'_>, MarketError> {
        self.query_to_arrow_on_path(None, sql)
    }

    pub fn query_to_arrow_on_path(
        &self,
        database_path: Option<&str>,
        sql: &str,
    ) -> Result<DuckDbArrowResult<'_>, MarketError> {
        validate_sql(sql)?;
        let database = self.open_database(database_path)?;
        let connection = self.connect(database.raw)?;
        let sql = into_c_string(sql, "SQL query")?;
        let mut raw_arrow: DuckDbArrowHandle = ptr::null_mut();

        let state =
            unsafe { (self.duckdb_query_arrow)(connection.raw, sql.as_ptr(), &mut raw_arrow) };

        if state != DUCKDB_SUCCESS {
            let error = self.arrow_error(raw_arrow);
            if !raw_arrow.is_null() {
                unsafe {
                    (self.duckdb_destroy_arrow)(&mut raw_arrow);
                }
            }
            return Err(MarketError::data_access(error));
        }

        if raw_arrow.is_null() {
            return Err(MarketError::data_access(
                "duckdb_query_arrow succeeded but returned a null arrow result",
            ));
        }

        let row_count = unsafe { (self.duckdb_arrow_row_count)(raw_arrow) as usize };
        let column_count = unsafe { (self.duckdb_arrow_column_count)(raw_arrow) as usize };

        Ok(DuckDbArrowResult {
            api: self,
            raw: raw_arrow,
            row_count,
            column_count,
            _connection: connection,
            _database: database,
        })
    }

    fn load_library() -> Result<Library, MarketError> {
        let mut errors = Vec::new();

        for candidate in library_candidates() {
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
        &self,
        database_path: Option<&str>,
    ) -> Result<DatabaseHandle<'_>, MarketError> {
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
            api: self,
            raw: raw_database,
        })
    }

    fn connect(&self, database: DuckDbDatabaseHandle) -> Result<ConnectionHandle<'_>, MarketError> {
        let mut raw_connection: DuckDbConnectionHandle = ptr::null_mut();
        let state = unsafe { (self.duckdb_connect)(database, &mut raw_connection) };

        if state != DUCKDB_SUCCESS || raw_connection.is_null() {
            return Err(MarketError::data_access("failed to open DuckDB connection"));
        }

        Ok(ConnectionHandle {
            api: self,
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

    fn arrow_error(&self, result: DuckDbArrowHandle) -> String {
        let error = if result.is_null() {
            ptr::null()
        } else {
            unsafe { (self.duckdb_query_arrow_error)(result) }
        };
        c_str_or_default(error, "DuckDB Arrow query failed without an error message")
    }

    fn result_column_count(&self, result: &QueryResult<'_>) -> Result<usize, MarketError> {
        let count = unsafe { (self.duckdb_column_count)(result.as_mut_ptr()) };
        usize::try_from(count).map_err(|_| {
            MarketError::computation(format!(
                "DuckDB column count does not fit into usize: {count}"
            ))
        })
    }

    fn result_row_count(&self, result: &QueryResult<'_>) -> Result<usize, MarketError> {
        let count = unsafe { (self.duckdb_row_count)(result.as_mut_ptr()) };
        usize::try_from(count).map_err(|_| {
            MarketError::computation(format!("DuckDB row count does not fit into usize: {count}"))
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

    fn result_json_value(
        &self,
        result: &QueryResult<'_>,
        column: DuckDbIdx,
        row: DuckDbIdx,
    ) -> Result<Value, MarketError> {
        if unsafe { (self.duckdb_value_is_null)(result.as_mut_ptr(), column, row) } {
            return Ok(Value::Null);
        }

        let value_type = unsafe { (self.duckdb_column_type)(result.as_mut_ptr(), column) };
        match value_type {
            DUCKDB_TYPE_BOOLEAN => Ok(Value::Bool(unsafe {
                (self.duckdb_value_boolean)(result.as_mut_ptr(), column, row)
            })),
            DUCKDB_TYPE_TINYINT | DUCKDB_TYPE_SMALLINT | DUCKDB_TYPE_INTEGER
            | DUCKDB_TYPE_BIGINT => Ok(Value::Number(Number::from(unsafe {
                (self.duckdb_value_int64)(result.as_mut_ptr(), column, row)
            }))),
            DUCKDB_TYPE_UTINYINT
            | DUCKDB_TYPE_USMALLINT
            | DUCKDB_TYPE_UINTEGER
            | DUCKDB_TYPE_UBIGINT => Ok(Value::Number(Number::from(unsafe {
                (self.duckdb_value_uint64)(result.as_mut_ptr(), column, row)
            }))),
            DUCKDB_TYPE_FLOAT | DUCKDB_TYPE_DOUBLE | DUCKDB_TYPE_DECIMAL => {
                let value = unsafe { (self.duckdb_value_double)(result.as_mut_ptr(), column, row) };
                let number = Number::from_f64(value).ok_or_else(|| {
                    MarketError::computation(format!(
                        "DuckDB returned a non-finite float at row {row}, column {column}"
                    ))
                })?;
                Ok(Value::Number(number))
            }
            DUCKDB_TYPE_INVALID => Err(MarketError::computation(format!(
                "DuckDB reported DUCKDB_TYPE_INVALID at row {row}, column {column}"
            ))),
            _ => self
                .result_string_value(result, column, row)
                .map(Value::String),
        }
    }

    fn result_string_value(
        &self,
        result: &QueryResult<'_>,
        column: DuckDbIdx,
        row: DuckDbIdx,
    ) -> Result<String, MarketError> {
        let raw_value = unsafe { (self.duckdb_value_varchar)(result.as_mut_ptr(), column, row) };
        if raw_value.is_null() {
            return Err(MarketError::computation(format!(
                "DuckDB returned a null string pointer at row {row}, column {column}"
            )));
        }

        let value = unsafe { CStr::from_ptr(raw_value) }
            .to_string_lossy()
            .into_owned();
        unsafe {
            (self.duckdb_free)(raw_value.cast());
        }
        Ok(value)
    }
}

fn validate_sql(sql: &str) -> Result<(), MarketError> {
    if sql.trim().is_empty() {
        return Err(MarketError::validation("DuckDB SQL must not be empty"));
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

fn library_candidates() -> Vec<String> {
    let mut candidates = Vec::new();

    if let Ok(path) = std::env::var("DUCKDB_LIBRARY_PATH") {
        if !path.trim().is_empty() {
            candidates.push(path);
        }
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

    candidates
}

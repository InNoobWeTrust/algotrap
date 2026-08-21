//! Private DuckDB C-API adapter for whole-series technical indicators.
//!
//! Production path: the engine registers a zero-argument table function
//! (`ta_indicator_frame()`) whose entire input job is an opaque, engine-owned
//! [`TaInvocation`] stored as DuckDB table-function *extra info*. Bind validates
//! the invocation, executes [`IndicatorFrame::compute`] inside the bind callback,
//! and emits DuckDB chunks column-wise (bulk copies, validity masks only for
//! nullable columns).
//!
//! Test-only path: a 17-argument literal-array table function is retained so the
//! performance regression test can compare the invocation path against the old
//! SQL-list-literal baseline in the same process. That path is compiled out of
//! production builds (`#[cfg(test)]`) and is never reachable from engine code.

use std::ffi::CString;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::Arc;

use crate::engine::duckdb_ffi::{
    DUCKDB_SUCCESS, DUCKDB_TYPE_BIGINT, DUCKDB_TYPE_BOOLEAN, DUCKDB_TYPE_DOUBLE, DuckDbApi,
    DuckDbBindInfoHandle, DuckDbConnectionHandle, DuckDbDataChunkHandle, DuckDbFunctionInfoHandle,
    DuckDbIdx, DuckDbInitInfoHandle, DuckDbLogicalTypeHandle, InvocationSlot,
};
use crate::engine::error::MarketError;
use crate::engine::execution_strategy::ExecutionStrategy;
use crate::engine::ta_execution::execute_standard_plan;
use crate::model::kline::Kline;
use crate::ta::indicator::{
    IndicatorColumn, IndicatorFrame, IndicatorOutput, IndicatorProjection, IndicatorSettings,
};

#[cfg(test)]
const SETTINGS_COUNT: usize = 10;

/// Engine-owned, validated, immutable input job handed to DuckDB as extra info.
///
/// `TaInvocation` is private to the engine boundary and can never be expressed
/// in SQL: no identifier or literal from the caller appears in the production
/// query, which is the static zero-argument call `ta_indicator_frame()`.
pub(crate) struct TaInvocation {
    klines: Vec<Kline>,
    settings: IndicatorSettings,
    projection: IndicatorProjection,
    strategy: ExecutionStrategy,
}

impl TaInvocation {
    /// Validates the engine-owned job before it is ever registered with DuckDB.
    #[cfg(test)]
    pub(crate) fn new(
        klines: Vec<Kline>,
        settings: IndicatorSettings,
    ) -> Result<Self, MarketError> {
        Self::with_strategy(klines, settings, ExecutionStrategy::Auto)
    }

    /// Validates an engine-owned job with an explicit engine execution policy.
    pub(crate) fn with_strategy(
        klines: Vec<Kline>,
        settings: IndicatorSettings,
        strategy: ExecutionStrategy,
    ) -> Result<Self, MarketError> {
        validate_invocation_input(&klines, settings)?;
        Ok(Self {
            klines,
            settings,
            projection: IndicatorProjection::Complete,
            strategy,
        })
    }

    /// Validates an engine-owned job with an explicit typed TA projection.
    #[cfg(test)]
    pub(crate) fn projected(
        klines: Vec<Kline>,
        settings: IndicatorSettings,
        projection: IndicatorProjection,
    ) -> Result<Self, MarketError> {
        Self::projected_with_strategy(klines, settings, projection, ExecutionStrategy::Auto)
    }

    /// Validates an engine-owned projected job with an explicit execution policy.
    pub(crate) fn projected_with_strategy(
        klines: Vec<Kline>,
        settings: IndicatorSettings,
        projection: IndicatorProjection,
        strategy: ExecutionStrategy,
    ) -> Result<Self, MarketError> {
        validate_invocation_input(&klines, settings)?;
        Ok(Self {
            klines,
            settings,
            projection,
            strategy,
        })
    }

    fn klines(&self) -> &[Kline] {
        &self.klines
    }

    fn settings(&self) -> IndicatorSettings {
        self.settings
    }

    fn projection(&self) -> &IndicatorProjection {
        &self.projection
    }
}

/// Rejects malformed engine input before registration and again during bind.
///
/// Kept in lock-step with the boundary contract previously enforced by the SQL
/// literal materializer plus [`IndicatorFrame::compute`].
fn validate_invocation_input(
    klines: &[Kline],
    settings: IndicatorSettings,
) -> Result<(), MarketError> {
    if klines.is_empty() {
        return Err(MarketError::validation("Kline slice is empty"));
    }
    settings.validate().map_err(MarketError::from)?;
    let mut previous = None;
    for (index, row) in klines.iter().enumerate() {
        for (name, value) in [
            ("open", row.open),
            ("high", row.high),
            ("low", row.low),
            ("close", row.close),
            ("volume", row.volume),
        ] {
            if !value.is_finite() {
                return Err(MarketError::validation(format!(
                    "kline {index} has non-finite {name}"
                )));
            }
        }
        if let Some(adjclose) = row.adjclose
            && !adjclose.is_finite()
        {
            return Err(MarketError::validation(format!(
                "kline {index} has non-finite adj_close"
            )));
        }
        if previous.is_some_and(|time| row.time <= time) {
            return Err(MarketError::validation(
                "klines must be in strictly increasing chronological order",
            ));
        }
        previous = Some(row.time);
    }
    Ok(())
}

/// Read-only bind data: contiguous column arrays plus the computed indicator frame.
struct BindState {
    len: usize,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    volume: Vec<f64>,
    time: Vec<i64>,
    adj_close: Vec<Option<f64>>,
    frame: IndicatorFrame,
    column_plan: Vec<ColumnPlan>,
}

/// One bound DuckDB output column, in canonical base-then-indicator order.
#[derive(Debug, Clone, Copy)]
enum ColumnPlan {
    Open,
    High,
    Low,
    Close,
    Volume,
    Time,
    AdjClose,
    Indicator(IndicatorOutput),
}

const BASE_COLUMNS: [ColumnPlan; 7] = [
    ColumnPlan::Open,
    ColumnPlan::High,
    ColumnPlan::Low,
    ColumnPlan::Close,
    ColumnPlan::Volume,
    ColumnPlan::Time,
    ColumnPlan::AdjClose,
];

impl ColumnPlan {
    fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::High => "high",
            Self::Low => "low",
            Self::Close => "close",
            Self::Volume => "volume",
            Self::Time => "time",
            Self::AdjClose => "adj_close",
            Self::Indicator(output) => output.column_name(),
        }
    }

    fn type_id(self) -> u32 {
        match self {
            Self::Time => DUCKDB_TYPE_BIGINT,
            Self::Indicator(IndicatorOutput::IsAtrGap) => DUCKDB_TYPE_BOOLEAN,
            _ => DUCKDB_TYPE_DOUBLE,
        }
    }
}

fn column_plan(projection: &IndicatorProjection) -> Vec<ColumnPlan> {
    let mut plan = BASE_COLUMNS.to_vec();
    plan.extend(projection.outputs().map(ColumnPlan::Indicator));
    plan
}

/// Mutable per-scan state owned by each table-function init.
struct InitState {
    cursor: usize,
}

/// Registers the production zero-argument invocation-backed table function.
///
/// The session retains one reference to `invocation_slot`; DuckDB owns the other
/// reference through extra info until the connection is destroyed.
pub(crate) fn register(
    api: &DuckDbApi,
    connection: DuckDbConnectionHandle,
    invocation_slot: Arc<InvocationSlot>,
) -> Result<(), MarketError> {
    let table_function = unsafe { (api.duckdb_create_table_function)() };
    if table_function.is_null() {
        return Err(MarketError::data_access(
            "DuckDB failed to create ta_indicator_frame function",
        ));
    }
    let guard = TableFunctionGuard {
        api,
        raw: table_function,
    };
    let name = CString::new("ta_indicator_frame").expect("static name contains no NUL");
    unsafe {
        (api.duckdb_table_function_set_name)(guard.raw, name.as_ptr());
        (api.duckdb_table_function_set_extra_info)(
            guard.raw,
            Arc::into_raw(Arc::clone(&invocation_slot))
                .cast_mut()
                .cast(),
            destroy_extra_info,
        );
        (api.duckdb_table_function_set_bind)(guard.raw, bind);
        (api.duckdb_table_function_set_init)(guard.raw, init);
        (api.duckdb_table_function_set_function)(guard.raw, function);
    }
    let state = unsafe { (api.duckdb_register_table_function)(connection, guard.raw) };
    if state != DUCKDB_SUCCESS {
        return Err(MarketError::data_access(
            "DuckDB failed to register ta_indicator_frame table function",
        ));
    }
    // DuckDB registers its own copy. The C API keeps caller ownership of this
    // descriptor, which must be destroyed even after a successful registration.
    drop(guard);
    Ok(())
}

/// Test-only registration of the retained literal-array baseline function.
#[cfg(test)]
pub(crate) fn register_literal(
    api: &DuckDbApi,
    connection: DuckDbConnectionHandle,
) -> Result<(), MarketError> {
    let table_function = unsafe { (api.duckdb_create_table_function)() };
    if table_function.is_null() {
        return Err(MarketError::data_access(
            "DuckDB failed to create literal ta_indicator_frame function",
        ));
    }
    let guard = TableFunctionGuard {
        api,
        raw: table_function,
    };
    let name = CString::new("ta_indicator_frame").expect("static name contains no NUL");
    unsafe {
        (api.duckdb_table_function_set_name)(guard.raw, name.as_ptr());
        add_list_parameter(api, guard.raw, DUCKDB_TYPE_DOUBLE)?;
        add_list_parameter(api, guard.raw, DUCKDB_TYPE_DOUBLE)?;
        add_list_parameter(api, guard.raw, DUCKDB_TYPE_DOUBLE)?;
        add_list_parameter(api, guard.raw, DUCKDB_TYPE_DOUBLE)?;
        add_list_parameter(api, guard.raw, DUCKDB_TYPE_DOUBLE)?;
        add_list_parameter(api, guard.raw, DUCKDB_TYPE_BIGINT)?;
        add_list_parameter(api, guard.raw, DUCKDB_TYPE_DOUBLE)?;
        for _ in 0..SETTINGS_COUNT {
            add_parameter(api, guard.raw, DUCKDB_TYPE_BIGINT)?;
        }
        (api.duckdb_table_function_set_bind)(guard.raw, bind_literal);
        (api.duckdb_table_function_set_init)(guard.raw, init);
        (api.duckdb_table_function_set_function)(guard.raw, function);
    }
    let state = unsafe { (api.duckdb_register_table_function)(connection, guard.raw) };
    if state != DUCKDB_SUCCESS {
        return Err(MarketError::data_access(
            "DuckDB failed to register literal ta_indicator_frame table function",
        ));
    }
    drop(guard);
    Ok(())
}

#[cfg(test)]
unsafe fn add_parameter(
    api: &DuckDbApi,
    function: *mut crate::engine::duckdb_ffi::DuckDBTableFunction,
    ty: u32,
) -> Result<(), MarketError> {
    let logical = LogicalTypeGuard::new(api, ty)?;
    unsafe { (api.duckdb_table_function_add_parameter)(function, logical.raw) };
    Ok(())
}

#[cfg(test)]
unsafe fn add_list_parameter(
    api: &DuckDbApi,
    function: *mut crate::engine::duckdb_ffi::DuckDBTableFunction,
    child: u32,
) -> Result<(), MarketError> {
    let child_type = LogicalTypeGuard::new(api, child)?;
    let list = unsafe { (api.duckdb_create_list_type)(child_type.raw) };
    let list_type = LogicalTypeGuard::from_raw(api, list)?;
    unsafe { (api.duckdb_table_function_add_parameter)(function, list_type.raw) };
    Ok(())
}

unsafe extern "C" fn bind(info: DuckDbBindInfoHandle) {
    let outcome = catch_unwind(AssertUnwindSafe(|| bind_inner(info)));
    if let Err(message) =
        outcome.unwrap_or_else(|_| Err("ta_indicator_frame bind callback panicked".into()))
    {
        set_bind_error(info, &message);
    }
}

fn bind_inner(info: DuckDbBindInfoHandle) -> Result<(), String> {
    let api = crate::engine::duckdb_ffi::duckdb_api().map_err(|error| error.message)?;
    let api = api.as_ref();
    if unsafe { (api.duckdb_bind_get_parameter_count)(info) } != 0 {
        return Err("ta_indicator_frame takes no arguments".into());
    }
    let extra = unsafe { (api.duckdb_bind_get_extra_info)(info) };
    if extra.is_null() {
        return Err("ta_indicator_frame is missing its engine invocation".into());
    }
    let invocation = unsafe { &*(extra as *const InvocationSlot) }.take_for_bind()?;
    let plan = column_plan(invocation.projection());
    add_result_columns(api, info, &plan)?;
    let state = build_bind_state(
        invocation.klines(),
        invocation.settings(),
        invocation.projection(),
        invocation.strategy,
        plan,
    )?;
    unsafe { (api.duckdb_bind_set_cardinality)(info, state.len as DuckDbIdx, true) };
    unsafe {
        (api.duckdb_bind_set_bind_data)(info, Box::into_raw(state).cast(), destroy_box::<BindState>)
    };
    Ok(())
}

#[cfg(test)]
unsafe extern "C" fn bind_literal(info: DuckDbBindInfoHandle) {
    let outcome = catch_unwind(AssertUnwindSafe(|| bind_literal_inner(info)));
    if let Err(message) =
        outcome.unwrap_or_else(|_| Err("ta_indicator_frame literal bind callback panicked".into()))
    {
        set_bind_error(info, &message);
    }
}

#[cfg(test)]
fn bind_literal_inner(info: DuckDbBindInfoHandle) -> Result<(), String> {
    let api = crate::engine::duckdb_ffi::duckdb_api().map_err(|error| error.message)?;
    let api = api.as_ref();
    let expected = 7 + SETTINGS_COUNT;
    if unsafe { (api.duckdb_bind_get_parameter_count)(info) } != expected as DuckDbIdx {
        return Err(format!("ta_indicator_frame requires {expected} arguments"));
    }
    let open = read_double_list(api, info, 0, "open")?;
    let high = read_double_list(api, info, 1, "high")?;
    let low = read_double_list(api, info, 2, "low")?;
    let close = read_double_list(api, info, 3, "close")?;
    let volume = read_double_list(api, info, 4, "volume")?;
    let time = read_time_list(api, info, 5)?;
    let adjclose = read_optional_double_list(api, info, 6, "adj_close")?;
    let lengths = [
        high.len(),
        low.len(),
        close.len(),
        volume.len(),
        time.len(),
        adjclose.len(),
    ];
    if open.is_empty() || lengths.iter().any(|length| *length != open.len()) {
        return Err("ta_indicator_frame requires non-empty equal-length input lists".into());
    }
    let values = (0..SETTINGS_COUNT)
        .map(|index| read_positive_period(api, info, index + 7))
        .collect::<Result<Vec<_>, _>>()?;
    let settings = IndicatorSettings {
        volume_ema_period: values[0],
        ema_period: values[1],
        rsi_period: values[2],
        rsi_smooth_period: values[3],
        reverse_rsi_period: values[4],
        atr_period: values[5],
        bias_period: values[6],
        structure_period: values[7],
        structure_sma_period: values[8],
        sharpe_period: values[9],
    };
    let klines = (0..open.len())
        .map(|index| Kline {
            open: open[index],
            high: high[index],
            low: low[index],
            close: close[index],
            volume: volume[index],
            time: time[index],
            adjclose: adjclose[index],
        })
        .collect::<Vec<_>>();
    let projection = IndicatorProjection::Complete;
    let plan = column_plan(&projection);
    add_result_columns(api, info, &plan)?;
    let state = build_bind_state(
        &klines,
        settings,
        &projection,
        ExecutionStrategy::Sequential,
        plan,
    )?;
    unsafe { (api.duckdb_bind_set_cardinality)(info, state.len as DuckDbIdx, true) };
    unsafe {
        (api.duckdb_bind_set_bind_data)(info, Box::into_raw(state).cast(), destroy_box::<BindState>)
    };
    Ok(())
}

/// Validates input, computes the frame, and materializes contiguous column arrays.
fn build_bind_state(
    klines: &[Kline],
    settings: IndicatorSettings,
    projection: &IndicatorProjection,
    strategy: ExecutionStrategy,
    column_plan: Vec<ColumnPlan>,
) -> Result<Box<BindState>, String> {
    // WHY: bind must validate input and run the kernel here, never in engine Rust.
    validate_invocation_input(klines, settings).map_err(|error| error.to_string())?;
    // WHY: DuckDB's function error is string-only; Display preserves typed kernel context.
    let frame = execute_standard_plan(klines, settings, projection, strategy)
        .map_err(MarketError::from)
        .map_err(|error| error.to_string())?;
    let open = klines.iter().map(|row| row.open).collect();
    let high = klines.iter().map(|row| row.high).collect();
    let low = klines.iter().map(|row| row.low).collect();
    let close = klines.iter().map(|row| row.close).collect();
    let volume = klines.iter().map(|row| row.volume).collect();
    let time = klines.iter().map(|row| row.time).collect();
    let adj_close = klines.iter().map(|row| row.adjclose).collect();
    Ok(Box::new(BindState {
        len: klines.len(),
        open,
        high,
        low,
        close,
        volume,
        time,
        adj_close,
        frame,
        column_plan,
    }))
}

fn add_result_columns(
    api: &DuckDbApi,
    info: DuckDbBindInfoHandle,
    plan: &[ColumnPlan],
) -> Result<(), String> {
    for column in plan {
        let ty = LogicalTypeGuard::new(api, column.type_id()).map_err(|error| error.message)?;
        let name = CString::new(column.name()).expect("static output name contains no NUL");
        unsafe { (api.duckdb_bind_add_result_column)(info, name.as_ptr(), ty.raw) };
    }
    Ok(())
}

unsafe extern "C" fn init(info: DuckDbInitInfoHandle) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let api = crate::engine::duckdb_ffi::duckdb_api().map_err(|error| error.message)?;
        if unsafe { (api.duckdb_init_get_bind_data)(info) }.is_null() {
            return Err("ta_indicator_frame init received null bind data".into());
        }
        unsafe {
            (api.duckdb_init_set_init_data)(
                info,
                Box::into_raw(Box::new(InitState { cursor: 0 })).cast(),
                destroy_box::<InitState>,
            )
        };
        Ok::<(), String>(())
    }));
    if let Err(message) =
        outcome.unwrap_or_else(|_| Err("ta_indicator_frame init callback panicked".into()))
    {
        set_init_error(info, &message);
    }
}

unsafe extern "C" fn function(info: DuckDbFunctionInfoHandle, output: DuckDbDataChunkHandle) {
    let outcome = catch_unwind(AssertUnwindSafe(|| function_inner(info, output)));
    if let Err(message) =
        outcome.unwrap_or_else(|_| Err("ta_indicator_frame function callback panicked".into()))
    {
        set_function_error(info, &message);
    }
}

fn function_inner(
    info: DuckDbFunctionInfoHandle,
    output: DuckDbDataChunkHandle,
) -> Result<(), String> {
    let api = crate::engine::duckdb_ffi::duckdb_api().map_err(|error| error.message)?;
    let api = api.as_ref();
    let bind = unsafe { &*((api.duckdb_function_get_bind_data)(info) as *const BindState) };
    let state = unsafe { &mut *((api.duckdb_function_get_init_data)(info) as *mut InitState) };
    let capacity = usize::try_from(unsafe { (api.duckdb_vector_size)() })
        .map_err(|_| "DuckDB vector capacity exceeds usize")?;
    let count = capacity.min(bind.len.saturating_sub(state.cursor));
    if count == 0 {
        unsafe { (api.duckdb_data_chunk_set_size)(output, 0) };
        return Ok(());
    }
    let base = state.cursor;

    for (column_index, column) in bind.column_plan.iter().enumerate() {
        match column {
            ColumnPlan::Open => {
                write_f64_slice(api, output, column_index, &bind.open[base..base + count])?
            }
            ColumnPlan::High => {
                write_f64_slice(api, output, column_index, &bind.high[base..base + count])?
            }
            ColumnPlan::Low => {
                write_f64_slice(api, output, column_index, &bind.low[base..base + count])?
            }
            ColumnPlan::Close => {
                write_f64_slice(api, output, column_index, &bind.close[base..base + count])?
            }
            ColumnPlan::Volume => {
                write_f64_slice(api, output, column_index, &bind.volume[base..base + count])?
            }
            ColumnPlan::Time => {
                write_i64_slice(api, output, column_index, &bind.time[base..base + count])?
            }
            ColumnPlan::AdjClose => write_optional_f64_slice(
                api,
                output,
                column_index,
                &bind.adj_close[base..base + count],
            )?,
            ColumnPlan::Indicator(IndicatorOutput::IsAtrGap) => write_optional_bool_slice(
                api,
                output,
                column_index,
                bool_slice(&bind.frame, column.name(), base, count)?,
            )?,
            ColumnPlan::Indicator(_) => write_optional_f64_slice(
                api,
                output,
                column_index,
                number_slice(&bind.frame, column.name(), base, count)?,
            )?,
        }
    }

    state.cursor += count;
    unsafe { (api.duckdb_data_chunk_set_size)(output, count as DuckDbIdx) };
    Ok(())
}

fn number_slice<'a>(
    frame: &'a IndicatorFrame,
    name: &str,
    base: usize,
    count: usize,
) -> Result<&'a [Option<f64>], String> {
    match frame.column(name) {
        Some(IndicatorColumn::Number(values)) => values
            .get(base..base + count)
            .ok_or_else(|| format!("indicator {name} rows are missing")),
        _ => Err(format!("indicator {name} is not numeric")),
    }
}
fn bool_slice<'a>(
    frame: &'a IndicatorFrame,
    name: &str,
    base: usize,
    count: usize,
) -> Result<&'a [Option<bool>], String> {
    match frame.column(name) {
        Some(IndicatorColumn::Boolean(values)) => values
            .get(base..base + count)
            .ok_or_else(|| format!("indicator {name} rows are missing")),
        _ => Err(format!("indicator {name} is not boolean")),
    }
}

/// Acquires a column vector and returns its data pointer exactly once per chunk.
fn chunk_vector(
    api: &DuckDbApi,
    chunk: DuckDbDataChunkHandle,
    column: usize,
) -> Result<*mut libc::c_void, String> {
    let vector = unsafe { (api.duckdb_data_chunk_get_vector)(chunk, column as DuckDbIdx) };
    if vector.is_null() {
        return Err("DuckDB returned null output vector".into());
    }
    let data = unsafe { (api.duckdb_vector_get_data)(vector) };
    if data.is_null() {
        return Err("DuckDB returned null output vector data".into());
    }
    Ok(data)
}

/// Allocates a writable validity mask and returns its pointer exactly once per chunk.
fn writable_validity(
    api: &DuckDbApi,
    chunk: DuckDbDataChunkHandle,
    column: usize,
) -> Result<*mut u64, String> {
    let vector = unsafe { (api.duckdb_data_chunk_get_vector)(chunk, column as DuckDbIdx) };
    if vector.is_null() {
        return Err("DuckDB returned null output vector".into());
    }
    unsafe { (api.duckdb_vector_ensure_validity_writable)(vector) };
    let validity = unsafe { (api.duckdb_vector_get_validity)(vector) };
    if validity.is_null() {
        return Err("DuckDB returned null writable validity mask".into());
    }
    Ok(validity)
}

/// Bulk-copies a contiguous, non-null f64 column (no validity mask allocated).
fn write_f64_slice(
    api: &DuckDbApi,
    chunk: DuckDbDataChunkHandle,
    column: usize,
    values: &[f64],
) -> Result<(), String> {
    let data = chunk_vector(api, chunk, column)?;
    unsafe { ptr::copy_nonoverlapping(values.as_ptr(), data.cast::<f64>(), values.len()) };
    Ok(())
}

/// Bulk-copies a contiguous, non-null i64 column (no validity mask allocated).
fn write_i64_slice(
    api: &DuckDbApi,
    chunk: DuckDbDataChunkHandle,
    column: usize,
    values: &[i64],
) -> Result<(), String> {
    let data = chunk_vector(api, chunk, column)?;
    unsafe { ptr::copy_nonoverlapping(values.as_ptr(), data.cast::<i64>(), values.len()) };
    Ok(())
}

/// Writes a nullable f64 column: one validity mask, one data pointer, per chunk.
fn write_optional_f64_slice(
    api: &DuckDbApi,
    chunk: DuckDbDataChunkHandle,
    column: usize,
    values: &[Option<f64>],
) -> Result<(), String> {
    let data = chunk_vector(api, chunk, column)?;
    let validity = writable_validity(api, chunk, column)?;
    unsafe { set_all_valid(validity, values.len()) };
    for (row, value) in values.iter().enumerate() {
        match value {
            Some(value) => unsafe { *data.cast::<f64>().add(row) = *value },
            None => unsafe { set_invalid(validity, row) },
        }
    }
    Ok(())
}

/// Writes a nullable bool column: one validity mask, one data pointer, per chunk.
fn write_optional_bool_slice(
    api: &DuckDbApi,
    chunk: DuckDbDataChunkHandle,
    column: usize,
    values: &[Option<bool>],
) -> Result<(), String> {
    let data = chunk_vector(api, chunk, column)?;
    let validity = writable_validity(api, chunk, column)?;
    unsafe { set_all_valid(validity, values.len()) };
    for (row, value) in values.iter().enumerate() {
        match value {
            Some(value) => unsafe { *data.cast::<bool>().add(row) = *value },
            None => unsafe { set_invalid(validity, row) },
        }
    }
    Ok(())
}

/// Marks rows `[0, count)` valid by writing the DuckDB validity bitset directly.
unsafe fn set_all_valid(mask: *mut u64, count: usize) {
    let words = count / 64;
    let remainder = count % 64;
    for word in 0..words {
        unsafe { *mask.add(word) = u64::MAX };
    }
    if remainder != 0 {
        unsafe { *mask.add(words) = (1u64 << remainder) - 1 };
    }
}

/// Clears a single validity bit, marking a row NULL.
unsafe fn set_invalid(mask: *mut u64, row: usize) {
    unsafe { *mask.add(row / 64) &= !(1u64 << (row % 64)) };
}

#[cfg(test)]
fn read_double_list(
    api: &DuckDbApi,
    info: DuckDbBindInfoHandle,
    index: usize,
    name: &str,
) -> Result<Vec<f64>, String> {
    read_optional_double_list(api, info, index, name)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            value.ok_or_else(|| format!("ta_indicator_frame {name}[{index}] must not be NULL"))
        })
        .collect()
}
#[cfg(test)]
fn read_optional_double_list(
    api: &DuckDbApi,
    info: DuckDbBindInfoHandle,
    index: usize,
    name: &str,
) -> Result<Vec<Option<f64>>, String> {
    let value = ValueGuard::parameter(api, info, index)?;
    if unsafe { (api.duckdb_is_null_value)(value.raw) } {
        return Err(format!("ta_indicator_frame {name} list must not be NULL"));
    }
    let size = unsafe { (api.duckdb_get_list_size)(value.raw) };
    (0..size)
        .map(|item| {
            let child = ValueGuard::child(api, value.raw, item)?;
            if unsafe { (api.duckdb_is_null_value)(child.raw) } {
                return Ok(None);
            }
            let number = unsafe { (api.duckdb_get_double)(child.raw) };
            if number.is_finite() {
                Ok(Some(number))
            } else {
                Err(format!("ta_indicator_frame {name}[{item}] must be finite"))
            }
        })
        .collect()
}
#[cfg(test)]
fn read_time_list(
    api: &DuckDbApi,
    info: DuckDbBindInfoHandle,
    index: usize,
) -> Result<Vec<i64>, String> {
    let value = ValueGuard::parameter(api, info, index)?;
    if unsafe { (api.duckdb_is_null_value)(value.raw) } {
        return Err("ta_indicator_frame time list must not be NULL".into());
    }
    let times = (0..unsafe { (api.duckdb_get_list_size)(value.raw) })
        .map(|item| {
            let child = ValueGuard::child(api, value.raw, item)?;
            if unsafe { (api.duckdb_is_null_value)(child.raw) } {
                Err(format!("ta_indicator_frame time[{item}] must not be NULL"))
            } else {
                Ok(unsafe { (api.duckdb_get_int64)(child.raw) })
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if times.windows(2).any(|pair| pair[1] <= pair[0]) {
        return Err("ta_indicator_frame time must be strictly increasing".into());
    }
    Ok(times)
}
#[cfg(test)]
fn read_positive_period(
    api: &DuckDbApi,
    info: DuckDbBindInfoHandle,
    index: usize,
) -> Result<usize, String> {
    let value = ValueGuard::parameter(api, info, index)?;
    let value = unsafe { (api.duckdb_get_int64)(value.raw) };
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("ta_indicator_frame period argument {index} must be positive"))
}

fn set_bind_error(info: DuckDbBindInfoHandle, message: &str) {
    if let Ok(api) = crate::engine::duckdb_ffi::duckdb_api()
        && let Ok(message) = CString::new(message)
    {
        unsafe { (api.duckdb_bind_set_error)(info, message.as_ptr()) };
    }
}
fn set_init_error(info: DuckDbInitInfoHandle, message: &str) {
    if let Ok(api) = crate::engine::duckdb_ffi::duckdb_api()
        && let Ok(message) = CString::new(message)
    {
        unsafe { (api.duckdb_init_set_error)(info, message.as_ptr()) };
    }
}
fn set_function_error(info: DuckDbFunctionInfoHandle, message: &str) {
    if let Ok(api) = crate::engine::duckdb_ffi::duckdb_api()
        && let Ok(message) = CString::new(message)
    {
        unsafe { (api.duckdb_function_set_error)(info, message.as_ptr()) };
    }
}
unsafe extern "C" fn destroy_box<T>(pointer: *mut libc::c_void) {
    if !pointer.is_null() {
        unsafe { drop(Box::from_raw(pointer.cast::<T>())) };
    }
}

/// Releases DuckDB's independent ownership of the extra-info `Arc`.
///
/// `register` transfers exactly one cloned raw `Arc` to DuckDB. The session
/// keeps its own `Arc`, so this callback never borrows state whose lifetime is
/// tied to descriptor destruction.
unsafe extern "C" fn destroy_extra_info(extra_info: *mut libc::c_void) {
    if !extra_info.is_null() {
        let invocation_slot = unsafe { Arc::from_raw(extra_info.cast::<InvocationSlot>()) };
        invocation_slot.record_extra_info_destructor();
    }
}

struct TableFunctionGuard<'a> {
    api: &'a DuckDbApi,
    raw: *mut crate::engine::duckdb_ffi::DuckDBTableFunction,
}
impl Drop for TableFunctionGuard<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { (self.api.duckdb_destroy_table_function)(&mut self.raw) };
        }
    }
}
struct LogicalTypeGuard<'a> {
    api: &'a DuckDbApi,
    raw: DuckDbLogicalTypeHandle,
}
impl<'a> LogicalTypeGuard<'a> {
    fn new(api: &'a DuckDbApi, ty: u32) -> Result<Self, MarketError> {
        Self::from_raw(api, unsafe { (api.duckdb_create_logical_type)(ty) })
    }
    fn from_raw(api: &'a DuckDbApi, raw: DuckDbLogicalTypeHandle) -> Result<Self, MarketError> {
        if raw.is_null() {
            Err(MarketError::data_access(
                "DuckDB failed to create logical type",
            ))
        } else {
            Ok(Self { api, raw })
        }
    }
}
impl Drop for LogicalTypeGuard<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.duckdb_destroy_logical_type)(&mut self.raw) };
    }
}
#[cfg(test)]
struct ValueGuard<'a> {
    api: &'a DuckDbApi,
    raw: crate::engine::duckdb_ffi::DuckDbValueHandle,
}
#[cfg(test)]
impl<'a> ValueGuard<'a> {
    fn parameter(
        api: &'a DuckDbApi,
        info: DuckDbBindInfoHandle,
        index: usize,
    ) -> Result<Self, String> {
        Self::from_raw(api, unsafe {
            (api.duckdb_bind_get_parameter)(info, index as DuckDbIdx)
        })
    }
    fn child(
        api: &'a DuckDbApi,
        parent: crate::engine::duckdb_ffi::DuckDbValueHandle,
        index: DuckDbIdx,
    ) -> Result<Self, String> {
        Self::from_raw(api, unsafe { (api.duckdb_get_list_child)(parent, index) })
    }
    fn from_raw(
        api: &'a DuckDbApi,
        raw: crate::engine::duckdb_ffi::DuckDbValueHandle,
    ) -> Result<Self, String> {
        if raw.is_null() {
            Err("DuckDB returned null table-function parameter value".into())
        } else {
            Ok(Self { api, raw })
        }
    }
}
#[cfg(test)]
impl Drop for ValueGuard<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.duckdb_destroy_value)(&mut self.raw) };
    }
}

/// Test-only literal SQL builder, retained solely for the baseline comparison.
#[cfg(test)]
pub(crate) fn ta_indicator_frame_literal_sql(
    klines: &[Kline],
    settings: IndicatorSettings,
) -> String {
    let list =
        |values: Vec<String>, type_name: &str| format!("[{}]::{type_name}[]", values.join(", "));
    let doubles = |values: Vec<f64>| {
        list(
            values.into_iter().map(|value| value.to_string()).collect(),
            "DOUBLE",
        )
    };
    let optional_doubles = klines
        .iter()
        .map(|row| {
            row.adjclose
                .map_or_else(|| "NULL".to_string(), |value| value.to_string())
        })
        .collect::<Vec<_>>();
    [
        doubles(klines.iter().map(|row| row.open).collect()),
        doubles(klines.iter().map(|row| row.high).collect()),
        doubles(klines.iter().map(|row| row.low).collect()),
        doubles(klines.iter().map(|row| row.close).collect()),
        doubles(klines.iter().map(|row| row.volume).collect()),
        list(
            klines.iter().map(|row| row.time.to_string()).collect(),
            "BIGINT",
        ),
        list(optional_doubles, "DOUBLE"),
        format!("{}::BIGINT", settings.volume_ema_period),
        format!("{}::BIGINT", settings.ema_period),
        format!("{}::BIGINT", settings.rsi_period),
        format!("{}::BIGINT", settings.rsi_smooth_period),
        format!("{}::BIGINT", settings.reverse_rsi_period),
        format!("{}::BIGINT", settings.atr_period),
        format!("{}::BIGINT", settings.bias_period),
        format!("{}::BIGINT", settings.structure_period),
        format!("{}::BIGINT", settings.structure_sma_period),
        format!("{}::BIGINT", settings.sharpe_period),
    ]
    .join(", ")
}

#[cfg(test)]
mod tests {
    use crate::engine::duckdb_ffi::duckdb_api;
    use crate::engine::duckdb_ta_table_function::TaInvocation;
    use crate::engine::traits::ComputedFrame;
    use crate::model::kline::Kline;
    use crate::ta::indicator::{
        IndicatorColumn, IndicatorFrame, IndicatorOutput, IndicatorProjection, IndicatorSettings,
    };
    use serde_json::Value;

    fn fixture() -> Vec<Kline> {
        (0..16)
            .map(|index| {
                let open = 100.0 + index as f64;
                Kline {
                    open,
                    high: if index == 4 { open } else { open + 3.0 },
                    low: if index == 4 { open } else { open - 2.0 },
                    close: if index == 4 {
                        open
                    } else {
                        open + if index % 2 == 0 { 1.0 } else { -1.0 }
                    },
                    volume: 1000.0 + index as f64,
                    time: 1_700_000_000_000 + index as i64 * 60_000,
                    adjclose: None,
                }
            })
            .collect()
    }

    #[test]
    fn invocation_rejects_non_monotonic_and_duplicate_times() {
        let settings = IndicatorSettings::default();
        let mut non_monotonic = fixture();
        non_monotonic.swap(5, 6);
        let non_monotonic_error = TaInvocation::new(non_monotonic, settings).err().unwrap();
        assert_eq!(
            non_monotonic_error.message,
            "klines must be in strictly increasing chronological order"
        );

        let mut duplicate = fixture();
        duplicate[6].time = duplicate[5].time;
        let duplicate_error = TaInvocation::new(duplicate, settings).err().unwrap();
        assert_eq!(
            duplicate_error.message,
            "klines must be in strictly increasing chronological order"
        );
    }

    #[test]
    #[ignore = "requires the installed DuckDB C runtime"]
    fn table_function_matches_indicator_kernel_and_reports_malformed_inputs() {
        let klines = fixture();
        let settings = IndicatorSettings {
            volume_ema_period: 2,
            ema_period: 3,
            rsi_period: 2,
            rsi_smooth_period: 2,
            reverse_rsi_period: 2,
            atr_period: 3,
            bias_period: 2,
            structure_period: 2,
            structure_sma_period: 2,
            sharpe_period: 3,
        };
        let expected = IndicatorFrame::compute(&klines, settings).unwrap();
        let output_sql = format!(
            "SELECT * FROM ta_indicator_frame({}) ORDER BY time",
            super::ta_indicator_frame_literal_sql(&klines, settings)
        );
        let actual: Vec<serde_json::Map<String, Value>> = serde_json::from_str(
            &duckdb_api()
                .unwrap()
                .query_to_json(
                    &crate::engine::duckdb_engine::TrustedEngineQuery::from_test_sql(output_sql),
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(actual.len(), klines.len());
        assert_eq!(
            actual[0].keys().count(),
            super::BASE_COLUMNS.len() + crate::ta::indicator::IndicatorOutput::all().count()
        );
        for (index, row) in actual.iter().enumerate() {
            assert_eq!(row["time"].as_i64(), Some(klines[index].time));
            for name in crate::ta::indicator::IndicatorOutput::all()
                .filter(|output| *output != crate::ta::indicator::IndicatorOutput::IsAtrGap)
                .map(crate::ta::indicator::IndicatorOutput::column_name)
            {
                match expected.column(name).unwrap() {
                    IndicatorColumn::Number(values) => match (row[name].as_f64(), values[index]) {
                        (Some(actual), Some(expected)) => {
                            assert!((actual - expected).abs() < 1e-12)
                        }
                        (actual, expected) => assert_eq!(actual, expected),
                    },
                    _ => unreachable!(),
                }
            }
            match expected.column("is_atr_gap").unwrap() {
                IndicatorColumn::Boolean(values) => {
                    assert_eq!(row["is_atr_gap"].as_bool(), values[index])
                }
                _ => unreachable!(),
            }
        }
        for invalid in [
            "SELECT * FROM ta_indicator_frame([1.0], [1.0,2.0], [1.0], [1.0], [1.0], [1], [NULL], 1,1,1,1,1,1,1,1,1,1)",
            "SELECT * FROM ta_indicator_frame([nan()], [1.0], [1.0], [1.0], [1.0], [1], [NULL], 1,1,1,1,1,1,1,1,1,1)",
            "SELECT * FROM ta_indicator_frame([1.0,2.0], [1.0,2.0], [1.0,2.0], [1.0,2.0], [1.0,2.0], [2,1], [NULL,NULL], 1,1,1,1,1,1,1,1,1,1)",
            "SELECT * FROM ta_indicator_frame([1.0], [1.0], [1.0], [1.0], [1.0], [1], [NULL], 0,1,1,1,1,1,1,1,1,1)",
        ] {
            assert!(
                duckdb_api()
                    .unwrap()
                    .query_to_json(
                        &crate::engine::duckdb_engine::TrustedEngineQuery::from_test_sql(
                            invalid.into()
                        )
                    )
                    .is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    #[ignore = "requires the installed DuckDB C runtime"]
    fn literal_registration_is_fresh_and_exactly_17_argument() {
        let api = duckdb_api().unwrap();
        let klines = fixture();
        let settings = IndicatorSettings::default();
        let sql = crate::engine::duckdb_engine::TrustedEngineQuery::from_test_sql(format!(
            "SELECT * FROM ta_indicator_frame({}) ORDER BY time",
            super::ta_indicator_frame_literal_sql(&klines, settings)
        ));
        let mut expected = None;

        for _ in 0..25 {
            let session = api.open_literal_session().unwrap();
            session.register_ta_literal().unwrap();
            let output = session.query_to_json_records(&sql).unwrap();
            if let Some(expected) = &expected {
                assert_eq!(&output, expected);
            } else {
                expected = Some(output);
            }
        }
    }

    #[test]
    #[ignore = "requires the installed DuckDB C runtime"]
    fn projected_dynamic_schema_writes_multiple_chunks_without_unrequested_columns() {
        let klines = (0..3_000)
            .map(|index| Kline {
                open: 100.0 + index as f64,
                high: 103.0 + index as f64,
                low: 98.0 + index as f64,
                close: 101.0 + index as f64,
                volume: 1_000.0 + index as f64,
                time: 1_700_000_000_000 + index as i64 * 60_000,
                adjclose: None,
            })
            .collect::<Vec<_>>();
        let projection = IndicatorProjection::selected([
            IndicatorOutput::Rssi,
            IndicatorOutput::RssiMa,
            IndicatorOutput::IsAtrGap,
        ]);
        let expected =
            IndicatorFrame::compute_projected(&klines, IndicatorSettings::default(), &projection)
                .unwrap();
        let mut session = duckdb_api().unwrap().open_session().unwrap();
        let actual = session
            .query_invocation_frame(
                TaInvocation::projected(klines.clone(), IndicatorSettings::default(), projection)
                    .unwrap(),
                &crate::engine::duckdb_engine::TrustedEngineQuery::from_test_sql(
                    "SELECT * FROM ta_indicator_frame() ORDER BY time".into(),
                ),
            )
            .unwrap();

        assert_eq!(actual.len(), klines.len());
        assert_eq!(
            actual.columns(),
            [
                "open",
                "high",
                "low",
                "close",
                "volume",
                "time",
                "adj_close",
                "rssi",
                "rssi_ma",
                "is_atr_gap"
            ]
        );
        assert!(!actual.has_column("atr"));
        for (index, expected_rssi) in match expected.column("rssi").unwrap() {
            IndicatorColumn::Number(values) => values,
            _ => unreachable!(),
        }
        .iter()
        .enumerate()
        {
            match (actual.f64_at("rssi", index).unwrap(), *expected_rssi) {
                (Some(actual), Some(expected)) => assert!((actual - expected).abs() <= 1e-12),
                (actual, expected) => assert_eq!(actual, expected),
            }
        }
    }
}

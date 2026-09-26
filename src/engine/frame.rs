//! Owned, domain-neutral result frames decoded from DuckDB queries.
//!
//! This module copies every result value before returning, so frames remain
//! usable after the statement, connection, and enclosing session are dropped.

use std::collections::HashSet;

use duckdb::arrow::datatypes::DataType;
use duckdb::types::Type;
use duckdb::{Connection, Row};
use serde_json::{Map, Number, Value};

use crate::engine::error::MarketError;
use crate::engine::traits::{ColumnDType, ComputedFrame};

/// Nullable column data exchanged across engine frame boundaries.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceColumnData {
    /// Nullable numeric column values.
    Number(Vec<Option<f64>>),
    /// Nullable boolean column values.
    Boolean(Vec<Option<bool>>),
    /// Nullable UTF-8 column values.
    Text(Vec<Option<String>>),
}

/// Columnar materialization of completed engine outputs.
#[derive(Debug, Clone)]
pub struct SourceFrame {
    len: usize,
    columns: Vec<(String, SourceColumnData)>,
}

impl SourceFrame {
    /// Creates a completed frame from typed columns with matching row counts.
    pub fn from_columns(columns: Vec<(String, SourceColumnData)>) -> Result<Self, MarketError> {
        for (name, _) in &columns {
            if name.trim().is_empty() {
                return Err(MarketError::validation(
                    "blank column name is not permitted",
                ));
            }
        }
        {
            let mut seen: HashSet<&str> = HashSet::with_capacity(columns.len());
            for (name, _) in &columns {
                if !seen.insert(name.as_str()) {
                    return Err(MarketError::validation(format!(
                        "duplicate column name {name}"
                    )));
                }
            }
        }
        let len = columns.first().map_or(0, |(_, column)| match column {
            SourceColumnData::Number(values) => values.len(),
            SourceColumnData::Boolean(values) => values.len(),
            SourceColumnData::Text(values) => values.len(),
        });
        if columns.iter().any(|(_, column)| match column {
            SourceColumnData::Number(values) => values.len() != len,
            SourceColumnData::Boolean(values) => values.len() != len,
            SourceColumnData::Text(values) => values.len() != len,
        }) {
            return Err(MarketError::computation(
                "completed frame columns have inconsistent row counts",
            ));
        }
        Ok(Self { len, columns })
    }

    /// Number of materialized rows.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the frame contains no rows.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns output column names in constructor insertion order.
    pub fn column_names(&self) -> Vec<&str> {
        self.columns.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// Returns one typed column by exact name, resolving to the first match.
    pub fn column(&self, name: &str) -> Option<&SourceColumnData> {
        self.columns
            .iter()
            .find_map(|(candidate, column)| (candidate == name).then_some(column))
    }

    fn column_at(&self, name: &str) -> Result<&SourceColumnData, MarketError> {
        self.column(name)
            .ok_or_else(|| MarketError::data_access(format!("column {name} not found")))
    }

    fn ensure_row(&self, row: usize) -> Result<(), MarketError> {
        if row >= self.len {
            return Err(MarketError::data_access(format!("row {row} out of bounds")));
        }
        Ok(())
    }
}

impl ComputedFrame for SourceFrame {
    fn len(&self) -> usize {
        self.len
    }

    fn columns(&self) -> Vec<String> {
        self.columns.iter().map(|(name, _)| name.clone()).collect()
    }

    fn column_dtypes(&self) -> Vec<(String, ColumnDType)> {
        self.columns
            .iter()
            .map(|(name, column)| {
                let dtype = match column {
                    SourceColumnData::Number(_) => ColumnDType::Number,
                    SourceColumnData::Boolean(_) => ColumnDType::Boolean,
                    SourceColumnData::Text(_) => ColumnDType::Text,
                };
                (name.clone(), dtype)
            })
            .collect()
    }

    fn slice_last(&self, count: usize) -> Result<Box<dyn ComputedFrame>, MarketError> {
        let start = self.len.saturating_sub(count);
        let columns = self
            .columns
            .iter()
            .map(|(name, column)| {
                let values = match column {
                    SourceColumnData::Number(values) => {
                        SourceColumnData::Number(values[start..].to_vec())
                    }
                    SourceColumnData::Boolean(values) => {
                        SourceColumnData::Boolean(values[start..].to_vec())
                    }
                    SourceColumnData::Text(values) => {
                        SourceColumnData::Text(values[start..].to_vec())
                    }
                };
                (name.clone(), values)
            })
            .collect();
        Ok(Box::new(Self {
            len: self.len - start,
            columns,
        }))
    }

    fn f64_at(&self, column: &str, row: usize) -> Result<Option<f64>, MarketError> {
        self.ensure_row(row)?;
        match self.column_at(column)? {
            SourceColumnData::Number(values) => Ok(values[row]),
            SourceColumnData::Boolean(_) | SourceColumnData::Text(_) => {
                Err(MarketError::data_access("column is not numeric"))
            }
        }
    }

    fn string_at(&self, column: &str, row: usize) -> Result<Option<String>, MarketError> {
        self.ensure_row(row)?;
        match self.column_at(column)? {
            SourceColumnData::Text(values) => Ok(values[row].clone()),
            SourceColumnData::Number(_) | SourceColumnData::Boolean(_) => {
                Err(MarketError::data_access("column is not UTF-8"))
            }
        }
    }

    fn to_json_records(&self) -> Result<Vec<Map<String, Value>>, MarketError> {
        let mut records = Vec::with_capacity(self.len);
        for row in 0..self.len {
            let mut record = Map::with_capacity(self.columns.len());
            for (name, column) in &self.columns {
                let value = match column {
                    SourceColumnData::Number(values) => match values[row] {
                        Some(value) => {
                            Number::from_f64(value).map(Value::Number).ok_or_else(|| {
                                MarketError::computation(
                                    "non-finite numeric output cannot be represented as JSON",
                                )
                            })?
                        }
                        None => Value::Null,
                    },
                    SourceColumnData::Boolean(values) => {
                        values[row].map_or(Value::Null, Value::Bool)
                    }
                    SourceColumnData::Text(values) => {
                        values[row].clone().map_or(Value::Null, Value::String)
                    }
                };
                record.insert(name.clone(), value);
            }
            records.push(record);
        }
        Ok(records)
    }

    fn has_column(&self, column: &str) -> bool {
        self.column(column).is_some()
    }
}

#[derive(Debug, Clone)]
enum QueryResultColumn {
    Null(Vec<Option<()>>),
    Float64(Vec<Option<f64>>),
    Int64(Vec<Option<i64>>),
    UInt64(Vec<Option<u64>>),
    Boolean(Vec<Option<bool>>),
    Utf8(Vec<Option<String>>),
}

impl QueryResultColumn {
    fn len(&self) -> usize {
        match self {
            Self::Null(values) => values.len(),
            Self::Float64(values) => values.len(),
            Self::Int64(values) => values.len(),
            Self::UInt64(values) => values.len(),
            Self::Boolean(values) => values.len(),
            Self::Utf8(values) => values.len(),
        }
    }

    fn push(&mut self, row: &Row<'_>, index: usize) -> Result<(), MarketError> {
        match self {
            Self::Null(values) => values.push(None),
            Self::Float64(values) => values.push(row.get(index).map_err(map_duckdb_error)?),
            Self::Int64(values) => values.push(row.get(index).map_err(map_duckdb_error)?),
            Self::UInt64(values) => values.push(row.get(index).map_err(map_duckdb_error)?),
            Self::Boolean(values) => values.push(row.get(index).map_err(map_duckdb_error)?),
            Self::Utf8(values) => values.push(row.get(index).map_err(map_duckdb_error)?),
        }
        Ok(())
    }

    fn slice_from(&self, start: usize) -> Self {
        match self {
            Self::Null(values) => Self::Null(values[start..].to_vec()),
            Self::Float64(values) => Self::Float64(values[start..].to_vec()),
            Self::Int64(values) => Self::Int64(values[start..].to_vec()),
            Self::UInt64(values) => Self::UInt64(values[start..].to_vec()),
            Self::Boolean(values) => Self::Boolean(values[start..].to_vec()),
            Self::Utf8(values) => Self::Utf8(values[start..].to_vec()),
        }
    }

    fn f64_at(&self, row: usize) -> Result<Option<f64>, MarketError> {
        match self {
            Self::Null(_) => Ok(None),
            Self::Float64(values) => Ok(values[row]),
            Self::Int64(values) => Ok(values[row].map(|value| value as f64)),
            Self::UInt64(values) => Ok(values[row].map(|value| value as f64)),
            _ => Err(MarketError::data_access("column is not numeric")),
        }
    }

    fn string_at(&self, row: usize) -> Result<Option<String>, MarketError> {
        match self {
            Self::Null(_) => Ok(None),
            Self::Utf8(values) => Ok(values[row].clone()),
            _ => Err(MarketError::data_access("column is not UTF-8")),
        }
    }

    fn json_value_at(&self, row: usize) -> Result<Value, MarketError> {
        match self {
            Self::Null(_) => Ok(Value::Null),
            Self::Float64(values) => match values[row] {
                Some(value) => Number::from_f64(value).map(Value::Number).ok_or_else(|| {
                    MarketError::computation("non-finite Float64 cannot be represented as JSON")
                }),
                None => Ok(Value::Null),
            },
            Self::Int64(values) => Ok(values[row].map_or(Value::Null, Value::from)),
            Self::UInt64(values) => Ok(values[row].map_or(Value::Null, Value::from)),
            Self::Boolean(values) => Ok(values[row].map_or(Value::Null, Value::Bool)),
            Self::Utf8(values) => Ok(values[row].clone().map_or(Value::Null, Value::String)),
        }
    }
}

#[derive(Clone, Copy)]
enum ColumnKind {
    Null,
    Float64,
    Int64,
    UInt64,
    Boolean,
    Utf8,
}

impl ColumnKind {
    fn from_declared_type(data_type: DataType, column: &str) -> Result<Self, MarketError> {
        match Type::from(&data_type) {
            Type::Null => Ok(Self::Null),
            Type::Double | Type::Float => Ok(Self::Float64),
            Type::BigInt | Type::Int => Ok(Self::Int64),
            Type::UBigInt | Type::UInt => Ok(Self::UInt64),
            Type::Boolean => Ok(Self::Boolean),
            Type::Text => Ok(Self::Utf8),
            unsupported => Err(MarketError::data_access(format!(
                "unsupported DuckDB result type for column {column}: {unsupported:?}"
            ))),
        }
    }

    fn empty_column(self) -> QueryResultColumn {
        match self {
            Self::Null => QueryResultColumn::Null(Vec::new()),
            Self::Float64 => QueryResultColumn::Float64(Vec::new()),
            Self::Int64 => QueryResultColumn::Int64(Vec::new()),
            Self::UInt64 => QueryResultColumn::UInt64(Vec::new()),
            Self::Boolean => QueryResultColumn::Boolean(Vec::new()),
            Self::Utf8 => QueryResultColumn::Utf8(Vec::new()),
        }
    }
}

/// Fully owned typed columns decoded from one DuckDB query result.
#[derive(Debug, Clone)]
pub struct QueryResultFrame {
    columns: Vec<String>,
    values: Vec<QueryResultColumn>,
    row_count: usize,
}

impl QueryResultFrame {
    /// Prepares and executes `sql`, copying supported DuckDB result values.
    ///
    /// SQL must already be trusted by the caller. Errors reported by DuckDB are
    /// converted to [`MarketError`] at this adapter boundary.
    pub fn from_connection(connection: &Connection, sql: &str) -> Result<Self, MarketError> {
        let mut statement = connection.prepare(sql).map_err(map_duckdb_error)?;
        let mut rows = statement.query([]).map_err(map_duckdb_error)?;
        let (columns, mut values) = {
            let statement = rows.as_ref().ok_or_else(|| {
                MarketError::invocation_lifecycle("query rows lost statement metadata")
            })?;
            let columns = statement.column_names();
            let values = columns
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    ColumnKind::from_declared_type(statement.column_type(index), column)
                })
                .map(|kind| kind.map(ColumnKind::empty_column))
                .collect::<Result<Vec<_>, _>>()?;
            (columns, values)
        };

        while let Some(row) = rows.next().map_err(map_duckdb_error)? {
            for (index, column) in values.iter_mut().enumerate() {
                column.push(row, index)?;
            }
        }

        Self::from_parts(columns, values)
    }

    fn from_parts(
        columns: Vec<String>,
        values: Vec<QueryResultColumn>,
    ) -> Result<Self, MarketError> {
        if columns.len() != values.len() {
            return Err(MarketError::computation(format!(
                "result has {} column names but {} decoded columns",
                columns.len(),
                values.len()
            )));
        }
        let row_count = values.first().map_or(0, QueryResultColumn::len);
        if values.iter().any(|column| column.len() != row_count) {
            return Err(MarketError::computation(
                "decoded columns have inconsistent row counts",
            ));
        }
        Ok(Self {
            columns,
            values,
            row_count,
        })
    }

    fn column_at(&self, column: &str) -> Result<&QueryResultColumn, MarketError> {
        self.columns
            .iter()
            .position(|candidate| candidate == column)
            .map(|index| &self.values[index])
            .ok_or_else(|| MarketError::data_access(format!("column {column} not found")))
    }

    fn ensure_row(&self, row: usize) -> Result<(), MarketError> {
        if row >= self.row_count {
            return Err(MarketError::data_access(format!("row {row} out of bounds")));
        }
        Ok(())
    }
}

impl ComputedFrame for QueryResultFrame {
    fn len(&self) -> usize {
        self.row_count
    }

    fn columns(&self) -> Vec<String> {
        self.columns.clone()
    }

    fn column_dtypes(&self) -> Vec<(String, ColumnDType)> {
        self.columns
            .iter()
            .zip(&self.values)
            .map(|(name, column)| {
                let dtype = match column {
                    QueryResultColumn::Float64(_)
                    | QueryResultColumn::Int64(_)
                    | QueryResultColumn::UInt64(_) => ColumnDType::Number,
                    QueryResultColumn::Boolean(_) => ColumnDType::Boolean,
                    QueryResultColumn::Utf8(_) => ColumnDType::Text,
                    QueryResultColumn::Null(_) => ColumnDType::Null,
                };
                (name.clone(), dtype)
            })
            .collect()
    }

    fn slice_last(&self, count: usize) -> Result<Box<dyn ComputedFrame>, MarketError> {
        let start = self.row_count.saturating_sub(count);
        Ok(Box::new(Self::from_parts(
            self.columns.clone(),
            self.values
                .iter()
                .map(|column| column.slice_from(start))
                .collect(),
        )?))
    }

    fn f64_at(&self, column: &str, row: usize) -> Result<Option<f64>, MarketError> {
        self.ensure_row(row)?;
        self.column_at(column)?.f64_at(row)
    }

    fn string_at(&self, column: &str, row: usize) -> Result<Option<String>, MarketError> {
        self.ensure_row(row)?;
        self.column_at(column)?.string_at(row)
    }

    fn to_json_records(&self) -> Result<Vec<Map<String, Value>>, MarketError> {
        let mut records = Vec::with_capacity(self.row_count);
        for row in 0..self.row_count {
            let mut record = Map::with_capacity(self.columns.len());
            for (column, values) in self.columns.iter().zip(&self.values) {
                record.insert(column.clone(), values.json_value_at(row)?);
            }
            records.push(record);
        }
        Ok(records)
    }

    fn has_column(&self, column: &str) -> bool {
        self.columns.iter().any(|candidate| candidate == column)
    }
}

fn map_duckdb_error(error: duckdb::Error) -> MarketError {
    MarketError::data_access(format!("DuckDB result access failed: {error}"))
}

#[cfg(test)]
mod tests {
    extern crate self as algotrap;

    use duckdb::Connection;
    use serde_json::json;

    use super::{QueryResultFrame, SourceColumnData, SourceFrame};
    use crate::engine::traits::ColumnDType;
    use crate::engine::{ComputedFrame, ErrorKind, MarketError};

    #[test]
    fn lists_mixed_source_column_dtypes_in_frame_order() {
        let frame = SourceFrame::from_columns(vec![
            ("text".into(), SourceColumnData::Text(vec![None])),
            ("number".into(), SourceColumnData::Number(vec![Some(1.0)])),
            (
                "boolean".into(),
                SourceColumnData::Boolean(vec![Some(true)]),
            ),
        ])
        .unwrap();

        let expected = vec![
            ("text".into(), ColumnDType::Text),
            ("number".into(), ColumnDType::Number),
            ("boolean".into(), ColumnDType::Boolean),
        ];
        assert_eq!(frame.column_dtypes(), expected);
        assert_eq!(frame.slice_last(0).unwrap().column_dtypes(), expected);
    }

    #[test]
    fn lists_projected_duckdb_column_dtypes_in_frame_order() {
        let frame = decode(
            "SELECT 'text'::VARCHAR AS text_value, true AS bool_value, \
             1.5::DOUBLE AS float_value, (-2)::BIGINT AS int_value, \
             3::UBIGINT AS uint_value, NULL AS null_value",
        );

        let expected = vec![
            ("text_value".into(), ColumnDType::Text),
            ("bool_value".into(), ColumnDType::Boolean),
            ("float_value".into(), ColumnDType::Number),
            ("int_value".into(), ColumnDType::Number),
            ("uint_value".into(), ColumnDType::Number),
            ("null_value".into(), ColumnDType::Number),
        ];
        assert_eq!(frame.column_dtypes(), expected);
        assert_eq!(frame.slice_last(0).unwrap().column_dtypes(), expected);
    }

    #[test]
    fn lists_decoded_null_column_dtype() {
        use super::QueryResultColumn;

        let frame = QueryResultFrame::from_parts(
            vec!["null_value".into()],
            vec![QueryResultColumn::Null(vec![None])],
        )
        .unwrap();

        assert_eq!(
            frame.column_dtypes(),
            vec![("null_value".into(), ColumnDType::Null)]
        );
    }

    #[test]
    fn exposes_engine_owned_paths_and_locked_constructor_signatures() {
        type SourceFrameConstructor =
            fn(Vec<(String, ModuleSourceColumnData)>) -> Result<ModuleSourceFrame, MarketError>;

        use algotrap::engine::frame::{
            QueryResultFrame as ModuleQueryResultFrame, SourceColumnData as ModuleSourceColumnData,
            SourceFrame as ModuleSourceFrame,
        };
        use algotrap::engine::{
            QueryResultFrame as ReexportQueryResultFrame,
            SourceColumnData as ReexportSourceColumnData, SourceFrame as ReexportSourceFrame,
        };

        let _: SourceFrameConstructor = ModuleSourceFrame::from_columns;
        let _: fn(&Connection, &str) -> Result<ModuleQueryResultFrame, MarketError> =
            ModuleQueryResultFrame::from_connection;

        let _: Option<ReexportSourceColumnData> = None;
        let _: Option<ReexportSourceFrame> = None;
        let _: Option<ReexportQueryResultFrame> = None;
    }

    #[test]
    fn output_and_owned_frames_implement_computed_frame_send_and_sync() {
        fn assert_computed_frame_send_and_sync<T: ComputedFrame + Send + Sync>() {}

        assert_computed_frame_send_and_sync::<SourceFrame>();
        assert_computed_frame_send_and_sync::<QueryResultFrame>();
    }

    #[test]
    fn accepts_empty_and_named_zero_length_output_schemas() {
        let empty = SourceFrame::from_columns(vec![]).expect("empty schema must be valid");
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
        assert_eq!(empty.column_names(), Vec::<&str>::new());
        assert!(empty.to_json_records().unwrap().is_empty());

        let schema_only = SourceFrame::from_columns(vec![
            ("number".into(), SourceColumnData::Number(vec![])),
            ("boolean".into(), SourceColumnData::Boolean(vec![])),
            ("text".into(), SourceColumnData::Text(vec![])),
        ])
        .expect("zero-length columns must be valid");
        assert_eq!(schema_only.len(), 0);
        assert!(schema_only.is_empty());
        assert_eq!(
            schema_only.column_names(),
            vec!["number", "boolean", "text"]
        );
        assert!(schema_only.to_json_records().unwrap().is_empty());
    }

    #[test]
    fn rejects_blank_and_duplicate_output_column_names() {
        for blank_name in ["", " ", "\t\n"] {
            let error = SourceFrame::from_columns(vec![(
                blank_name.into(),
                SourceColumnData::Number(vec![Some(1.0)]),
            )])
            .expect_err("blank names must be rejected");
            assert_eq!(error.kind, ErrorKind::ValidationError);
            assert!(error.message.contains("blank"));
        }

        let error = SourceFrame::from_columns(vec![
            ("value".into(), SourceColumnData::Number(vec![Some(1.0)])),
            ("value".into(), SourceColumnData::Boolean(vec![Some(true)])),
        ])
        .expect_err("exact duplicate names must be rejected");
        assert_eq!(error.kind, ErrorKind::ValidationError);

        let distinct = SourceFrame::from_columns(vec![
            (" value ".into(), SourceColumnData::Number(vec![Some(1.0)])),
            ("value".into(), SourceColumnData::Boolean(vec![Some(true)])),
        ])
        .expect("names must remain exact and untrimmed");
        assert_eq!(distinct.column_names(), vec![" value ", "value"]);
    }

    #[test]
    fn rejects_mixed_variant_inconsistent_lengths() {
        let error = SourceFrame::from_columns(vec![
            (
                "number".into(),
                SourceColumnData::Number(vec![Some(1.0), None]),
            ),
            (
                "boolean".into(),
                SourceColumnData::Boolean(vec![Some(true)]),
            ),
            (
                "text".into(),
                SourceColumnData::Text(vec![Some("value".into()), None]),
            ),
        ])
        .expect_err("all typed columns must have the same row count");

        assert_eq!(error.kind, ErrorKind::ComputationError);
    }

    #[test]
    fn preserves_mixed_column_order_nulls_and_access() {
        let frame = SourceFrame::from_columns(vec![
            (
                "number".into(),
                SourceColumnData::Number(vec![Some(1.5), None]),
            ),
            (
                "boolean".into(),
                SourceColumnData::Boolean(vec![Some(true), None]),
            ),
            (
                "text".into(),
                SourceColumnData::Text(vec![Some("first".into()), None]),
            ),
        ])
        .expect("equal-length mixed columns must be valid");

        assert_eq!(frame.len(), 2);
        assert!(!frame.is_empty());
        assert_eq!(frame.column_names(), vec!["number", "boolean", "text"]);
        assert!(matches!(
            frame.column("number"),
            Some(SourceColumnData::Number(_))
        ));
        assert!(matches!(
            frame.column("boolean"),
            Some(SourceColumnData::Boolean(_))
        ));
        assert!(matches!(
            frame.column("text"),
            Some(SourceColumnData::Text(_))
        ));
        assert_eq!(frame.f64_at("number", 0).unwrap(), Some(1.5));
        assert_eq!(frame.f64_at("number", 1).unwrap(), None);
        assert_eq!(
            frame.string_at("text", 0).unwrap().as_deref(),
            Some("first")
        );
        assert_eq!(frame.string_at("text", 1).unwrap(), None);
    }

    #[test]
    fn slices_mixed_columns_with_saturating_count() {
        let frame = SourceFrame::from_columns(vec![
            (
                "number".into(),
                SourceColumnData::Number(vec![Some(1.0), None, Some(3.0)]),
            ),
            (
                "boolean".into(),
                SourceColumnData::Boolean(vec![Some(false), None, Some(true)]),
            ),
            (
                "text".into(),
                SourceColumnData::Text(vec![Some("one".into()), None, Some("three".into())]),
            ),
        ])
        .expect("equal-length mixed columns must be valid");

        let tail = frame.slice_last(2).unwrap();
        assert_eq!(tail.columns(), vec!["number", "boolean", "text"]);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail.f64_at("number", 0).unwrap(), None);
        assert_eq!(tail.string_at("text", 0).unwrap(), None);
        assert_eq!(tail.string_at("text", 1).unwrap().as_deref(), Some("three"));
        assert_eq!(tail.slice_last(usize::MAX).unwrap().len(), 2);
    }

    #[test]
    fn serializes_mixed_columns_and_rejects_non_finite_numbers() {
        let frame = SourceFrame::from_columns(vec![
            (
                "number".into(),
                SourceColumnData::Number(vec![Some(1.5), None]),
            ),
            (
                "boolean".into(),
                SourceColumnData::Boolean(vec![Some(false), None]),
            ),
            (
                "text".into(),
                SourceColumnData::Text(vec![Some("value".into()), None]),
            ),
        ])
        .expect("finite values must be valid");
        assert_eq!(
            frame.to_json_records().unwrap(),
            vec![
                serde_json::Map::from_iter([
                    ("number".into(), json!(1.5)),
                    ("boolean".into(), json!(false)),
                    ("text".into(), json!("value")),
                ]),
                serde_json::Map::from_iter([
                    ("number".into(), json!(null)),
                    ("boolean".into(), json!(null)),
                    ("text".into(), json!(null)),
                ]),
            ]
        );

        let non_finite = SourceFrame::from_columns(vec![(
            "number".into(),
            SourceColumnData::Number(vec![Some(f64::NAN)]),
        )])
        .expect("non-finite values are accepted at construction");
        let error = non_finite
            .to_json_records()
            .expect_err("non-finite values cannot be represented as JSON");
        assert_eq!(error.kind, ErrorKind::ComputationError);
    }

    #[test]
    fn reports_data_access_errors_for_missing_wrong_type_and_out_of_bounds_output_access() {
        let frame = SourceFrame::from_columns(vec![
            ("number".into(), SourceColumnData::Number(vec![Some(1.0)])),
            (
                "boolean".into(),
                SourceColumnData::Boolean(vec![Some(true)]),
            ),
            (
                "text".into(),
                SourceColumnData::Text(vec![Some("value".into())]),
            ),
        ])
        .expect("one-row mixed columns must be valid");

        for error in [
            frame.f64_at("boolean", 0).map(|_| ()),
            frame.f64_at("text", 0).map(|_| ()),
            frame.string_at("number", 0).map(|_| ()),
            frame.string_at("boolean", 0).map(|_| ()),
            frame.f64_at("missing", 0).map(|_| ()),
            frame.f64_at("number", 1).map(|_| ()),
        ] {
            assert_eq!(
                error.expect_err("invalid output access must fail").kind,
                ErrorKind::DataAccessError
            );
        }
        assert!(frame.has_column("number"));
        assert!(!frame.has_column("missing"));
    }

    fn decode(sql: &str) -> QueryResultFrame {
        let connection = Connection::open_in_memory().expect("test connection must open");
        QueryResultFrame::from_connection(&connection, sql).expect("query must decode")
    }

    #[test]
    fn decodes_supported_types_with_nulls_at_edges_and_middle() {
        let frame = decode(
            "SELECT * FROM (VALUES \
                (CAST(NULL AS DOUBLE), CAST(NULL AS BIGINT), CAST(NULL AS UBIGINT), CAST(NULL AS BOOLEAN), CAST(NULL AS VARCHAR)), \
                (1.5, CAST(-2 AS BIGINT), CAST(3 AS UBIGINT), true, 'middle'), \
                (CAST(NULL AS DOUBLE), CAST(NULL AS BIGINT), CAST(NULL AS UBIGINT), CAST(NULL AS BOOLEAN), CAST(NULL AS VARCHAR)) \
            ) AS values_table(float_value, int_value, uint_value, bool_value, text_value)",
        );

        assert_eq!(frame.len(), 3);
        assert_eq!(
            frame.columns(),
            vec![
                "float_value",
                "int_value",
                "uint_value",
                "bool_value",
                "text_value"
            ]
        );
        assert_eq!(frame.f64_at("float_value", 0).unwrap(), None);
        assert_eq!(frame.f64_at("float_value", 1).unwrap(), Some(1.5));
        assert_eq!(frame.f64_at("int_value", 1).unwrap(), Some(-2.0));
        assert_eq!(frame.f64_at("uint_value", 1).unwrap(), Some(3.0));
        assert_eq!(frame.string_at("text_value", 0).unwrap(), None);
        assert_eq!(
            frame.string_at("text_value", 1).unwrap().as_deref(),
            Some("middle")
        );
        assert_eq!(frame.string_at("text_value", 2).unwrap(), None);
    }

    #[test]
    fn decodes_result_larger_than_one_duckdb_chunk() {
        let frame =
            decode("SELECT i, i::DOUBLE AS float_value, NULL AS null_value FROM range(5000) t(i)");

        assert_eq!(frame.len(), 5_000);
        assert_eq!(frame.f64_at("i", 0).unwrap(), Some(0.0));
        assert_eq!(frame.f64_at("float_value", 4_999).unwrap(), Some(4_999.0));
        assert_eq!(frame.f64_at("null_value", 2_048).unwrap(), None);
    }

    #[test]
    fn slices_last_rows_with_saturating_count() {
        let frame = decode("SELECT i::BIGINT AS value FROM range(3) t(i)");

        let all_rows = frame.slice_last(10).unwrap();
        let final_rows = frame.slice_last(2).unwrap();
        assert_eq!(all_rows.len(), 3);
        assert_eq!(final_rows.len(), 2);
        assert_eq!(final_rows.f64_at("value", 0).unwrap(), Some(1.0));
        assert_eq!(final_rows.f64_at("value", 1).unwrap(), Some(2.0));
    }

    #[test]
    fn reports_wrong_type_missing_column_and_out_of_bounds_access() {
        let frame = decode("SELECT CAST(1.5 AS DOUBLE) AS numeric_value, 'text' AS text_value");

        assert!(frame.string_at("numeric_value", 0).is_err());
        assert!(frame.f64_at("text_value", 0).is_err());
        assert!(frame.f64_at("missing_value", 0).is_err());
        assert!(frame.f64_at("numeric_value", 1).is_err());
        assert!(frame.has_column("numeric_value"));
        assert!(!frame.has_column("missing_value"));
    }

    #[test]
    fn json_records_preserve_shape_values_and_nulls() {
        let frame = decode(
            "SELECT CAST(1.5 AS DOUBLE) AS float_value, CAST(-2 AS BIGINT) AS int_value, \
                CAST(3 AS UBIGINT) AS uint_value, true AS bool_value, 'text' AS text_value, \
                CAST(NULL AS VARCHAR) AS null_value",
        );

        assert_eq!(
            frame.to_json_records().unwrap(),
            vec![serde_json::Map::from_iter([
                ("float_value".into(), json!(1.5)),
                ("int_value".into(), json!(-2)),
                ("uint_value".into(), json!(3)),
                ("bool_value".into(), json!(true)),
                ("text_value".into(), json!("text")),
                ("null_value".into(), json!(null)),
            ])]
        );
    }

    #[test]
    fn rejects_unsupported_result_type() {
        let connection = Connection::open_in_memory().expect("test connection must open");
        let error = QueryResultFrame::from_connection(
            &connection,
            "SELECT DATE '2026-01-01' AS date_value",
        )
        .expect_err("DATE must not decode into the owned MVP frame");

        assert!(error.message.contains("unsupported DuckDB result type"));
    }

    #[test]
    fn preserves_duplicate_decoded_names_with_first_match_lookup() {
        use super::{QueryResultColumn, QueryResultFrame};

        let frame = QueryResultFrame::from_parts(
            vec!["value".to_owned(), "value".to_owned()],
            vec![
                QueryResultColumn::Float64(vec![Some(1.0)]),
                QueryResultColumn::Float64(vec![Some(2.0)]),
            ],
        )
        .expect("duplicate decoded names must be preserved");

        assert_eq!(
            frame.columns(),
            vec!["value".to_owned(), "value".to_owned()]
        );
        assert_eq!(frame.len(), 1);
        assert!(frame.has_column("value"));
        assert!(!frame.has_column("missing"));
        assert_eq!(frame.f64_at("value", 0).unwrap(), Some(1.0));
        let tail = frame.slice_last(1).unwrap();
        assert_eq!(tail.columns(), vec!["value".to_owned(), "value".to_owned()]);
        assert_eq!(tail.f64_at("value", 0).unwrap(), Some(1.0));
    }
}

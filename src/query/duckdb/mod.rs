//! DuckDB projection adapter for completed stream frames.
//!
//! # `project` lifecycle for newcomers
//!
//! Each call moves one finished [`SourceFrame`] in and returns a new owned
//! [`QueryResultFrame`], in three nested scopes:
//!
//! 1. `session::with_session` borrows a thread-local in-memory DuckDB
//!    [`Connection`](duckdb::Connection) (opening it on first use, reusing it
//!    for nested calls on the same thread).
//! 2. `vtab::register` installs the ephemeral `computed()` table function once
//!    per connection; it scans only the active frame, never global state.
//! 3. `invocation::with_frame` parks the frame in a thread-local slot so the
//!    DuckDB scan callback can reach it, runs `SELECT … FROM computed()`, then
//!    clears the slot via RAII even on error. `bind` reclaims sole ownership,
//!    `init` pins the scan to one thread, and `func` streams the rows chunk by
//!    chunk into the owned result.

mod invocation;
mod session;
mod vtab;

use std::sync::Arc;

use crate::engine::error::MarketError;
use crate::engine::frame::{QueryResultFrame, SourceFrame};
use crate::engine::traits::ComputedFrame;
use crate::query::{FrameQuery, RawQuery};

/// Executes source-controlled raw DuckDB projections over a completed [`SourceFrame`].
///
/// The adapter only scans frame values supplied by its caller. It does not own
/// aggregate execution, market input, or upstream computation lifecycle.
///
/// Why unit-sized: there is no connection pool or cache to manage — all shared
/// state lives in thread-local session/invocation slots, so a stateless
/// `DuckDBQuery` value is enough to run any number of sequential projections.
#[derive(Debug, Default)]
pub struct DuckDBQuery;

impl DuckDBQuery {
    /// Creates a DuckDB-backed frame projection adapter.
    /// `const` and field-free on purpose: every call borrows thread-local
    /// state instead of `self`, so construction never opens a connection.
    pub const fn new() -> Self {
        Self
    }

    /// Executes raw SQL against a completed frame registered as `computed()`.
    /// Ownership flow: `frame` is moved into an `Arc`, parked for the scan
    /// callback, reclaimed by `bind`, and finally materialized into a new
    /// `QueryResultFrame` by running `query.sql()`. Nesting two `project` calls on
    /// the same thread errors instead of mixing their frames.
    pub fn project(
        &self,
        frame: SourceFrame,
        query: RawQuery,
    ) -> Result<QueryResultFrame, MarketError> {
        session::with_session(|connection| {
            vtab::register(connection)?;
            invocation::with_frame(Arc::new(frame), || {
                QueryResultFrame::from_connection(connection, query.sql())
            })
        })
    }
}

impl FrameQuery for DuckDBQuery {
    fn query(
        &self,
        frame: SourceFrame,
        query: RawQuery,
    ) -> Result<Box<dyn ComputedFrame>, MarketError> {
        self.project(frame, query)
            .map(|frame| Box::new(frame) as Box<dyn ComputedFrame>)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::engine::error::ErrorKind;
    use crate::engine::frame::{SourceColumnData, SourceFrame};
    use crate::engine::traits::ComputedFrame;
    use crate::query::duckdb::DuckDBQuery;
    use crate::query::{FrameQuery, RawQuery};
    use serde_json::json;

    #[test]
    fn projects_completed_number_and_boolean_values_to_json() {
        let frame = SourceFrame::from_columns(vec![
            (
                "metric".into(),
                SourceColumnData::Number(vec![Some(1.5), None, Some(3.0)]),
            ),
            (
                "signal".into(),
                SourceColumnData::Boolean(vec![Some(false), None, Some(true)]),
            ),
        ])
        .unwrap();

        let result = DuckDBQuery::new()
            .query(
                frame,
                RawQuery::source_controlled(
                    "SELECT metric, signal FROM computed() WHERE metric IS NULL OR metric >= 1.5 ORDER BY metric NULLS LAST",
                ),
            )
            .unwrap();

        assert_eq!(result.columns(), vec!["metric", "signal"]);
        assert_eq!(
            result.to_json_records().unwrap(),
            vec![
                serde_json::Map::from_iter([
                    ("metric".into(), json!(1.5)),
                    ("signal".into(), json!(false)),
                ]),
                serde_json::Map::from_iter([
                    ("metric".into(), json!(3.0)),
                    ("signal".into(), json!(true)),
                ]),
                serde_json::Map::from_iter([
                    ("metric".into(), json!(null)),
                    ("signal".into(), json!(null)),
                ]),
            ]
        );
    }

    #[test]
    fn projects_mixed_text_number_boolean_with_nulls_in_source_order() {
        let frame = SourceFrame::from_columns(vec![
            (
                "label".into(),
                SourceColumnData::Text(vec![
                    Some("alpha".to_owned()),
                    None,
                    Some("gamma".to_owned()),
                ]),
            ),
            (
                "metric".into(),
                SourceColumnData::Number(vec![Some(1.5), None, Some(3.0)]),
            ),
            (
                "signal".into(),
                SourceColumnData::Boolean(vec![Some(true), None, Some(false)]),
            ),
        ])
        .unwrap();

        let result = DuckDBQuery::new()
            .project(
                frame,
                RawQuery::source_controlled("SELECT label, metric, signal FROM computed()"),
            )
            .unwrap();

        assert_eq!(result.columns(), vec!["label", "metric", "signal"]);
        assert_eq!(result.len(), 3);
        assert_eq!(
            result.string_at("label", 0).unwrap().as_deref(),
            Some("alpha")
        );
        assert_eq!(result.string_at("label", 1).unwrap(), None);
        assert_eq!(
            result.string_at("label", 2).unwrap().as_deref(),
            Some("gamma")
        );
        assert_eq!(result.f64_at("metric", 0).unwrap(), Some(1.5));
        assert_eq!(result.f64_at("metric", 1).unwrap(), None);
        assert_eq!(result.f64_at("metric", 2).unwrap(), Some(3.0));
        assert_eq!(
            result.to_json_records().unwrap(),
            vec![
                serde_json::Map::from_iter([
                    ("label".into(), json!("alpha")),
                    ("metric".into(), json!(1.5)),
                    ("signal".into(), json!(true)),
                ]),
                serde_json::Map::from_iter([
                    ("label".into(), json!(null)),
                    ("metric".into(), json!(null)),
                    ("signal".into(), json!(null)),
                ]),
                serde_json::Map::from_iter([
                    ("label".into(), json!("gamma")),
                    ("metric".into(), json!(3.0)),
                    ("signal".into(), json!(false)),
                ]),
            ]
        );
    }

    #[test]
    fn preserves_source_insertion_order_without_sql_reorder() {
        let frame = SourceFrame::from_columns(vec![
            (
                "signal".into(),
                SourceColumnData::Boolean(vec![Some(true), Some(false)]),
            ),
            (
                "label".into(),
                SourceColumnData::Text(vec![Some("one".to_owned()), Some("two".to_owned())]),
            ),
            (
                "metric".into(),
                SourceColumnData::Number(vec![Some(1.0), Some(2.0)]),
            ),
        ])
        .unwrap();

        let result = DuckDBQuery::new()
            .project(
                frame,
                RawQuery::source_controlled("SELECT * FROM computed()"),
            )
            .unwrap();

        assert_eq!(result.columns(), vec!["signal", "label", "metric"]);
        assert_eq!(
            result.to_json_records().unwrap(),
            vec![
                serde_json::Map::from_iter([
                    ("signal".into(), json!(true)),
                    ("label".into(), json!("one")),
                    ("metric".into(), json!(1.0)),
                ]),
                serde_json::Map::from_iter([
                    ("signal".into(), json!(false)),
                    ("label".into(), json!("two")),
                    ("metric".into(), json!(2.0)),
                ]),
            ]
        );
    }

    #[test]
    fn reflects_sql_reorder_and_rename_in_query_order() {
        let frame = SourceFrame::from_columns(vec![
            (
                "metric".into(),
                SourceColumnData::Number(vec![Some(1.0), Some(2.0)]),
            ),
            (
                "signal".into(),
                SourceColumnData::Boolean(vec![Some(false), Some(true)]),
            ),
            (
                "label".into(),
                SourceColumnData::Text(vec![Some("one".to_owned()), Some("two".to_owned())]),
            ),
        ])
        .unwrap();

        let result = DuckDBQuery::new()
            .project(
                frame,
                RawQuery::source_controlled(
                    "SELECT signal AS flag, label AS name, metric AS value FROM computed()",
                ),
            )
            .unwrap();

        assert_eq!(result.columns(), vec!["flag", "name", "value"]);
        assert!(result.has_column("flag"));
        assert!(result.has_column("name"));
        assert!(result.has_column("value"));
        assert!(!result.has_column("metric"));
        assert_eq!(result.f64_at("value", 0).unwrap(), Some(1.0));
        assert_eq!(result.f64_at("value", 1).unwrap(), Some(2.0));
        assert_eq!(result.string_at("name", 0).unwrap().as_deref(), Some("one"));
        assert_eq!(result.string_at("name", 1).unwrap().as_deref(), Some("two"));
        assert_eq!(
            result.to_json_records().unwrap(),
            vec![
                serde_json::Map::from_iter([
                    ("flag".into(), json!(false)),
                    ("name".into(), json!("one")),
                    ("value".into(), json!(1.0)),
                ]),
                serde_json::Map::from_iter([
                    ("flag".into(), json!(true)),
                    ("name".into(), json!("two")),
                    ("value".into(), json!(2.0)),
                ]),
            ]
        );
    }

    #[test]
    fn scans_mixed_frame_across_multiple_duckdb_chunks() {
        let row_count = 5_000usize;
        let numbers = (0..row_count)
            .map(|index| {
                if index % 7 == 0 {
                    None
                } else {
                    Some(index as f64)
                }
            })
            .collect::<Vec<_>>();
        let booleans = (0..row_count)
            .map(|index| {
                if index % 11 == 0 {
                    None
                } else {
                    Some(index % 2 == 0)
                }
            })
            .collect::<Vec<_>>();
        let texts = (0..row_count)
            .map(|index| {
                if index % 13 == 0 {
                    None
                } else {
                    Some(format!("row-{index:04}"))
                }
            })
            .collect::<Vec<_>>();
        let frame = SourceFrame::from_columns(vec![
            ("metric".into(), SourceColumnData::Number(numbers)),
            ("signal".into(), SourceColumnData::Boolean(booleans)),
            ("label".into(), SourceColumnData::Text(texts)),
        ])
        .unwrap();

        let result = DuckDBQuery::new()
            .project(
                frame,
                RawQuery::source_controlled("SELECT metric, signal, label FROM computed()"),
            )
            .unwrap();

        assert_eq!(result.columns(), vec!["metric", "signal", "label"]);
        assert_eq!(result.len(), row_count);
        assert_eq!(result.f64_at("metric", 0).unwrap(), None);
        assert_eq!(result.f64_at("metric", 1).unwrap(), Some(1.0));
        assert_eq!(result.f64_at("metric", 2_044).unwrap(), None);
        assert_eq!(result.f64_at("metric", 2_047).unwrap(), Some(2_047.0));
        assert_eq!(result.f64_at("metric", 2_048).unwrap(), Some(2_048.0));
        assert_eq!(result.f64_at("metric", 4_999).unwrap(), Some(4_999.0));
        assert_eq!(
            result.string_at("label", 1).unwrap().as_deref(),
            Some("row-0001")
        );
        assert_eq!(
            result.string_at("label", 2_047).unwrap().as_deref(),
            Some("row-2047")
        );
        assert_eq!(
            result.string_at("label", 4_999).unwrap().as_deref(),
            Some("row-4999")
        );
        assert_eq!(result.to_json_records().unwrap().len(), row_count);
    }

    #[test]
    fn scans_empty_typed_schema_as_zero_rows() {
        let frame = SourceFrame::from_columns(vec![
            ("metric".into(), SourceColumnData::Number(vec![])),
            ("signal".into(), SourceColumnData::Boolean(vec![])),
            ("label".into(), SourceColumnData::Text(vec![])),
        ])
        .unwrap();

        let result = DuckDBQuery::new()
            .project(
                frame,
                RawQuery::source_controlled("SELECT metric, signal, label FROM computed()"),
            )
            .unwrap();

        assert_eq!(result.len(), 0);
        assert!(result.is_empty());
        assert_eq!(result.columns(), vec!["metric", "signal", "label"]);
        assert!(result.to_json_records().unwrap().is_empty());
    }

    #[test]
    fn preserves_single_thread_invocation_lifecycle() {
        let make_frame = || {
            SourceFrame::from_columns(vec![(
                "metric".into(),
                SourceColumnData::Number(vec![Some(1.0)]),
            )])
            .unwrap()
        };

        let first = DuckDBQuery::new()
            .project(
                make_frame(),
                RawQuery::source_controlled("SELECT metric FROM computed()"),
            )
            .unwrap();
        assert_eq!(first.len(), 1);
        let second = DuckDBQuery::new()
            .project(
                make_frame(),
                RawQuery::source_controlled("SELECT metric FROM computed()"),
            )
            .unwrap();
        assert_eq!(second.len(), 1);

        let outer = Arc::new(make_frame());
        let error = super::invocation::with_frame(outer, || {
            DuckDBQuery::new().project(
                make_frame(),
                RawQuery::source_controlled("SELECT metric FROM computed()"),
            )
        })
        .expect_err("nested frame query must fail");
        assert_eq!(error.kind, ErrorKind::InvocationLifecycleError);
    }

    #[test]
    fn computed_table_rejects_arguments_without_a_compute_lifecycle() {
        let frame = SourceFrame::from_columns(vec![(
            "metric".into(),
            SourceColumnData::Number(vec![Some(1.0)]),
        )])
        .unwrap();

        let error = DuckDBQuery::new()
            .project(
                frame,
                RawQuery::source_controlled("SELECT * FROM computed(1)"),
            )
            .expect_err("computed table accepts no arguments");

        assert!(error.message.contains("computed"));
    }
}

//! Projection boundaries over already-computed frames.
//!
//! # Ownership and trust for newcomers
//!
//! - A query never computes market data itself: the caller moves a finished
//!   [`SourceFrame`] into the adapter, and the adapter returns a brand-new owned
//!   result frame. Nothing is borrowed across the call and no frame/result data
//!   is cached (the DuckDB adapter may reuse a thread-local connection).
//! - SQL is trusted, not sanitized: [`RawQuery`] only wraps text the application
//!   already owns and reviews in source control (see
//!   [`RawQuery::source_controlled`]). Never build it from end-user input.
//! - [`FrameQuery`] is the public seam: DuckDB projection and gap-zone analysis
//!   both flow through this one method, so a test fake only needs to implement
//!   [`FrameQuery::query`].

/// DuckDB-backed frame projection adapter.
/// Newcomer note: this backend owns nothing between calls — each `project`
/// opens (or reuses) a thread-local connection, scans the moved-in frame
/// through an ephemeral `computed()` table, and returns an owned copy.
pub mod duckdb;

/// Ephemeral recent-gap-zone relation over completed source frames.
/// Newcomer note: this is a read-only view built on top of [`FrameQuery`] —
/// it takes the source frame, runs source-controlled SQL, then decodes the
/// result rows into plain Rust structs without persisting anything.
pub mod gap_zones;

use crate::engine::error::MarketError;
use crate::engine::frame::SourceFrame;
use crate::engine::traits::ComputedFrame;

/// Source-controlled raw SQL accepted as-is by a query adapter.
///
/// This type does not sanitize, validate, or parameterize SQL. Callers must
/// only create values from SQL text that their application owns and reviews in
/// source control.
///
/// Why this wrapper exists: it makes the trust boundary visible in the type
/// system. If you see a `RawQuery`, you know the SQL came from reviewed source
/// code — not from a user, file, or network — because that is the only way to
/// construct one (`source_controlled`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawQuery {
    sql: String,
}

impl RawQuery {
    /// Marks source-controlled raw SQL for execution without sanitization.
    /// Newcomer note: naming the constructor this way forces every call site to
    /// state “I reviewed this SQL” — reach for it only with a string literal or
    /// a builder (like `build_recent_zones_sql`) that injects just validated
    /// integer literals.
    pub fn source_controlled(sql: impl Into<String>) -> Self {
        Self { sql: sql.into() }
    }

    /// Returns the owned raw SQL for adapter execution.
    /// The borrow lives only as long as the `RawQuery`; adapters copy what they
    /// need during `query`/`project` and never retain the `&str`.
    pub fn sql(&self) -> &str {
        &self.sql
    }
}

/// Projects an already-computed frame using a raw query language adapter.
/// Newcomer note: `frame` is moved in (the query takes ownership), and the
/// `Ok` value is a boxed trait object because different backends (DuckDB today,
/// a fake in tests) can return different concrete frame types.
pub trait FrameQuery: Send + Sync {
    /// Executes `query` over `frame` and returns an owned projected result.
    /// The input frame is consumed; the caller keeps only the returned frame.
    fn query(
        &self,
        frame: SourceFrame,
        query: RawQuery,
    ) -> Result<Box<dyn ComputedFrame>, MarketError>;
}

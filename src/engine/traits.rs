//! Engine-neutral result-frame access contract.

use crate::engine::error::MarketError;

/// Engine-neutral column type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnDType {
    Number,
    Boolean,
    Text,
    Null,
}

/// Engine-neutral table access trait.
///
/// This is the downstream contract consumed by charts, LLM tools,
/// and JSON serialization. All column access goes through this interface.
pub trait ComputedFrame: Send + Sync {
    /// Returns the number of rows in the frame.
    fn len(&self) -> usize;

    /// Returns true if the frame has no rows.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the column names in order.
    fn columns(&self) -> Vec<String>;

    /// Returns column names and types in frame column order.
    fn column_dtypes(&self) -> Vec<(String, ColumnDType)>;

    /// Returns a view of the last `count` rows.
    ///
    /// If `count` >= len(), returns all rows (saturating, not error).
    fn slice_last(&self, count: usize) -> Result<Box<dyn ComputedFrame>, MarketError>;

    /// Returns the f64 value at the given column and row.
    ///
    /// Returns `Ok(None)` for null cells and an error for missing columns,
    /// out-of-bounds rows, or type mismatches.
    fn f64_at(&self, column: &str, row: usize) -> Result<Option<f64>, MarketError>;

    /// Returns the string value at the given column and row.
    ///
    /// Returns `Ok(None)` for null cells and an error for missing columns,
    /// out-of-bounds rows, or type mismatches.
    fn string_at(&self, column: &str, row: usize) -> Result<Option<String>, MarketError>;

    /// Returns the frame as a vector of JSON objects (records).
    fn to_json_records(
        &self,
    ) -> Result<Vec<serde_json::Map<String, serde_json::Value>>, MarketError>;

    /// Returns true if the column exists in this frame.
    fn has_column(&self, column: &str) -> bool;
}

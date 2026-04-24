//! Core traits for the engine abstraction layer.

use crate::engine::error::MarketError;
use crate::engine::telegram_config::TelegramIndicatorConfig;
use crate::engine::validation::{ValidatedIndicator, ValidatedTicker};
use crate::model::kline::Kline;
use polars::prelude::DataFrame;

/// Engine trait for compute backends.
///
/// All implementations must be Send + Sync to ensure thread-safety
/// when used across async tasks.
pub trait MarketFrameEngine: Send + Sync {
    /// Returns a string identifying the compute backend (e.g., "polars", "duckdb").
    /// Used for logging, error context, and debugging.
    fn engine_identity(&self) -> &str;

    /// Compute indicators for telegram bot flow.
    ///
    /// Takes a slice of klines, validated ticker config, and indicator specs.
    /// Returns a computed frame with all derived columns.
    fn compute_telegram(
        &self,
        klines: &[Kline],
        ticker: ValidatedTicker,
        indicators: Vec<ValidatedIndicator>,
        config: &TelegramIndicatorConfig,
    ) -> Result<Box<dyn ComputedFrame>, MarketError>;

    /// Compute indicators for cryptobot flow.
    ///
    /// Takes a slice of klines and validated ticker config.
    /// Returns a computed frame with all derived columns.
    fn compute_crypto(
        &self,
        klines: &[Kline],
        ticker: ValidatedTicker,
    ) -> Result<Box<dyn ComputedFrame>, MarketError>;
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

    /// Returns a reference to the underlying DataFrame.
    ///
    /// **This is a migration aid** - needed because downstream consumers (e.g., `ta::gap_zones`)
    /// are not yet migrated to use `ComputedFrame`. Once all consumers use the engine boundary,
    /// this method should be removed.
    fn as_dataframe(&self) -> &DataFrame;

    /// Returns true if the column exists in this frame.
    fn has_column(&self, column: &str) -> bool;

    /// Returns the underlying Polars DataFrame when available.
    fn dataframe(&self) -> Option<&DataFrame>;
}

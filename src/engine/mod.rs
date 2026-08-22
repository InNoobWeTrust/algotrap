//! Engine abstraction layer for compute backend neutrality.
//!
//! This module provides the boundary between downstream consumers and DuckDB computation.
//!
//! # Architecture
//!
//! - [`MarketFrameEngine`]: Trait producing [`ComputedFrame`] values
//! - [`ComputedFrame`]: table access for charts, tools, and serialization
//! - [`DuckDBEngine`]: the shared compute implementation

pub mod duckdb_engine;
mod duckdb_ffi;
mod duckdb_ta_table_function;
pub mod error;
pub mod execution_strategy;
pub mod gap_zones;
mod indicators;
mod ta_execution;
/// Creates the single shared DuckDB-backed compute engine.
pub fn create_engine() -> Box<dyn traits::MarketFrameEngine> {
    Box::new(duckdb_engine::DuckDBEngine::new())
}
pub mod kline_batch;
pub mod telegram_config;
pub mod traits;
pub mod validation;

// Re-export types for convenience
pub use duckdb_engine::{
    CryptoBatchRequest, CryptoBatchResult, DuckDBComputedFrame, DuckDBEngine, TelegramBatchRequest,
    TelegramBatchResult,
};
pub use error::{ErrorKind, MarketError};
pub use execution_strategy::{ExecutionInstructions, ExecutionStrategy};
pub use kline_batch::{BatchLimits, RawKlineBatch};
pub use telegram_config::TelegramIndicatorConfig;
pub use traits::{ComputedFrame, MarketFrameEngine};
pub use validation::{Ticker, ValidatedIndicator, ValidatedTicker};

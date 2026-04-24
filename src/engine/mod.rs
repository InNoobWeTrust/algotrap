//! Engine abstraction layer for compute backend neutrality.
//!
//! This module provides the boundary between the downstream consumers (charts, LLM tools,
//! JSON export) and the underlying compute engine (Polars, DuckDB, etc.).
//!
//! # Architecture
//!
//! - [`MarketFrameEngine`]: Trait for compute backends that produce [`ComputedFrame`]
//! - [`ComputedFrame`]: Engine-neutral table access
//! - [`PolarsEngine`]: Baseline Polars-backed implementation
//! - [`DuckDBEngine`]: Future DuckDB-backed implementation (stubbed)

pub mod duckdb_engine;
pub mod duckdb_ffi;
pub mod duckdb_sql_indicators;
pub mod error;
/// Engine factory - creates the appropriate engine based on configuration.
pub mod engine_factory {
    use crate::engine::duckdb_engine::DuckDBEngine;
    use crate::engine::polars_engine::PolarsEngine;
    use crate::engine::traits::MarketFrameEngine;

    /// Engine selection based on environment variable or config.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum EngineType {
        Polars,
        DuckDB,
    }

    impl EngineType {
        /// Parse from environment variable or config string.
        pub fn from_str(s: &str) -> Self {
            match s.to_lowercase().as_str() {
                "duckdb" | "duck" => EngineType::DuckDB,
                _ => EngineType::Polars,
            }
        }

        /// Get from ENGINE environment variable, defaulting to Polars.
        pub fn from_env() -> Self {
            std::env::var("ALGOTRAP_ENGINE")
                .ok()
                .map(|v| Self::from_str(&v))
                .unwrap_or(EngineType::Polars)
        }
    }

    /// Create an engine instance based on the selected type.
    pub fn create_engine(engine_type: EngineType) -> Box<dyn MarketFrameEngine> {
        match engine_type {
            EngineType::Polars => Box::new(PolarsEngine::new()),
            EngineType::DuckDB => Box::new(DuckDBEngine::new()),
        }
    }

    /// Create engine from environment variable or default to Polars.
    pub fn create_engine_from_env() -> Box<dyn MarketFrameEngine> {
        create_engine(EngineType::from_env())
    }
}
pub mod json_serializer;
pub mod kline_batch;
pub mod polars_engine;
pub mod telegram_config;
pub mod traits;
pub mod type_mapper;
pub mod validation;

// Re-export types for convenience
pub use engine_factory::{create_engine, create_engine_from_env, EngineType};
pub use error::{ErrorKind, MarketError};
pub use kline_batch::{BatchLimits, RawKlineBatch};
pub use telegram_config::TelegramIndicatorConfig;
pub use traits::{ComputedFrame, MarketFrameEngine};
pub use type_mapper::DuckDbType;
pub use validation::{Ticker, ValidatedIndicator, ValidatedTicker};

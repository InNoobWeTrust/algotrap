/// Stream ingestion and processing adapters.
pub mod adapter;
/// Engine-owned result frames and validation boundaries.
pub mod engine;
/// External service and notification integrations.
pub mod ext;
/// Market data domain models.
pub mod model;
/// Common public exports for application code.
pub mod prelude;
/// Frame projection adapters.
pub mod query;
/// Technical-analysis kernels and processors.
pub mod ta;
/// Market-time and timeframe utilities.
pub mod time_utils;

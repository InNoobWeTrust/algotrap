//! Engine-owned result frames and validation boundaries.
//!
//! This module owns [`SourceColumnData`], [`SourceFrame`], [`QueryResultFrame`], and
//! [`ComputedFrame`] table access for charts, tools, and serialization.

/// Engine error types.
pub mod error;
/// Owned result-frame types and accessors.
pub mod frame;
/// Engine-neutral frame access contracts.
pub mod traits;
/// Ticker input validation types.
pub mod validation;

// Re-export types for convenience
pub use error::{ErrorKind, MarketError};
pub use frame::{QueryResultFrame, SourceColumnData, SourceFrame};
pub use traits::ComputedFrame;
pub use validation::{Ticker, ValidatedTicker};

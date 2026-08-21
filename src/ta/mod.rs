pub mod error;
pub mod experimental;
pub mod gap_zones;
pub mod indicator;
pub mod ma;
pub mod metric;
pub mod ohlc;
pub mod plan;
pub mod prelude;
pub mod rsi;

pub use error::{TaError, TaErrorKind, TaResult};

/// Rejects non-finite numerical inputs at TA kernel boundaries.
pub(crate) fn validate_finite_series(name: &str, values: &[f64]) -> TaResult<()> {
    if let Some(index) = values.iter().position(|value| !value.is_finite()) {
        return Err(TaError::validation(format!(
            "{name} contains a non-finite value at index {index}"
        )));
    }
    Ok(())
}

/// Rejects a non-finite numerical scalar at a TA kernel boundary.
pub(crate) fn validate_finite_value(name: &str, value: f64) -> TaResult<()> {
    value
        .is_finite()
        .then_some(())
        .ok_or_else(|| TaError::validation(format!("{name} must be finite")))
}

/// Ensures a numerical kernel never returns a non-finite result.
pub(crate) fn validate_finite_output(name: &str, values: &[f64]) -> TaResult<()> {
    if let Some(index) = values.iter().position(|value| !value.is_finite()) {
        return Err(TaError::computation(format!(
            "{name} produced a non-finite value at index {index}"
        )));
    }
    Ok(())
}

/// Technical-analysis error types.
pub mod error;
/// Stateful technical-analysis kernel contracts.
pub mod kernel;
/// Built-in stateful technical-analysis transforms.
pub mod ops;
/// Common technical-analysis exports.
pub mod prelude;
/// Stateful kernel execution wrapper.
pub mod processor;

pub use error::{TaError, TaErrorKind, TaResult};

/// Rejects a non-finite numerical scalar at a TA kernel boundary.
pub(crate) fn validate_finite_value(name: &str, value: f64) -> TaResult<()> {
    value
        .is_finite()
        .then_some(())
        .ok_or_else(|| TaError::validation(format!("{name} must be finite")))
}

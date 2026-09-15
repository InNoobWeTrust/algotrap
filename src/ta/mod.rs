/// Technical-analysis error types.
pub mod error;
pub mod iching;
/// Stateful technical-analysis kernel contracts.
pub mod kernel;
/// Built-in stateful technical-analysis transforms.
pub mod ops;
/// Common technical-analysis exports.
pub mod prelude;
/// Stateful kernel execution wrapper.
pub mod processor;

pub use error::{TaError, TaErrorKind, TaResult};
pub use iching::{
    HexagramEnergy, IchingSignal, LeapMonthPolicy, plum_blossom_signal,
    plum_blossom_signal_with_policy,
};

/// Rejects a non-finite numerical scalar at a TA kernel boundary.
pub(crate) fn validate_finite_value(name: &str, value: f64) -> TaResult<()> {
    value
        .is_finite()
        .then_some(())
        .ok_or_else(|| TaError::validation(format!("{name} must be finite")))
}

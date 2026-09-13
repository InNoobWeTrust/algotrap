use crate::ta::{TaError, TaResult};

pub(super) fn validate_period(period: usize) -> TaResult<()> {
    if period == 0 {
        Err(TaError::invalid_period("period must be greater than zero"))
    } else {
        Ok(())
    }
}

pub(super) fn validate_finite_input(name: &str, value: f64) -> TaResult<()> {
    value
        .is_finite()
        .then_some(())
        .ok_or_else(|| TaError::validation(format!("{name} input must be finite")))
}

pub(super) fn validate_finite_output(name: &str, value: f64) -> TaResult<()> {
    value
        .is_finite()
        .then_some(())
        .ok_or_else(|| TaError::computation(format!("{name} produced a non-finite value")))
}

pub(super) fn validate_multiplier(name: &str, value: f64) -> TaResult<()> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(TaError::validation(format!(
            "{name} must be finite and strictly positive"
        )))
    }
}

/// Requires a current-row child output that the preserved first-valid-point
/// invariant guarantees for valid inputs.
pub fn require_output(label: &str, value: Option<f64>) -> TaResult<f64> {
    value.ok_or_else(|| TaError::computation(format!("{label} output is missing")))
}

/// Computes ATR percent with the preserved zero-open edge behavior.
pub fn atr_percent(atr: f64, open: f64) -> f64 {
    if open == 0.0 { 0.0 } else { atr / open }
}

/// Combines two nullable derived inputs, propagating `None` and rejecting a
/// non-finite computed value.
pub fn option_map2(
    first: Option<f64>,
    second: Option<f64>,
    combine: impl FnOnce(f64, f64) -> f64,
) -> TaResult<Option<f64>> {
    match (first, second) {
        (Some(first), Some(second)) => {
            let value = combine(first, second);
            if value.is_finite() {
                Ok(Some(value))
            } else {
                Err(TaError::computation(
                    "derived indicator output is non-finite",
                ))
            }
        }
        _ => Ok(None),
    }
}

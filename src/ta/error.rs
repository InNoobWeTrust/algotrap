//! Technical-analysis domain error contract.

/// Classifies a failure produced by the technical-analysis domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaErrorKind {
    Validation,
    InvalidPeriod,
    InvalidPlan,
    Alignment,
    Source,
    NonFiniteIndicatorOutput,
    Computation,
}

/// A typed technical-analysis failure with stable diagnostics for adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaError {
    pub kind: TaErrorKind,
    pub message: String,
    pub context: Option<String>,
}

/// The result type returned by technical-analysis APIs.
pub type TaResult<T> = Result<T, TaError>;

impl TaError {
    /// Creates a TA input or invariant validation error.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(TaErrorKind::Validation, message)
    }

    /// Creates an error for a zero or otherwise invalid lookback period.
    pub fn invalid_period(message: impl Into<String>) -> Self {
        Self::new(TaErrorKind::InvalidPeriod, message)
    }

    /// Creates an error for an invalid lazy indicator plan.
    pub fn invalid_plan(message: impl Into<String>) -> Self {
        Self::new(TaErrorKind::InvalidPlan, message)
    }

    /// Creates an error for incompatible domain series lengths.
    pub fn alignment(message: impl Into<String>) -> Self {
        Self::new(TaErrorKind::Alignment, message)
    }

    /// Creates an error for an unavailable or invalid domain source.
    pub fn source(message: impl Into<String>) -> Self {
        Self::new(TaErrorKind::Source, message)
    }

    /// Creates a typed error for a non-finite materialized indicator value.
    pub fn non_finite_indicator_output(row: usize, column: &'static str) -> Self {
        Self {
            kind: TaErrorKind::NonFiniteIndicatorOutput,
            message: "indicator output is non-finite".into(),
            context: Some(format!("row {row}, column {column}")),
        }
    }

    /// Creates a TA computation failure.
    pub fn computation(message: impl Into<String>) -> Self {
        Self::new(TaErrorKind::Computation, message)
    }

    fn new(kind: TaErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            context: None,
        }
    }
}

impl std::fmt::Display for TaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(context) = &self.context {
            write!(f, "{} [{context}]", self.message)
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for TaError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_expose_typed_ta_context() {
        let period = TaError::invalid_period("period must be greater than zero");
        assert_eq!(period.kind, TaErrorKind::InvalidPeriod);

        let plan = TaError::invalid_plan("indicator plan contains a cycle");
        assert_eq!(plan.kind, TaErrorKind::InvalidPlan);

        let output = TaError::non_finite_indicator_output(7, "atr");
        assert_eq!(output.kind, TaErrorKind::NonFiniteIndicatorOutput);
        assert_eq!(output.message, "indicator output is non-finite");
        assert_eq!(output.context.as_deref(), Some("row 7, column atr"));
    }
}

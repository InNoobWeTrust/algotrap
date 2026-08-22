/// Error kind enumeration for MarketError
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    ConfigurationError,
    ValidationError,
    ComputationError,
    NonFiniteIndicatorOutput,
    DataAccessError,
    ThreadSafetyError,
    InvocationLifecycleError,
    PartialFailure,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorKind::ConfigurationError => write!(f, "ConfigurationError"),
            ErrorKind::ValidationError => write!(f, "ValidationError"),
            ErrorKind::ComputationError => write!(f, "ComputationError"),
            ErrorKind::NonFiniteIndicatorOutput => write!(f, "NonFiniteIndicatorOutput"),
            ErrorKind::DataAccessError => write!(f, "DataAccessError"),
            ErrorKind::ThreadSafetyError => write!(f, "ThreadSafetyError"),
            ErrorKind::InvocationLifecycleError => write!(f, "InvocationLifecycleError"),
            ErrorKind::PartialFailure => write!(f, "PartialFailure"),
        }
    }
}

/// Custom error type with detailed variants for fail-closed handling
#[derive(Debug, Clone)]
pub struct MarketError {
    pub kind: ErrorKind,
    pub message: String,
    pub context: Option<String>,
}

impl MarketError {
    /// Creates a configuration error for a controlled runtime boundary.
    pub fn configuration(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::ConfigurationError,
            message: msg.into(),
            context: None,
        }
    }

    /// Creates a new ValidationError
    pub fn validation(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::ValidationError,
            message: msg.into(),
            context: None,
        }
    }

    /// Creates a new ComputationError
    pub fn computation(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::ComputationError,
            message: msg.into(),
            context: None,
        }
    }

    /// Creates a typed error when an indicator kernel produces NaN or infinity.
    pub fn non_finite_indicator_output(row: usize, column: &'static str) -> Self {
        Self {
            kind: ErrorKind::NonFiniteIndicatorOutput,
            message: "indicator output is non-finite".into(),
            context: Some(format!("row {row}, column {column}")),
        }
    }

    /// Creates a new DataAccessError
    pub fn data_access(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::DataAccessError,
            message: msg.into(),
            context: None,
        }
    }

    /// Creates a new ThreadSafetyError
    pub fn thread_safety(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::ThreadSafetyError,
            message: msg.into(),
            context: None,
        }
    }

    /// Creates a typed error for an invalid per-session invocation transition.
    pub fn invocation_lifecycle(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvocationLifecycleError,
            message: msg.into(),
            context: None,
        }
    }

    /// Creates a new PartialFailure
    pub fn partial_failure(errors: Vec<MarketError>) -> Self {
        Self {
            kind: ErrorKind::PartialFailure,
            message: format!("Partial failure with {} error(s)", errors.len()),
            context: None,
        }
    }

    /// Adds context to an error
    pub fn with_context(self, ctx: impl Into<String>) -> Self {
        Self {
            context: Some(ctx.into()),
            ..self
        }
    }
}

impl std::fmt::Display for MarketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref ctx) = self.context {
            write!(f, "{}: {} [{}]", self.kind, self.message, ctx)
        } else {
            write!(f, "{}: {}", self.kind, self.message)
        }
    }
}

impl std::error::Error for MarketError {}

impl From<crate::ta::TaError> for MarketError {
    /// Maps TA domain failures to the engine's established public error contract.
    fn from(error: crate::ta::TaError) -> Self {
        match error.kind {
            crate::ta::TaErrorKind::NonFiniteIndicatorOutput => Self {
                kind: ErrorKind::NonFiniteIndicatorOutput,
                message: error.message,
                context: error.context,
            },
            crate::ta::TaErrorKind::Computation => Self::computation(error.message),
            crate::ta::TaErrorKind::Validation
            | crate::ta::TaErrorKind::InvalidPeriod
            | crate::ta::TaErrorKind::InvalidPlan
            | crate::ta::TaErrorKind::Alignment
            | crate::ta::TaErrorKind::Source => Self::validation(error.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ta::TaError;

    #[test]
    fn ta_validation_and_non_finite_errors_preserve_engine_contract() {
        let validation = MarketError::from(TaError::validation("Kline slice is empty"));
        assert_eq!(validation.kind, ErrorKind::ValidationError);
        assert_eq!(validation.message, "Kline slice is empty");

        let output = MarketError::from(TaError::non_finite_indicator_output(0, "atr"));
        assert_eq!(output.kind, ErrorKind::NonFiniteIndicatorOutput);
        assert_eq!(output.message, "indicator output is non-finite");
        assert_eq!(output.context.as_deref(), Some("row 0, column atr"));
    }
}

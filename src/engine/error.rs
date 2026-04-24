/// Error kind enumeration for MarketError
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    ValidationError,
    ComputationError,
    DataAccessError,
    ThreadSafetyError,
    PartialFailure,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorKind::ValidationError => write!(f, "ValidationError"),
            ErrorKind::ComputationError => write!(f, "ComputationError"),
            ErrorKind::DataAccessError => write!(f, "DataAccessError"),
            ErrorKind::ThreadSafetyError => write!(f, "ThreadSafetyError"),
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

impl From<polars::prelude::PolarsError> for MarketError {
    fn from(err: polars::prelude::PolarsError) -> Self {
        Self::computation(err.to_string())
    }
}

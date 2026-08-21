use std::num::NonZeroUsize;

/// Immutable instructions for opt-in intra-series execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionInstructions {
    workers: NonZeroUsize,
}

impl ExecutionInstructions {
    /// Creates instructions with a non-zero internal execution budget.
    pub const fn new(workers: NonZeroUsize) -> Self {
        Self { workers }
    }

    /// Returns the requested internal execution budget.
    pub const fn workers(self) -> NonZeroUsize {
        self.workers
    }
}

/// Evaluation policy resolved at the DuckDB engine boundary.
///
/// `Auto` currently resolves to [`ExecutionStrategy::Sequential`].
/// `IntraSeries` is explicitly opt-in and may not be combined with a
/// cross-request batch worker count above one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExecutionStrategy {
    /// Use the empirically safe engine default.
    #[default]
    Auto,
    /// Evaluate each TA plan on its deterministic sequential backend.
    Sequential,
    /// Use the isolated TA intra-series backend with the supplied instructions.
    IntraSeries(ExecutionInstructions),
}

impl ExecutionStrategy {
    /// Resolves the engine default without exposing execution details to TA callers.
    pub const fn resolve(self) -> Self {
        match self {
            Self::Auto => Self::Sequential,
            explicit => explicit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutionStrategy;

    #[test]
    fn auto_resolves_to_sequential() {
        assert_eq!(
            ExecutionStrategy::Auto.resolve(),
            ExecutionStrategy::Sequential
        );
    }
}

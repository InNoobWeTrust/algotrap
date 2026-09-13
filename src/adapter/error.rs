use crate::ta::TaError;

/// Errors emitted by a stream pipeline, parameterized by source error `E` and stream identifier `Id`.
pub enum StreamPipelineError<E, Id> {
    /// An error from the upstream source stream.
    Source(E),
    /// The broadcast receiver lagged, skipping `skipped` buffered messages.
    Lagged { skipped: u64 },
    /// A technical-analysis kernel failed.
    Ta(TaError),
    /// The left and right stream identifiers did not align.
    Alignment { left: Id, right: Id },
    /// The downstream output stream was closed.
    OutputClosed,
    /// A pipeline task failed to join with message `message`.
    TaskJoin { message: String },
}

use core::fmt;

/// Errors raised while validating or serializing a chart document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChartContractError {
    NonIncreasingTimestamp {
        index: usize,
        previous: i64,
        current: i64,
    },
    InvalidRecordTime {
        index: usize,
    },
    InvalidGapBounds {
        index: usize,
    },
    EmptyDataset,
    EmptyRegistry,
    UnsupportedVersion {
        found: u32,
        supported: u32,
    },
    InvalidIdentity {
        field: &'static str,
    },
    JsonSerialization {
        message: String,
    },
}

impl ChartContractError {
    pub(crate) fn json(error: serde_json::Error) -> Self {
        Self::JsonSerialization {
            message: error.to_string(),
        }
    }
}

impl fmt::Display for ChartContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonIncreasingTimestamp {
                index,
                previous,
                current,
            } => write!(
                formatter,
                "non-increasing timestamp at index {index}: previous={previous}, current={current}"
            ),
            Self::InvalidRecordTime { index } => {
                write!(formatter, "invalid record time at index {index}")
            }
            Self::InvalidGapBounds { index } => {
                write!(formatter, "invalid gap bounds at index {index}")
            }
            Self::EmptyDataset => formatter.write_str("dataset is empty"),
            Self::EmptyRegistry => formatter.write_str("registry is empty"),
            Self::UnsupportedVersion { found, supported } => {
                write!(
                    formatter,
                    "unsupported version {found} (expected {supported})"
                )
            }
            Self::InvalidIdentity { field } => write!(formatter, "invalid empty {field}"),
            Self::JsonSerialization { message } => {
                write!(formatter, "JSON serialization failed: {message}")
            }
        }
    }
}

impl core::error::Error for ChartContractError {}

//! DuckDB type mapping utilities.

use crate::engine::error::MarketError;
use polars::prelude::*;

/// DuckDB type representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DuckDbType {
    Boolean,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Int8,
    Int16,
    Int32,
    Int64,
    Float32,
    Float64,
    Text,
    Date,
    Time,
    Timestamp,
    TimestampS,
    TimestampMs,
    TimestampNs,
    Interval,
    Blob,
    JSON,
    UUID,
}

impl std::fmt::Display for DuckDbType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DuckDbType::Boolean => write!(f, "BOOLEAN"),
            DuckDbType::UInt8 => write!(f, "UTINYINT"),
            DuckDbType::UInt16 => write!(f, "USMALLINT"),
            DuckDbType::UInt32 => write!(f, "UINTEGER"),
            DuckDbType::UInt64 => write!(f, "UBIGINT"),
            DuckDbType::Int8 => write!(f, "TINYINT"),
            DuckDbType::Int16 => write!(f, "SMALLINT"),
            DuckDbType::Int32 => write!(f, "INTEGER"),
            DuckDbType::Int64 => write!(f, "BIGINT"),
            DuckDbType::Float32 => write!(f, "FLOAT"),
            DuckDbType::Float64 => write!(f, "DOUBLE"),
            DuckDbType::Text => write!(f, "VARCHAR"),
            DuckDbType::Date => write!(f, "DATE"),
            DuckDbType::Time => write!(f, "TIME"),
            DuckDbType::Timestamp => write!(f, "TIMESTAMP"),
            DuckDbType::TimestampS => write!(f, "TIMESTAMP_S"),
            DuckDbType::TimestampMs => write!(f, "TIMESTAMP_MS"),
            DuckDbType::TimestampNs => write!(f, "TIMESTAMP_NS"),
            DuckDbType::Interval => write!(f, "INTERVAL"),
            DuckDbType::Blob => write!(f, "BLOB"),
            DuckDbType::JSON => write!(f, "JSON"),
            DuckDbType::UUID => write!(f, "UUID"),
        }
    }
}

/// Maps Polars DataType to DuckDB type.
pub fn polars_dtype_to_duckdb(dtype: &DataType) -> Result<DuckDbType, MarketError> {
    match dtype {
        DataType::Boolean => Ok(DuckDbType::Boolean),
        DataType::UInt8 => Ok(DuckDbType::UInt8),
        DataType::UInt16 => Ok(DuckDbType::UInt16),
        DataType::UInt32 => Ok(DuckDbType::UInt32),
        DataType::UInt64 => Ok(DuckDbType::UInt64),
        DataType::Int8 => Ok(DuckDbType::Int8),
        DataType::Int16 => Ok(DuckDbType::Int16),
        DataType::Int32 => Ok(DuckDbType::Int32),
        DataType::Int64 => Ok(DuckDbType::Int64),
        DataType::Float32 => Ok(DuckDbType::Float32),
        DataType::Float64 => Ok(DuckDbType::Float64),
        DataType::String => Ok(DuckDbType::Text),
        DataType::Date => Ok(DuckDbType::Date),
        DataType::Time => Ok(DuckDbType::Time),
        DataType::Datetime(TimeUnit::Milliseconds, None) => Ok(DuckDbType::TimestampMs),
        DataType::Datetime(TimeUnit::Nanoseconds, None) => Ok(DuckDbType::TimestampNs),
        DataType::Datetime(_, _) => Ok(DuckDbType::Timestamp),
        DataType::Duration(_) => Ok(DuckDbType::Int64),
        DataType::Binary | DataType::BinaryOffset => Ok(DuckDbType::Blob),
        DataType::Null => Err(MarketError::validation("Null type not supported")),
        DataType::Unknown(_) => Err(MarketError::validation("Unknown type not supported")),
        DataType::List(_) => Err(MarketError::validation("List type not supported")),
        DataType::Array(_, _) => Err(MarketError::validation("Array type not supported")),
        other => Err(MarketError::validation(format!(
            "{other:?} type not supported"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_types() {
        assert_eq!(
            polars_dtype_to_duckdb(&DataType::Float64).unwrap(),
            DuckDbType::Float64
        );
        assert_eq!(
            polars_dtype_to_duckdb(&DataType::Int64).unwrap(),
            DuckDbType::Int64
        );
        assert_eq!(
            polars_dtype_to_duckdb(&DataType::Boolean).unwrap(),
            DuckDbType::Boolean
        );
        assert_eq!(
            polars_dtype_to_duckdb(&DataType::String).unwrap(),
            DuckDbType::Text
        );
    }

    #[test]
    fn test_timestamp_types() {
        assert_eq!(
            polars_dtype_to_duckdb(&DataType::Datetime(TimeUnit::Milliseconds, None)).unwrap(),
            DuckDbType::TimestampMs
        );
        assert_eq!(
            polars_dtype_to_duckdb(&DataType::Datetime(TimeUnit::Microseconds, None)).unwrap(),
            DuckDbType::Timestamp
        );
        assert_eq!(
            polars_dtype_to_duckdb(&DataType::Datetime(TimeUnit::Nanoseconds, None)).unwrap(),
            DuckDbType::TimestampNs
        );
    }
}

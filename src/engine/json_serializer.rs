//! JSON serialization utilities with NaN handling policy.

use crate::engine::error::MarketError;
use polars::prelude::*;

/// Policy for handling NaN values during JSON serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonNanPolicy {
    /// Reject NaN values with an error.
    Reject,
    /// Convert NaN to JSON null.
    Null,
    /// Convert NaN to the string "NaN".
    Stringify,
}

/// Recursive JSON serializer with NaN policy.
pub struct RecursiveJsonSerializer {
    policy: JsonNanPolicy,
}

impl RecursiveJsonSerializer {
    /// Creates a new serializer with the given NaN policy.
    pub fn new(policy: JsonNanPolicy) -> Self {
        Self { policy }
    }

    /// Serializes an AnyValue to JSON, applying the NaN policy recursively.
    pub fn serialize_any(&self, value: &AnyValue) -> Result<serde_json::Value, MarketError> {
        to_json_value_with_policy(value, self.policy)
    }
}

/// Serializes a DataFrame to a vector of JSON maps using the specified NaN policy.
pub fn serialize_dataframe(
    df: &DataFrame,
    policy: JsonNanPolicy,
) -> Result<Vec<serde_json::Map<String, serde_json::Value>>, MarketError> {
    let serializer = RecursiveJsonSerializer::new(policy);
    let columns: Vec<String> = df
        .get_columns()
        .iter()
        .map(|column| column.name().to_string())
        .collect();
    let mut records = Vec::with_capacity(df.height());

    for row_idx in 0..df.height() {
        let mut record = serde_json::Map::with_capacity(columns.len());

        for col_name in &columns {
            let col = df.column(col_name).map_err(|e| {
                MarketError::computation(format!("Failed to get column '{}': {}", col_name, e))
            })?;

            let value = if let Ok(series) = col.f64() {
                series
                    .get(row_idx)
                    .map(|v| serializer.serialize_any(&AnyValue::Float64(v)))
                    .unwrap_or_else(|| Ok(serde_json::Value::Null))
            } else if let Ok(series) = col.str() {
                series
                    .get(row_idx)
                    .map(|v| serializer.serialize_any(&AnyValue::String(v)))
                    .unwrap_or_else(|| Ok(serde_json::Value::Null))
            } else {
                col.get(row_idx)
                    .map(|value| serializer.serialize_any(&value))
                    .unwrap_or_else(|_| Ok(serde_json::Value::Null))
            };

            record.insert(col_name.clone(), value?);
        }

        records.push(record);
    }

    Ok(records)
}

/// Converts an AnyValue to JSON Value with the given NaN policy.
pub fn to_json_value_with_policy(
    value: &AnyValue,
    policy: JsonNanPolicy,
) -> Result<serde_json::Value, MarketError> {
    match value {
        AnyValue::Null => Ok(serde_json::Value::Null),
        AnyValue::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        AnyValue::UInt8(v) => Ok(serde_json::json!(v)),
        AnyValue::UInt16(v) => Ok(serde_json::json!(v)),
        AnyValue::UInt32(v) => Ok(serde_json::json!(v)),
        AnyValue::UInt64(v) => Ok(serde_json::json!(v)),
        AnyValue::Int8(v) => Ok(serde_json::json!(v)),
        AnyValue::Int16(v) => Ok(serde_json::json!(v)),
        AnyValue::Int32(v) => Ok(serde_json::json!(v)),
        AnyValue::Int64(v) => Ok(serde_json::json!(v)),
        AnyValue::Float32(v) => f64_to_json(*v as f64, policy),
        AnyValue::Float64(v) => f64_to_json(*v, policy),
        AnyValue::String(v) => Ok(serde_json::Value::String((*v).to_owned())),
        AnyValue::StringOwned(v) => Ok(serde_json::Value::String(v.to_string())),
        AnyValue::Binary(v) => Ok(serde_json::json!(v)),
        AnyValue::BinaryOwned(v) => Ok(serde_json::json!(v)),
        AnyValue::Datetime(v, time_unit, _tz) => {
            let ms = match time_unit {
                TimeUnit::Milliseconds => *v,
                TimeUnit::Microseconds => *v / 1000,
                TimeUnit::Nanoseconds => *v / 1_000_000,
            };
            Ok(serde_json::Value::Number(ms.into()))
        }
        other => Err(MarketError::validation(format!(
            "{other:?} serialization not implemented"
        ))),
    }
}

fn f64_to_json(v: f64, policy: JsonNanPolicy) -> Result<serde_json::Value, MarketError> {
    if v.is_nan() {
        match policy {
            JsonNanPolicy::Reject => Err(MarketError::validation("NaN value encountered")),
            JsonNanPolicy::Null => Ok(serde_json::Value::Null),
            JsonNanPolicy::Stringify => Ok(serde_json::Value::String("NaN".to_string())),
        }
    } else {
        Ok(serde_json::json!(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_policy() {
        let ser = RecursiveJsonSerializer::new(JsonNanPolicy::Null);
        assert_eq!(
            ser.serialize_any(&AnyValue::Null).unwrap(),
            serde_json::Value::Null
        );
    }

    #[test]
    fn test_nan_reject_policy() {
        let result = to_json_value_with_policy(&AnyValue::Float64(f64::NAN), JsonNanPolicy::Reject);
        assert!(result.is_err());
    }

    #[test]
    fn test_nan_null_policy() {
        let result =
            to_json_value_with_policy(&AnyValue::Float64(f64::NAN), JsonNanPolicy::Null).unwrap();
        assert_eq!(result, serde_json::Value::Null);
    }

    #[test]
    fn test_serialize_dataframe_null_nan_policy() {
        let df = df!(
            "price" => &[1.0_f64, f64::NAN],
            "symbol" => &["BTC", "ETH"]
        )
        .unwrap();

        let records = serialize_dataframe(&df, JsonNanPolicy::Null).unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].get("price"), Some(&serde_json::json!(1.0)));
        assert_eq!(records[1].get("price"), Some(&serde_json::Value::Null));
        assert_eq!(
            records[1].get("symbol"),
            Some(&serde_json::Value::String("ETH".to_string()))
        );
    }
}

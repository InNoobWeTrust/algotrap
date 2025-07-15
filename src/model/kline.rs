use polars_row_derive::IterToDataFrame;
use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Deserialize, IterToDataFrame)]
pub struct Kline {
    #[serde(deserialize_with = "de_f64_or_string_as_f64")]
    pub open: f64,
    #[serde(deserialize_with = "de_f64_or_string_as_f64")]
    pub high: f64,
    #[serde(deserialize_with = "de_f64_or_string_as_f64")]
    pub low: f64,
    #[serde(deserialize_with = "de_f64_or_string_as_f64")]
    pub close: f64,
    #[serde(deserialize_with = "de_f64_or_string_as_f64")]
    pub volume: f64,
    pub time: i64,
    #[serde(default, deserialize_with = "de_opt_f64_or_string_as_f64")]
    pub adjclose: Option<f64>,
}

fn de_f64_or_string_as_f64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::String(s) => s.parse().map_err(de::Error::custom)?,
        Value::Number(num) => num
            .as_f64()
            .ok_or_else(|| de::Error::custom("Invalid number"))?,
        _ => return Err(de::Error::custom("wrong type")),
    })
}

fn de_opt_f64_or_string_as_f64<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<f64>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::String(s) => Some(s.parse().map_err(de::Error::custom)?),
        Value::Number(num) => Some(
            num.as_f64()
                .ok_or_else(|| de::Error::custom("Invalid number"))?,
        ),
        Value::Null => None,
        _ => return Err(de::Error::custom("wrong type")),
    })
}

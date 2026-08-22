use crate::engine::traits::ComputedFrame;
use core::error::Error;
use serde_json::Value;
use std::ops::Deref;

#[inline]
pub fn df_to_json(df: &dyn ComputedFrame) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let records = df.to_json_records()?;
    Ok(Value::Array(
        records.into_iter().map(Value::Object).collect(),
    ))
}

/// Transitive type to make syntastic sugar for converting Dataframe to JSON
pub struct JsonDataframe(Value);

impl Deref for JsonDataframe {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<&dyn ComputedFrame> for JsonDataframe {
    type Error = Box<dyn Error + Send + Sync>;

    fn try_from(value: &dyn ComputedFrame) -> Result<Self, Self::Error> {
        let records = value.to_json_records()?;
        let df_json = Value::Array(records.into_iter().map(Value::Object).collect());
        Ok(JsonDataframe(df_json))
    }
}

impl TryFrom<Box<dyn ComputedFrame>> for JsonDataframe {
    type Error = Box<dyn Error + Send + Sync>;

    fn try_from(value: Box<dyn ComputedFrame>) -> Result<Self, Self::Error> {
        let records = value.to_json_records()?;
        let df_json = Value::Array(records.into_iter().map(Value::Object).collect());
        Ok(JsonDataframe(df_json))
    }
}

impl From<&JsonDataframe> for Value {
    fn from(value: &JsonDataframe) -> Self {
        value.0.clone()
    }
}

impl From<JsonDataframe> for Value {
    fn from(value: JsonDataframe) -> Self {
        value.0
    }
}

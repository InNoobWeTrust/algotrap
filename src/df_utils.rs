use core::error::Error;
use polars::prelude::*;
use serde_json::Value;
use std::{io::Cursor, ops::Deref};

#[inline]
pub fn df_to_json(df: &mut DataFrame) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let mut file = Cursor::new(Vec::new());
    JsonWriter::new(&mut file)
        .with_json_format(JsonFormat::Json)
        .finish(df)
        .unwrap();
    //let df_json = String::from_utf8(file.into_inner()).unwrap();
    let df_json = serde_json::from_slice(&file.into_inner())?;
    Ok(df_json)
}

/// Transitive type to make syntastic sugar for converting Dataframe to JSON
pub struct JsonDataframe(Value);

impl Deref for JsonDataframe {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<&DataFrame> for JsonDataframe {
    type Error = Box<dyn Error + Send + Sync>;

    fn try_from(value: &DataFrame) -> Result<Self, Self::Error> {
        let df_json = df_to_json(&mut value.clone())?;
        Ok(JsonDataframe(df_json))
    }
}

impl TryFrom<DataFrame> for JsonDataframe {
    type Error = Box<dyn Error + Send + Sync>;

    fn try_from(mut value: DataFrame) -> Result<Self, Self::Error> {
        let df_json = df_to_json(&mut value)?;
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

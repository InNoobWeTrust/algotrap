use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ChartContractError;

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;
pub const DATASET_SCHEMA_VERSION: u32 = 1;
pub const FIXED_DOCUMENT_SCHEMA_VERSION: u32 = 1;

/// A flat, per-candle JSON record consumed directly by the canonical chart template.
pub type ChartRecord = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DatasetKey {
    pub ticker: String,
    pub timeframe: String,
}

impl DatasetKey {
    #[must_use]
    pub fn new(ticker: impl Into<String>, timeframe: impl Into<String>) -> Self {
        Self {
            ticker: ticker.into(),
            timeframe: timeframe.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapDirection {
    Bullish,
    Bearish,
    Flat,
}

/// Raw gap-zone geometry as emitted by the production Cryptobot chart contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapZone {
    pub time_ms: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub body_bottom: f64,
    pub body_top: f64,
    pub body_ratio: Option<f64>,
    pub direction: GapDirection,
}

/// One ticker/timeframe payload supplied to the canonical template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractiveDataset {
    pub key: DatasetKey,
    pub display_symbol: String,
    pub records: Vec<ChartRecord>,
    pub gap_zones: Vec<GapZone>,
}

/// Metadata consumed by the template ticker picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntryMeta {
    pub symbol: String,
    pub sl_percent: String,
    pub tol_percent: String,
    pub default_tf: String,
}

/// The two JSON arrays injected into the production interactive template.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartRegistry {
    pub schema_version: u32,
    pub tickers: Value,
    pub chart_tfs: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    FixedDocument,
}

/// A standalone fixed chart rendered by the same interactive production template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixedChartDocument {
    pub schema_version: u32,
    pub kind: DocumentKind,
    pub title: String,
    pub subtitle: Option<String>,
    pub dataset: InteractiveDataset,
}

/// Validates the renderer's essential dataset invariants.
pub fn validate_interactive_dataset(
    dataset: &InteractiveDataset,
) -> Result<(), ChartContractError> {
    validate_non_empty("ticker", &dataset.key.ticker)?;
    validate_non_empty("timeframe", &dataset.key.timeframe)?;
    validate_non_empty("display_symbol", &dataset.display_symbol)?;
    if dataset.records.is_empty() {
        return Err(ChartContractError::EmptyDataset);
    }

    let mut previous = None;
    for (index, record) in dataset.records.iter().enumerate() {
        let time = record_time_ms(record).ok_or(ChartContractError::InvalidRecordTime { index })?;
        if let Some(previous) = previous
            && time <= previous
        {
            return Err(ChartContractError::NonIncreasingTimestamp {
                index,
                previous,
                current: time,
            });
        }
        previous = Some(time);
    }

    for (index, zone) in dataset.gap_zones.iter().enumerate() {
        if !zone.body_bottom.is_finite()
            || !zone.body_top.is_finite()
            || zone.body_bottom > zone.body_top
        {
            return Err(ChartContractError::InvalidGapBounds { index });
        }
    }
    Ok(())
}

/// Serializes a validated template registry as compact JSON.
pub fn serialize_registry(registry: &ChartRegistry) -> Result<String, ChartContractError> {
    validate_registry(registry)?;
    serde_json::to_string(&serde_json::json!({
        "schema_version": registry.schema_version,
        "tickers": registry.tickers,
        "chart_tfs": registry.chart_tfs,
    }))
    .map_err(ChartContractError::json)
}

/// Validates a fixed document before it is embedded in the template.
pub fn validate_fixed_document(document: &FixedChartDocument) -> Result<(), ChartContractError> {
    if document.schema_version != FIXED_DOCUMENT_SCHEMA_VERSION {
        return Err(ChartContractError::UnsupportedVersion {
            found: document.schema_version,
            supported: FIXED_DOCUMENT_SCHEMA_VERSION,
        });
    }
    validate_interactive_dataset(&document.dataset)
}

pub(crate) fn validate_registry(registry: &ChartRegistry) -> Result<(), ChartContractError> {
    if registry.schema_version != REGISTRY_SCHEMA_VERSION {
        return Err(ChartContractError::UnsupportedVersion {
            found: registry.schema_version,
            supported: REGISTRY_SCHEMA_VERSION,
        });
    }
    if registry.tickers.as_array().is_none_or(Vec::is_empty)
        || registry.chart_tfs.as_array().is_none_or(Vec::is_empty)
    {
        return Err(ChartContractError::EmptyRegistry);
    }
    Ok(())
}

fn record_time_ms(record: &ChartRecord) -> Option<i64> {
    record
        .get("time")
        .or_else(|| record.get("time_ms"))
        .and_then(|value| {
            value.as_i64().or_else(|| {
                let value = value.as_f64()?;
                (value.is_finite() && value.fract() == 0.0).then_some(value as i64)
            })
        })
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), ChartContractError> {
    if value.trim().is_empty() {
        return Err(ChartContractError::InvalidIdentity { field });
    }
    Ok(())
}

/// Converts a raw zone to the exact production `gapZones` object shape.
#[must_use]
pub fn gap_zone_to_json(zone: &GapZone) -> Value {
    serde_json::json!({
        "time_ms": zone.time_ms,
        "open": zone.open,
        "high": zone.high,
        "low": zone.low,
        "close": zone.close,
        "volume": zone.volume,
        "body_bottom": zone.body_bottom,
        "body_top": zone.body_top,
        "body_ratio": zone.body_ratio,
        "direction": zone.direction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(time: i64) -> ChartRecord {
        Map::from_iter([(String::from("time"), Value::from(time))])
    }

    fn zone() -> GapZone {
        GapZone {
            time_ms: 1_700_000_000_000,
            open: 100.0,
            high: 115.0,
            low: 95.0,
            close: 110.0,
            volume: 1_000.0,
            body_bottom: 100.0,
            body_top: 110.0,
            body_ratio: Some(0.8),
            direction: GapDirection::Bullish,
        }
    }

    fn dataset(records: Vec<ChartRecord>) -> InteractiveDataset {
        InteractiveDataset {
            key: DatasetKey::new("BTC-USDT", "1h"),
            display_symbol: String::from("BingX:BTC-USDT"),
            records,
            gap_zones: vec![zone()],
        }
    }

    #[test]
    fn dataset_validation_rejects_empty_and_non_increasing_records() {
        assert!(matches!(
            validate_interactive_dataset(&dataset(vec![])),
            Err(ChartContractError::EmptyDataset)
        ));
        assert!(matches!(
            validate_interactive_dataset(&dataset(vec![record(2), record(1)])),
            Err(ChartContractError::NonIncreasingTimestamp { .. })
        ));
    }

    #[test]
    fn gap_zone_to_json_emits_exactly_raw_keys() {
        let value = gap_zone_to_json(&zone());
        let object = value.as_object().expect("gap zone JSON must be an object");
        let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "body_bottom",
                "body_ratio",
                "body_top",
                "close",
                "direction",
                "high",
                "low",
                "open",
                "time_ms",
                "volume",
            ]
        );
        for forbidden in ["rssi", "rssi_ma", "trust", "raw", "candle", "ema"] {
            assert!(!object.contains_key(forbidden), "forbidden key {forbidden}");
        }
    }
}

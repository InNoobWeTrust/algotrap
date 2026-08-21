//! Engine adapter from computed-frame JSON records to typed TA gap-zone inputs.

use serde_json::{Map, Value};

use crate::engine::{error::MarketError, traits::ComputedFrame};
use crate::ta::gap_zones::{GapCandle, GapZone, GapZoneParams, extract_gap_zones};

/// Extracts typed gap zones from a computed frame while preserving legacy malformed-row skipping.
pub fn extract_gap_zones_from_frame(
    frame: &dyn ComputedFrame,
    params: &GapZoneParams,
) -> Result<Vec<GapZone>, MarketError> {
    extract_gap_zones_from_records(&frame.to_json_records()?, params)
}

/// Adapts DuckDB-compatible result records to typed gap candles before pure extraction.
pub fn extract_gap_zones_from_records(
    rows: &[Map<String, Value>],
    params: &GapZoneParams,
) -> Result<Vec<GapZone>, MarketError> {
    let candles = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| record_to_gap_candle(row, rows.len() - index - 1))
        .collect::<Vec<_>>();
    Ok(extract_gap_zones(&candles, params)?)
}

fn record_to_gap_candle(row: &Map<String, Value>, age_bars: usize) -> Option<GapCandle> {
    row.get("is_atr_gap")?.as_bool().filter(|gap| *gap)?;
    let candle = GapCandle {
        open: finite_number(row, "open")?,
        close: finite_number(row, "close")?,
        is_atr_gap: true,
        body_ratio: finite_number(row, "body_ratio")?,
        rssi: row
            .get("rssi")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite()),
        age_bars,
    };
    candle.validate().ok().map(|_| candle)
}

fn finite_number(row: &Map<String, Value>, field: &str) -> Option<f64> {
    row.get(field)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    struct FixtureFrame(Vec<Map<String, Value>>);

    impl ComputedFrame for FixtureFrame {
        fn len(&self) -> usize {
            self.0.len()
        }

        fn columns(&self) -> Vec<String> {
            vec![
                "open".into(),
                "close".into(),
                "is_atr_gap".into(),
                "body_ratio".into(),
            ]
        }

        fn slice_last(&self, count: usize) -> Result<Box<dyn ComputedFrame>, MarketError> {
            let start = self.0.len().saturating_sub(count);
            Ok(Box::new(Self(self.0[start..].to_vec())))
        }

        fn f64_at(&self, column: &str, row: usize) -> Result<Option<f64>, MarketError> {
            self.0
                .get(row)
                .and_then(|record| record.get(column))
                .and_then(Value::as_f64)
                .map(Some)
                .ok_or_else(|| MarketError::data_access("fixture numeric cell is unavailable"))
        }

        fn string_at(&self, _: &str, _: usize) -> Result<Option<String>, MarketError> {
            Ok(None)
        }

        fn to_json_records(&self) -> Result<Vec<Map<String, Value>>, MarketError> {
            Ok(self.0.clone())
        }

        fn has_column(&self, column: &str) -> bool {
            self.0.iter().any(|record| record.contains_key(column))
        }
    }

    #[test]
    fn record_adapter_preserves_legacy_gap_zone_output_and_invalid_row_skipping() {
        let records = vec![
            json!({"open": 100.0, "close": 105.0, "is_atr_gap": true, "body_ratio": 0.5, "rssi": 70.0}),
            json!({"open": 1.0, "close": 2.0, "is_atr_gap": true, "body_ratio": null}),
        ]
        .into_iter()
        .map(|value| value.as_object().unwrap().clone())
        .collect::<Vec<_>>();
        let zones = extract_gap_zones_from_records(&records, &GapZoneParams::default()).unwrap();
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].bottom, 100.0);
        assert_eq!(zones[0].top, 105.0);
        assert_eq!(zones[0].trust, 0.35);
    }

    #[test]
    fn frame_adapter_matches_record_adapter_output() {
        let records = vec![json!({"open": 100.0, "close": 105.0, "is_atr_gap": true, "body_ratio": 0.5, "rssi": 70.0})]
            .into_iter()
            .map(|value| value.as_object().unwrap().clone())
            .collect::<Vec<_>>();
        let params = GapZoneParams::default();
        assert_eq!(
            extract_gap_zones_from_frame(&FixtureFrame(records.clone()), &params).unwrap(),
            extract_gap_zones_from_records(&records, &params).unwrap()
        );
    }
}

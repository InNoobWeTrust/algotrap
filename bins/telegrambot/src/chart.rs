use algotrap::engine::traits::ComputedFrame;
use algotrap::prelude::Timeframe;
use algotrap::query::gap_zones::{GapZoneDirection, GapZoneRecord};
use algotrap::ta::materialize_iching_forecast;
use algotrap::ta::prelude::{LeapMonthPolicy, plum_blossom_signal_with_policy};
use algotrap::time_utils::iching_forecast_horizon;
use chartlib::{
    ChartRecord, DatasetKey, DocumentKind, FIXED_DOCUMENT_SCHEMA_VERSION, FixedChartDocument,
    ForecastRecord, GapDirection, GapZone, InteractiveDataset,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::config::TickerConf;

/// Builds a fixed chart from Telegram's own projected indicators in the canonical flat format.
pub fn fixed_chart_document(
    tf: &Timeframe,
    df: &dyn ComputedFrame,
    ticker: &TickerConf,
    gap_zones: &[GapZoneRecord],
    as_of: DateTime<Utc>,
) -> Result<FixedChartDocument, Box<dyn core::error::Error + Send + Sync>> {
    fixed_chart_document_at(tf, df, ticker, gap_zones, as_of)
}

fn fixed_chart_document_at(
    tf: &Timeframe,
    df: &dyn ComputedFrame,
    ticker: &TickerConf,
    gap_zones: &[GapZoneRecord],
    as_of: DateTime<Utc>,
) -> Result<FixedChartDocument, Box<dyn core::error::Error + Send + Sync>> {
    let records = (0..df.len())
        .map(|row| flat_record(df, row))
        .collect::<Result<Vec<_>, _>>()?;
    let forecast = if let Some(last) = records.last() {
        let last_open = last["time"]
            .as_f64()
            .ok_or("last chart timestamp is missing")? as i64;
        let bars = iching_forecast_horizon(last_open, *tf, as_of);
        let forecast = materialize_iching_forecast(&bars)?;
        (!forecast.is_empty()).then(|| {
            forecast
                .into_iter()
                .map(|bar| {
                    let values = bar.rendered_pane3_values();
                    ForecastRecord {
                        time: bar.bar_open_ms,
                        original: values.original,
                        transformed: values.transformed,
                        mutual_high: values.mutual_high,
                        mutual_low: values.mutual_low,
                        mutual_mean: values.mutual_mean,
                    }
                })
                .collect()
        })
    } else {
        None
    };
    let gap_zones = gap_zones
        .iter()
        .map(|zone| GapZone {
            time_ms: zone.time_ms,
            open: zone.open,
            high: zone.high,
            low: zone.low,
            close: zone.close,
            volume: zone.volume,
            body_bottom: zone.body_bottom,
            body_top: zone.body_top,
            body_ratio: zone.body_ratio,
            direction: match zone.direction {
                GapZoneDirection::Bullish => GapDirection::Bullish,
                GapZoneDirection::Bearish => GapDirection::Bearish,
                GapZoneDirection::Flat => GapDirection::Flat,
            },
        })
        .collect();
    Ok(FixedChartDocument {
        schema_version: FIXED_DOCUMENT_SCHEMA_VERSION,
        kind: DocumentKind::FixedDocument,
        title: format!("BingX:{} {}", ticker.symbol, tf),
        subtitle: None,
        dataset: InteractiveDataset {
            key: DatasetKey::new(&ticker.symbol, tf.to_string()),
            display_symbol: format!("BingX:{}", ticker.symbol),
            records,
            gap_zones,
            forecast,
        },
    })
}

/// Renders one caller-selected Telegram dataset through the production four-pane template.
pub fn render_single_tf_chart_html(
    tf: &Timeframe,
    df: &dyn ComputedFrame,
    ticker: &TickerConf,
    gap_zones: &[GapZoneRecord],
    as_of: DateTime<Utc>,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let document = fixed_chart_document(tf, df, ticker, gap_zones, as_of)?;
    chartlib::render_fixed_html(&document).map_err(Into::into)
}

fn flat_record(
    df: &dyn ComputedFrame,
    row: usize,
) -> Result<ChartRecord, Box<dyn core::error::Error + Send + Sync>> {
    let mut record = ChartRecord::new();
    for field in ["time", "open", "high", "low", "close", "volume"] {
        insert_required(df, &mut record, field, row)?;
    }
    for field in [
        "volume_sma",
        "ema200",
        "bias_reversion",
        "atr_upperband",
        "atr_lowerband",
        "neutral_revrsi",
        "bullish_revrsi",
        "bearish_revrsi",
        "structure_power",
        "structure_power_sma",
        "atr_percent",
        "atr_reversion_percent",
        "leverage",
        "iching_open",
        "iching_high",
        "iching_low",
        "iching_close",
        "iching_moving_line",
        "iching_transformed_close",
        "iching_mutual_close",
        "iching_mutual_high",
        "iching_mutual_low",
        "iching_mutual_mean",
    ] {
        insert_optional(df, &mut record, field, row)?;
    }
    let time_ms = required_value(df, "time", row)? as i64;
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(time_ms)
        .ok_or_else(|| format!("invalid I-Ching timestamp: {time_ms}"))?;
    // `LeapMonthPolicy::Allow` (not the `Reject` default): historical candles on 1d/1w/1M
    // spans cover lunar leap months; Reject would error and silently drop those chart rows.
    // Keep consistent with cryptobot's iching presentation path.
    let signal = plum_blossom_signal_with_policy(timestamp, LeapMonthPolicy::Allow)
        .map_err(|error| format!("I-Ching calculation failed at {time_ms}: {error}"))?;
    let transformed = signal
        .transformed
        .ok_or_else(|| format!("I-Ching transformed channel is missing at {time_ms}"))?;
    record.insert(
        String::from("iching_original_energy"),
        Value::from(signal.original.energy),
    );
    record.insert(
        String::from("iching_transformed_energy"),
        Value::from(transformed.energy),
    );
    record.insert(
        String::from("iching_mutual_energy"),
        Value::from(signal.mutual.energy),
    );

    let close = required_value(df, "close", row)?;
    let open = required_value(df, "open", row)?;
    record.insert(
        String::from("volume_color"),
        Value::from(if close >= open {
            "rgba(76, 175, 80, 0.3)"
        } else {
            "rgba(242, 54, 69, 0.3)"
        }),
    );
    for (field, color) in [
        ("bias_reversion_color", "rgba(178, 181, 190, 0.2)"),
        ("ema200_color", "rgba(156, 39, 176, 0.5)"),
        ("neutral_revrsi_color", "rgba(178,181,190,0.2)"),
        ("bullish_revrsi_color", "rgba(33,150,243,0.2)"),
        ("bearish_revrsi_color", "rgba(255,152,0,0.2)"),
        ("atr_upperband_color", "rgba(76, 175, 80, 0.2)"),
        ("atr_lowerband_color", "rgba(242, 54, 69, 0.2)"),
    ] {
        record.insert(String::from(field), Value::from(color));
    }
    let power = optional_number(df, "structure_power", row)?;
    let smoothing = optional_number(df, "structure_power_sma", row)?;
    if let (Some(power), Some(smoothing)) = (power, smoothing) {
        record.insert(
            String::from("structure_power_direction"),
            Value::from(3.0 * power - 2.0 * smoothing),
        );
    } else {
        record.insert(String::from("structure_power_direction"), Value::Null);
    }
    let atr_reversion = optional_number(df, "atr_reversion_percent", row)?;
    let atr_reversion_color = match atr_reversion {
        Some(value) if value > 50.0 => "rgba(76, 175, 80, 0.5)",
        Some(value) if value < -50.0 => "rgba(242, 54, 69, 0.5)",
        _ => "rgba(41, 98, 255, 0.2)",
    };
    record.insert(
        String::from("atr_reversion_percent_color"),
        Value::from(atr_reversion_color),
    );
    Ok(record)
}

fn insert_required(
    df: &dyn ComputedFrame,
    record: &mut ChartRecord,
    field: &str,
    row: usize,
) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    record.insert(
        String::from(field),
        Value::from(required_value(df, field, row)?),
    );
    Ok(())
}

fn insert_optional(
    df: &dyn ComputedFrame,
    record: &mut ChartRecord,
    field: &str,
    row: usize,
) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    if let Some(value) = optional_number(df, field, row)? {
        record.insert(String::from(field), Value::from(value));
    } else if df.has_column(field) {
        record.insert(String::from(field), Value::Null);
    }
    Ok(())
}

fn required_value(
    df: &dyn ComputedFrame,
    field: &str,
    row: usize,
) -> Result<f64, Box<dyn core::error::Error + Send + Sync>> {
    optional_number(df, field, row)?
        .ok_or_else(|| format!("required chart value {field} is missing at row {row}").into())
}

fn optional_number(
    df: &dyn ComputedFrame,
    field: &str,
    row: usize,
) -> Result<Option<f64>, Box<dyn core::error::Error + Send + Sync>> {
    if !df.has_column(field) {
        return Ok(None);
    }
    df.f64_at(field, row).map_err(Into::into)
}

#[cfg(test)]
mod chart_tests {
    use super::*;
    use algotrap::engine::frame::{SourceColumnData, SourceFrame};

    fn ticker() -> TickerConf {
        TickerConf {
            symbol: String::from("BTC-USDT"),
            sl_percent: 0.02,
            tol_percent: 0.01,
            tfs: vec![Timeframe::H1],
            default_tf: Timeframe::H1,
        }
    }

    fn frame() -> SourceFrame {
        let values = |items| SourceColumnData::Number(items);
        SourceFrame::from_columns(vec![
            (
                "time".into(),
                values(vec![Some(1_704_067_200_000.0), Some(1_704_070_800_000.0)]),
            ),
            ("open".into(), values(vec![Some(100.0), Some(101.0)])),
            ("high".into(), values(vec![Some(102.0), Some(103.0)])),
            ("low".into(), values(vec![Some(99.0), Some(100.0)])),
            ("close".into(), values(vec![Some(101.0), Some(102.0)])),
            ("volume".into(), values(vec![Some(1_000.0), Some(1_100.0)])),
            ("volume_sma".into(), values(vec![Some(900.0), Some(950.0)])),
            ("structure_power".into(), values(vec![Some(2.0), Some(3.0)])),
            ("structure_power_sma".into(), values(vec![None, Some(2.5)])),
            (
                "atr_reversion_percent".into(),
                values(vec![Some(0.0), Some(1.0)]),
            ),
        ])
        .expect("source frame")
    }

    #[test]
    fn forecast_serializes_separately_from_candles() {
        let as_of = chrono::DateTime::from_timestamp_millis(1_704_070_800_000).unwrap();
        let document =
            fixed_chart_document(&Timeframe::H1, &frame(), &ticker(), &[], as_of).unwrap();
        let mut plain = document.clone();
        plain.dataset.forecast = None;
        let actual = serde_json::to_value(&document.dataset).unwrap();
        let baseline = serde_json::to_value(&plain.dataset).unwrap();
        assert_eq!(actual["forecast"].as_array().unwrap().len(), 10);
        assert!(baseline.get("forecast").is_none());
        assert_eq!(
            serde_json::to_vec(&actual["records"]).unwrap(),
            serde_json::to_vec(&baseline["records"]).unwrap()
        );
        assert!(
            document
                .dataset
                .records
                .iter()
                .all(|record| !record.contains_key("original"))
        );
        assert_eq!(
            serde_json::to_vec(&document).unwrap(),
            serde_json::to_vec(
                &fixed_chart_document(&Timeframe::H1, &frame(), &ticker(), &[], as_of).unwrap()
            )
            .unwrap()
        );
    }

    #[test]
    fn fixed_document_keeps_market_records_unchanged() {
        let as_of = chrono::DateTime::from_timestamp_millis(1_704_070_800_000).unwrap();
        let document =
            fixed_chart_document(&Timeframe::H1, &frame(), &ticker(), &[], as_of).unwrap();
        let expected = (0..frame().len())
            .map(|row| flat_record(&frame(), row).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(document.dataset.records, expected);
        assert_eq!(document.dataset.forecast.as_ref().unwrap().len(), 10);
    }

    #[test]
    fn forecast_counts_match_timeframe_horizons() {
        let as_of = chrono::DateTime::from_timestamp_millis(1_704_070_800_000).unwrap();
        for (tf, count) in [
            (Timeframe::H1, 10),
            (Timeframe::W1, 5),
            (Timeframe::MOS1, 2),
        ] {
            let document = fixed_chart_document(&tf, &frame(), &ticker(), &[], as_of).unwrap();
            assert_eq!(document.dataset.forecast.unwrap().len(), count);
        }
    }

    #[test]
    fn adapter_preserves_flat_records_and_directional_warmup_null() {
        let document = fixed_chart_document(&Timeframe::H1, &frame(), &ticker(), &[], Utc::now())
            .expect("document");
        assert_eq!(document.dataset.records.len(), 2);
        assert_eq!(
            document.dataset.records[0]["structure_power_direction"],
            Value::Null
        );
        assert_eq!(
            document.dataset.records[1]["structure_power_direction"],
            Value::from(4.0)
        );
        assert!(document.dataset.records[0].contains_key("iching_original_energy"));
    }

    #[test]
    fn adapter_survives_lunar_leap_month_dates() {
        // 2020-05-23T04:00:00Z is inside the lunar leap 4th month (see
        // ta::iching::signal::tests::find_2020_leap_solar_date). With the old
        // Reject default, plum_blossom_signal errors and the whole
        // fixed_chart_document aborts, dropping every candle that spans a leap
        // month — holes in 1d/1w/1M chart renders.
        let leap_ms: i64 = 1_590_211_200_000; // 2020-05-23T04:00:00Z == leap date
        let ts = chrono::DateTime::from_timestamp_millis(leap_ms).unwrap();

        // Sanity guard: the chosen instant is actually a leap month under Reject,
        // so the Allow assertion below cannot be vacuous.
        assert!(
            plum_blossom_signal_with_policy(ts, LeapMonthPolicy::Reject).is_err(),
            "sanity: 2020-05-23T04:00Z must be a leap-month date, else this test is vacuous"
        );

        let frame = SourceFrame::from_columns(vec![
            (
                "time".into(),
                SourceColumnData::Number(vec![Some(leap_ms as f64)]),
            ),
            ("open".into(), SourceColumnData::Number(vec![Some(100.0)])),
            ("high".into(), SourceColumnData::Number(vec![Some(102.0)])),
            ("low".into(), SourceColumnData::Number(vec![Some(99.0)])),
            ("close".into(), SourceColumnData::Number(vec![Some(101.0)])),
            (
                "volume".into(),
                SourceColumnData::Number(vec![Some(1_000.0)]),
            ),
        ])
        .expect("source frame");
        let document = fixed_chart_document(&Timeframe::H1, &frame, &ticker(), &[], Utc::now())
            .expect("leap-month render");
        assert!(
            document.dataset.records[0]["iching_original_energy"].is_number(),
            "leap-month candle must still emit an I-Ching energy value"
        );
    }

    #[test]
    fn fixed_render_uses_shared_canonical_template() {
        let html =
            render_single_tf_chart_html(&Timeframe::H1, &frame(), &ticker(), &[], Utc::now())
                .expect("render");
        assert!(html.contains("chartlib-fixed"));
        assert!(html.contains("LightweightCharts.createChart"));
        assert!(html.contains("ICHING") || html.contains("I-Ching"));
        assert!(!html.to_lowercase().contains("rssi"));
    }

    #[test]
    fn adapter_rejects_missing_required_candle_value() {
        let frame = SourceFrame::from_columns(vec![
            (
                "time".into(),
                SourceColumnData::Number(vec![Some(1_704_067_200_000.0)]),
            ),
            ("open".into(), SourceColumnData::Number(vec![Some(100.0)])),
        ])
        .expect("source frame");
        let error = fixed_chart_document(&Timeframe::H1, &frame, &ticker(), &[], Utc::now())
            .expect_err("missing close");
        assert!(error.to_string().contains("required chart value high"));
    }
}

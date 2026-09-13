use algotrap::engine::traits::ComputedFrame;
use algotrap::prelude::*;
use minijinja::render;

use crate::config::TickerConf;

// ─── Chart Column Registry ───────────────────────────────────────────────────

/// Canonical list of derived indicator columns available to the chart template.
///
/// OHLCV base columns (time, open, high, low, close, volume) are implicit.
/// Update this list whenever the Telegram presentation output contract changes.
pub const CHART_COLUMNS: &[&str] = &[
    "volume_sma",
    "bias_reversion",
    "ema200",
    "neutral_revrsi",
    "bullish_revrsi",
    "bearish_revrsi",
    "atr_upperband",
    "atr_lowerband",
    "atr_percent",
    "structure_power",
    "structure_power_sma",
    "rssi",
    "rssi_ma",
    "atr_reversion_percent",
    "leverage",
    "sharpe",
    "is_atr_gap",
    "body_ratio",
];

// ─── Chart Rendering ─────────────────────────────────────────────────────────

/// Render a chart HTML page for a **single** timeframe.
///
/// Produces a self-contained HTML page with LightweightCharts showing
/// exactly one timeframe's data. Suitable for Browserless screenshot
/// capture — no interactive toggle needed.
pub fn render_single_tf_chart_html(
    tf: &Timeframe,
    df: &dyn ComputedFrame,
    ticker: &TickerConf,
    gap_zones_json: &str,
    rssi_tint: &str,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let records = df
        .to_json_records()
        .map_err(|e| std::io::Error::other(format!("{e}")))?;
    let df_json =
        serde_json::Value::Array(records.into_iter().map(serde_json::Value::Object).collect());
    let dataset = serde_json::to_string(&df_json)?;

    Ok(render!(
        TDV_HTML_TEMPLATE,
        dataset => dataset,
        symbol => format!("BingX:{}", ticker.symbol),
        tf => tf.to_string(),
        sl_percent => format!("{:.0}", ticker.sl_percent * 100.),
        tol_percent => format!("{:.2}", ticker.tol_percent * 100.),
        gap_zones => gap_zones_json,
        rssi_tint => rssi_tint,
    )
    .trim()
    .to_string())
}

/// Determine RSSI background tint class from the last RSSI value.
pub fn rssi_tint_class(last_rssi: f64) -> &'static str {
    match last_rssi {
        r if r >= 60.0 => "bullish",
        r if r <= 40.0 => "bearish",
        _ => "neutral",
    }
}

/// Extract the last RSSI value from a ComputedFrame, defaulting to 50.0.
pub fn last_rssi_from_df(df: &dyn ComputedFrame) -> f64 {
    let last_row = df.len().saturating_sub(1);
    df.f64_at("rssi", last_row).ok().flatten().unwrap_or(50.0)
}

/// Convert gap zones to chart-level JSON for band rendering.
///
/// Takes pre-budgeted [`algotrap::query::gap_zones::GapZoneRecord`]s and emits
/// `{top, bottom, direction}` per zone with no trust weighting, filtering, or
/// truncation — the caller already budgeted via `recent_gap_zones`.
pub fn gap_zones_to_chart_json(
    zones: &[algotrap::query::gap_zones::GapZoneRecord],
) -> String {
    let chart_zones: Vec<serde_json::Value> = zones
        .iter()
        .map(|z| {
            let direction = match z.direction {
                algotrap::query::gap_zones::GapZoneDirection::Bullish => "bullish",
                algotrap::query::gap_zones::GapZoneDirection::Bearish => "bearish",
                algotrap::query::gap_zones::GapZoneDirection::Flat => "flat",
            };
            serde_json::json!({
                "top": z.body_top,
                "bottom": z.body_bottom,
                "direction": direction,
            })
        })
        .collect();
    serde_json::to_string(&chart_zones).unwrap_or_else(|_| "[]".to_string())
}

// ─── Chart HTML Template ─────────────────────────────────────────────────────

const TDV_HTML_TEMPLATE: &str = include_str!("chart_template.html");

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod chart_tests {
    use super::*;
    use std::collections::HashSet;

    const BASE_COLUMNS: &[&str] = &["time", "open", "high", "low", "close", "volume"];

    #[test]
    fn chart_template_references_only_known_columns() {
        let template = include_str!("chart_template.html");

        // Match `d.xxx` and `d["xxx"]` patterns in JS
        let re = regex::Regex::new(r#"d\.([a-z_][a-z0-9_]*)|d\["([a-z_][a-z0-9_]*)"\]"#).unwrap();
        let referenced: HashSet<&str> = re
            .captures_iter(template)
            .filter_map(|c| c.get(1).or(c.get(2)).map(|m| m.as_str()))
            .filter(|k| !BASE_COLUMNS.contains(k))
            .collect();

        let known: HashSet<&str> = CHART_COLUMNS.iter().copied().collect();

        let unknown: Vec<&&str> = referenced.difference(&known).collect();
        assert!(
            unknown.is_empty(),
            "Chart template references unknown columns: {:?}\n\
             Either add them to CHART_COLUMNS or remove from the template.",
            unknown
        );
    }

    #[test]
    fn rssi_tint_class_boundaries() {
        assert_eq!(rssi_tint_class(60.0), "bullish");
        assert_eq!(rssi_tint_class(75.0), "bullish");
        assert_eq!(rssi_tint_class(40.0), "bearish");
        assert_eq!(rssi_tint_class(20.0), "bearish");
        assert_eq!(rssi_tint_class(50.0), "neutral");
        assert_eq!(rssi_tint_class(59.9), "neutral");
        assert_eq!(rssi_tint_class(40.1), "neutral");
    }

    #[test]
    fn gap_zones_to_chart_json_emits_top_bottom_direction_without_trust() {
        use algotrap::query::gap_zones::{GapZoneDirection, GapZoneRecord};

        fn record(direction: GapZoneDirection, bottom: f64, top: f64) -> GapZoneRecord {
            GapZoneRecord {
                time_ms: 1_700_000_000_000,
                open: 100.0,
                high: 115.0,
                low: 95.0,
                close: 110.0,
                volume: 1_000.0,
                body_bottom: bottom,
                body_top: top,
                direction,
                body_ratio: Some(0.8),
            }
        }

        let zones = vec![
            record(GapZoneDirection::Bullish, 100.0, 110.0),
            record(GapZoneDirection::Bearish, 90.0, 100.0),
            record(GapZoneDirection::Flat, 100.0, 100.0),
        ];
        let json = gap_zones_to_chart_json(&zones);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["top"], serde_json::json!(110.0));
        assert_eq!(arr[0]["bottom"], serde_json::json!(100.0));
        assert_eq!(arr[0]["direction"], serde_json::json!("bullish"));
        assert_eq!(arr[1]["direction"], serde_json::json!("bearish"));
        assert_eq!(arr[2]["direction"], serde_json::json!("flat"));
        for entry in arr {
            let obj = entry.as_object().unwrap();
            assert!(obj.contains_key("top"));
            assert!(obj.contains_key("bottom"));
            assert!(obj.contains_key("direction"));
            assert!(
                !obj.contains_key("trust"),
                "chart JSON must not carry trust: {obj:?}"
            );
            assert_eq!(obj.len(), 3);
        }
        assert_eq!(gap_zones_to_chart_json(&[]), "[]");
    }
}

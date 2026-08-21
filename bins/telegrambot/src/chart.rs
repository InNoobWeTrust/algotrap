use algotrap::engine::traits::ComputedFrame;
use algotrap::prelude::*;
use minijinja::render;

use crate::config::TickerConf;

// ─── Chart Column Registry ───────────────────────────────────────────────────

/// Canonical list of derived indicator columns available to the chart template.
///
/// OHLCV base columns (time, open, high, low, close, volume) are implicit.
/// Update this list whenever `data.rs:indicators()` changes column aliases.
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

#[allow(dead_code)]
const BASE_COLUMNS: &[&str] = &["time", "open", "high", "low", "close", "volume"];

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
/// Takes at most 10 most recent zones above min_trust.
pub fn gap_zones_to_chart_json(
    zones: &[algotrap::ta::gap_zones::GapZone],
    min_trust: f64,
) -> String {
    let chart_zones: Vec<serde_json::Value> = zones
        .iter()
        .filter(|z| z.trust >= min_trust)
        .take(10) // hardcoded cap for visual clarity
        .map(|z| {
            let direction = if z.bullish { "bullish" } else { "bearish" };
            serde_json::json!({
                "top": z.top,
                "bottom": z.bottom,
                "direction": direction,
                "trust": z.trust
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
}

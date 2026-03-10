use algotrap::df_utils::JsonDataframe;
use algotrap::prelude::*;
use minijinja::render;
use polars::prelude::DataFrame;
use serde_json::Value;

use crate::config::EnvConf;

/// Render a chart HTML page for a **single** timeframe.
///
/// Produces a self-contained HTML page with LightweightCharts showing
/// exactly one timeframe's data. Suitable for Browserless screenshot
/// capture — no interactive toggle needed.
pub fn render_single_tf_chart_html(
    tf: &Timeframe,
    df: &DataFrame,
    conf: &EnvConf,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let df_json: JsonDataframe = df
        .try_into()
        .expect("Failed to serialize data frame to json");
    let df_json: Value = df_json.into();
    let dataset = serde_json::to_string(&df_json)?;

    Ok(render!(
        TDV_HTML_TEMPLATE,
        dataset => dataset,
        symbol => format!("BingX:{}", conf.symbol),
        tf => tf.to_string(),
        sl_percent => format!("{:.0}", conf.sl_percent * 100.),
        tol_percent => format!("{:.2}", conf.tol_percent * 100.)
    )
    .trim()
    .to_string())
}

// ─── Chart HTML Template ─────────────────────────────────────────────────────

const TDV_HTML_TEMPLATE: &str = include_str!("chart_template.html");

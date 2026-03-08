use std::collections::HashMap;

use algotrap::df_utils::JsonDataframe;
use algotrap::prelude::*;
use minijinja::render;
use rayon::prelude::*;
use serde_json::Value;

use crate::config::EnvConf;

/// Render the multi-timeframe chart as an HTML string.
///
/// Produces a self-contained HTML page with LightweightCharts.
/// Suitable for Browserless screenshot or direct browser viewing.
pub fn render_chart_html(
    all_dfs: &HashMap<Timeframe, polars::prelude::DataFrame>,
    conf: &EnvConf,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let all_dfs_serialized: HashMap<String, Value> = all_dfs
        .par_iter()
        .map(|(tf, df)| {
            let df_json: JsonDataframe = df
                .try_into()
                .expect("Failed to serialize data frame to json");
            let df_json: Value = df_json.into();
            (tf.to_string(), df_json)
        })
        .collect();
    let df_json = serde_json::to_string(&all_dfs_serialized)?;
    let tfs_json = serde_json::to_string(&conf.tfs)?;

    Ok(render!(
        TDV_HTML_TEMPLATE,
        dataset => df_json,
        symbol => format!("BingX:{}", conf.symbol),
        tfs => tfs_json,
        default_tf => conf.default_tf.to_string(),
        sl_percent => format!("{:.0}", conf.sl_percent * 100.),
        tol_percent => format!("{:.2}", conf.tol_percent * 100.)
    )
    .trim()
    .to_string())
}

// ─── Chart HTML Template (adapted from cryptobot) ────────────────────────────

const TDV_HTML_TEMPLATE: &str = include_str!("chart_template.html");

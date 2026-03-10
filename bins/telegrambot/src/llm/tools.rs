use std::collections::HashMap;

use algotrap::prelude::*;
use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionTool, ChatCompletionTools, FunctionObjectArgs,
};
use polars::prelude::*;
use tracing::warn;

use crate::browserless::capture_chart_screenshot;
use crate::chart::render_single_tf_chart_html;
use crate::config::EnvConf;

/// Build LLM tool definitions by loading schemas from `tools.json`.
///
/// Only the tool *schemas* (name, description, parameters) are externalized.
/// Tool *execution* logic stays in Rust — adding a new tool still requires
/// a code change in `execute_tool_call`, but tweaking descriptions or
/// parameter docs is a config-only change.
pub fn build_tools(
    conf: &EnvConf,
) -> Result<Vec<ChatCompletionTools>, Box<dyn core::error::Error + Send + Sync>> {
    let path = std::path::Path::new(&conf.prompts_dir).join("tools.json");
    let json_str = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read tool schemas from {}: {e}", path.display()))?;

    let schemas: Vec<serde_json::Value> = serde_json::from_str(&json_str)?;

    let tools = schemas
        .into_iter()
        .map(|schema| {
            let name = schema["name"].as_str().unwrap_or("unknown").to_string();
            let description = schema["description"].as_str().unwrap_or("").to_string();
            let parameters = schema.get("parameters").cloned().unwrap_or(serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }));

            ChatCompletionTools::Function(ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name(name)
                    .description(description)
                    .parameters(parameters)
                    .build()
                    .expect("Failed to build tool function"),
            })
        })
        .collect();

    Ok(tools)
}

/// Execute a tool call and return the result as a string,
/// plus an optional chart screenshot.
pub async fn execute_tool_call(
    tool_call: &ChatCompletionMessageToolCall,
    all_dfs: &HashMap<Timeframe, DataFrame>,
    conf: &EnvConf,
    chart_screenshots: &mut HashMap<String, Vec<u8>>,
) -> Result<(String, Option<Vec<u8>>), Box<dyn core::error::Error + Send + Sync>> {
    let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)?;

    match tool_call.function.name.as_str() {
        "get_indicator_summary" => {
            let tf_str = args["timeframe"].as_str().unwrap_or("4h");
            let tf: Timeframe = tf_str
                .parse()
                .map_err(|e: String| -> Box<dyn core::error::Error + Send + Sync> { e.into() })?;
            match all_dfs.get(&tf) {
                Some(df) => {
                    let last_rows = df.slice(-3, 3);
                    let summary = extract_indicator_summary(&last_rows, &tf)?;
                    Ok((summary, None))
                }
                None => Ok((
                    format!("Timeframe {tf_str} not available. Available: {:?}", conf.tfs),
                    None,
                )),
            }
        }
        "get_price_action" => {
            let tf_str = args["timeframe"].as_str().unwrap_or("4h");
            let num = args["num_candles"].as_u64().unwrap_or(5).min(20) as usize;
            let tf: Timeframe = tf_str
                .parse()
                .map_err(|e: String| -> Box<dyn core::error::Error + Send + Sync> { e.into() })?;
            match all_dfs.get(&tf) {
                Some(df) => {
                    let rows = df.slice(-(num as i64), num);
                    let price_data = extract_price_action(&rows)?;
                    Ok((price_data, None))
                }
                None => Ok((
                    format!("Timeframe {tf_str} not available. Available: {:?}", conf.tfs),
                    None,
                )),
            }
        }
        "capture_chart" => {
            let default_tf_str = conf.default_tf.to_string();
            let tf_str = args["timeframe"].as_str().unwrap_or(&default_tf_str);

            // Check cache first
            if let Some(cached) = chart_screenshots.get(tf_str) {
                return Ok((
                    format!("[Chart screenshot for {tf_str} captured — see attached image]"),
                    Some(cached.clone()),
                ));
            }

            // Parse timeframe and render per-TF chart HTML
            let tf: Timeframe = tf_str
                .parse()
                .map_err(|e: String| -> Box<dyn core::error::Error + Send + Sync> { e.into() })?;
            let df = match all_dfs.get(&tf) {
                Some(df) => df,
                None => {
                    return Ok((
                        format!("Timeframe {tf_str} not available. Available: {:?}", conf.tfs),
                        None,
                    ));
                }
            };
            let chart_html = render_single_tf_chart_html(&tf, df, conf)?;

            match capture_chart_screenshot(&chart_html, &conf.browserless_url).await {
                Ok(png) => {
                    chart_screenshots.insert(tf_str.to_string(), png.clone());
                    Ok((
                        format!("[Chart screenshot for {tf_str} captured — see attached image]"),
                        Some(png),
                    ))
                }
                Err(e) => {
                    warn!("Failed to capture chart screenshot: {e}");
                    Ok((
                        format!(
                            "Failed to capture chart: {e}. Proceeding with data-only analysis."
                        ),
                        None,
                    ))
                }
            }
        }
        "get_multi_tf_overview" => {
            let overview = build_multi_tf_overview(all_dfs, conf)?;
            Ok((overview, None))
        }
        _ => Ok((
            format!("Unknown tool: {}", tool_call.function.name),
            None,
        )),
    }
}

// ─── Data Extraction Helpers ─────────────────────────────────────────────────

fn extract_indicator_summary(
    df: &DataFrame,
    tf: &Timeframe,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let mut lines = vec![format!("=== {tf} Indicator Summary (last 3 candles) ===")];

    let cols = [
        "rssi",
        "rssi_ma",
        "atr_reversion_percent",
        "structure_power",
        "structure_power_sma",
        "sharpe",
        "ema200",
        "atr_percent",
        "leverage",
        "climax_signal",
    ];

    for col_name in &cols {
        if let Ok(series) = df.column(col_name) {
            let values: Vec<String> = (0..series.len())
                .map(|i| format!("{}", series.get(i).unwrap_or(AnyValue::Null)))
                .collect();
            lines.push(format!("  {col_name}: [{}]", values.join(", ")));
        }
    }

    // Also include latest OHLC for context
    for col_name in ["open", "high", "low", "close", "volume"] {
        if let Ok(series) = df.column(col_name) {
            let values: Vec<String> = (0..series.len())
                .map(|i| format!("{}", series.get(i).unwrap_or(AnyValue::Null)))
                .collect();
            lines.push(format!("  {col_name}: [{}]", values.join(", ")));
        }
    }

    Ok(lines.join("\n"))
}

fn extract_price_action(
    df: &DataFrame,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let mut lines = vec!["=== Price Action ===".to_string()];

    for i in 0..df.height() {
        let row: Vec<String> = ["time", "open", "high", "low", "close", "volume"]
            .iter()
            .filter_map(|col_name| {
                df.column(col_name)
                    .ok()
                    .map(|c| format!("{col_name}={}", c.get(i).unwrap_or(AnyValue::Null)))
            })
            .collect();
        lines.push(format!("  Candle {}: {}", i + 1, row.join(", ")));
    }

    Ok(lines.join("\n"))
}

fn build_multi_tf_overview(
    all_dfs: &HashMap<Timeframe, DataFrame>,
    conf: &EnvConf,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let mut lines = vec![format!(
        "=== {} Multi-Timeframe Overview ===",
        conf.symbol
    )];

    // Sort by timeframe weight for ordered output
    let mut tfs: Vec<Timeframe> = all_dfs.keys().cloned().collect();
    tfs.sort_by_key(|tf| tf.weight());

    for tf in &tfs {
        if let Some(df) = all_dfs.get(tf) {
            let last = df.slice(-1, 1);
            let get_val = |name: &str| -> String {
                last.column(name)
                    .ok()
                    .and_then(|c| c.get(0).ok())
                    .map(|v| format!("{v}"))
                    .unwrap_or_else(|| "N/A".to_string())
            };

            lines.push(format!(
                "  {tf}: RSSI={rssi}, ATR_rev={atr_rev}%, signal={signal}, \
                 structure_pwr={pwr}, sharpe={sharpe}, close={close}",
                rssi = get_val("rssi"),
                atr_rev = get_val("atr_reversion_percent"),
                signal = get_val("climax_signal"),
                pwr = get_val("structure_power"),
                sharpe = get_val("sharpe"),
                close = get_val("close"),
            ));
        }
    }

    Ok(lines.join("\n"))
}

use std::collections::HashMap;

use algotrap::prelude::*;
use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionTool, ChatCompletionTools, FunctionObjectArgs,
};
use polars::prelude::*;
use tracing::warn;

use crate::browserless::capture_chart_screenshot;
use crate::chart::render_single_tf_chart_html;
use crate::config::{EnvConf, TickerConf};

use super::AnalysisMode;

/// Build LLM tool definitions by loading schemas from `tools.json`.
///
/// In `AlertScan` mode, `capture_chart` is excluded to avoid wasteful
/// Browserless calls for below-threshold tickers.
pub fn build_tools(
    conf: &EnvConf,
    mode: AnalysisMode,
) -> Result<Vec<ChatCompletionTools>, Box<dyn core::error::Error + Send + Sync>> {
    let path = std::path::Path::new(&conf.prompts_dir).join("tools.json");
    let json_str = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read tool schemas from {}: {e}", path.display()))?;

    let schemas: Vec<serde_json::Value> = serde_json::from_str(&json_str)?;

    let tools = schemas
        .into_iter()
        .filter(|schema| {
            // In alert scan mode, exclude capture_chart (ADR-5)
            if mode == AnalysisMode::AlertScan {
                schema["name"].as_str() != Some("capture_chart")
            } else {
                true
            }
        })
        .map(|schema| {
            let name = schema["name"].as_str().unwrap_or("unknown").to_string();
            let description = schema["description"].as_str().unwrap_or("").to_string();
            let parameters = schema
                .get("parameters")
                .cloned()
                .unwrap_or(serde_json::json!({
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

/// Execute a tool call and return the result as a string.
pub async fn execute_tool_call(
    tool_call: &ChatCompletionMessageToolCall,
    all_dfs: &HashMap<Timeframe, DataFrame>,
    conf: &EnvConf,
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
    scratchpad: &mut HashMap<String, String>,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
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
                    let mut summary = extract_indicator_summary(&last_rows, &tf)?;
                    // Append gap zone context if active
                    if ic.is_active("gap_zones") {
                        if let Some(gap_ctx) = compute_gap_zone_context(df, ic) {
                            summary.push_str("\n");
                            summary.push_str(&gap_ctx);
                        }
                    }
                    Ok(summary)
                }
                None => Ok(format!(
                    "Timeframe {tf_str} not available. Available: {:?}",
                    ticker.tfs
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
                    Ok(price_data)
                }
                None => Ok(format!(
                    "Timeframe {tf_str} not available. Available: {:?}",
                    ticker.tfs
                )),
            }
        }
        "capture_chart" => {
            let default_tf_str = ticker.default_tf.to_string();
            let tf_str = args["timeframe"].as_str().unwrap_or(&default_tf_str);

            // Parse timeframe and render per-TF chart HTML
            let tf: Timeframe = tf_str
                .parse()
                .map_err(|e: String| -> Box<dyn core::error::Error + Send + Sync> { e.into() })?;
            let df = match all_dfs.get(&tf) {
                Some(df) => df,
                None => {
                    return Ok(format!(
                        "Timeframe {tf_str} not available. Available: {:?}",
                        ticker.tfs
                    ));
                }
            };
            let last_rssi = crate::chart::last_rssi_from_df(df);
            let rssi_tint = crate::chart::rssi_tint_class(last_rssi);
            let params = ic.gap_zone_params();
            let zones = algotrap::ta::gap_zones::extract_gap_zones(df, &params);
            let gap_zones_json = crate::chart::gap_zones_to_chart_json(&zones, 0.3);
            let chart_html =
                render_single_tf_chart_html(&tf, df, ticker, &gap_zones_json, rssi_tint)?;

            match capture_chart_screenshot(&chart_html, &conf.browserless_url).await {
                Ok(_png) => Ok(format!(
                    "[Chart screenshot for {tf_str} captured — see attached image]"
                )),
                Err(e) => {
                    warn!("Failed to capture chart screenshot: {e}");
                    Ok(format!(
                        "Failed to capture chart: {e}. Proceeding with data-only analysis."
                    ))
                }
            }
        }
        "get_multi_tf_overview" => {
            let overview = build_multi_tf_overview(all_dfs, ticker, ic)?;
            Ok(overview)
        }
        "read_kb" => {
            let topic = args["topic"].as_str().unwrap_or("");
            let content = crate::kb::read_topic(&conf.memory_dir, topic);
            if content.is_empty() {
                Ok(format!("KB topic '{topic}' is empty."))
            } else {
                Ok(content)
            }
        }
        "write_kb" => {
            let topic = args["topic"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            match crate::kb::write_topic(&conf.memory_dir, topic, content) {
                Ok(msg) => Ok(msg),
                Err(e) => Ok(format!("Failed to write KB: {e}")),
            }
        }
        "write_notes" => {
            let key = args["key"].as_str().unwrap_or("default").to_string();
            let content = args["content"].as_str().unwrap_or("").to_string();
            scratchpad.insert(key, content);
            Ok("Noted.".to_string())
        }
        "read_notes" => {
            let key = args.get("key").and_then(|v| v.as_str());
            match key {
                Some(k) => match scratchpad.get(k) {
                    Some(content) => Ok(content.clone()),
                    None => Ok(format!("No notes found for key '{k}'.")),
                },
                None => {
                    if scratchpad.is_empty() {
                        Ok("No notes saved yet.".to_string())
                    } else {
                        let mut keys: Vec<&String> = scratchpad.keys().collect();
                        keys.sort(); // Deterministic order
                        let entries: Vec<String> = keys
                            .iter()
                            .map(|k| format!("[{}]: {}", k, scratchpad[k.as_str()]))
                            .collect();
                        Ok(entries.join("\n"))
                    }
                }
            }
        }
        _ => Ok(format!("Unknown tool: {}", tool_call.function.name)),
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
        "band_reversion",
        "structure_power",
        "structure_power_sma",
        "sharpe",
        "ema200",
        "atr_percent",
        "leverage",
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
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let mut lines = vec![format!(
        "=== {} Multi-Timeframe Overview ===",
        ticker.symbol
    )];

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
                "  {tf}: RSSI={rssi}, band_rev={band_rev}, \
                 structure_pwr={pwr}, sharpe={sharpe}, close={close}",
                rssi = get_val("rssi"),
                band_rev = get_val("band_reversion"),
                pwr = get_val("structure_power"),
                sharpe = get_val("sharpe"),
                close = get_val("close"),
            ));

            // Append gap zone summary per timeframe if active
            if ic.is_active("gap_zones") {
                if let Some(gap_ctx) = compute_gap_zone_context(df, ic) {
                    lines.push(format!("    {gap_ctx}"));
                }
            }
        }
    }

    Ok(lines.join("\n"))
}

/// Extract OHLC columns from a DataFrame and compute gap zone summary.
fn compute_gap_zone_context(df: &DataFrame, ic: &crate::memory::IndicatorConfig) -> Option<String> {
    use algotrap::ta::gap_zones::{self, body_ratio, is_atr_gap};

    if df.height() == 0 {
        return None;
    }

    let params = ic.gap_zone_params();

    // Use pre-computed columns if available, fallback to on-the-fly
    let working_df = if df.column("is_atr_gap").is_ok() && df.column("body_ratio").is_ok() {
        df.clone()
    } else {
        let ohlc: algotrap::ta::Ohlc = [col("open"), col("high"), col("low"), col("close")];
        df.clone()
            .lazy()
            .with_columns([
                is_atr_gap(&ohlc, params.atr_period).alias("is_atr_gap"),
                body_ratio(&ohlc).alias("body_ratio"),
            ])
            .collect()
            .ok()?
    };

    let zones = gap_zones::extract_gap_zones(&working_df, &params);

    if zones.is_empty() {
        return Some("Gap zones: none detected".to_string());
    }

    let closes = df.column("close").ok()?.f64().ok()?;
    let current_price = closes.get(closes.len() - 1)?;
    let summary = gap_zones::gap_zone_summary(&zones, current_price);

    let nearest_str = match summary.nearest_gap {
        Some((b, t, trust)) => format!(", nearest={b:.0}-{t:.0} (trust {trust:.2})"),
        None => String::new(),
    };

    Some(format!(
        "Gap zones: {} total ({} above, {} below), overlap@price: count={} weighted_trust={:.2}{}",
        zones.len(),
        summary.zones_above,
        summary.zones_below,
        summary.overlap_at_price.count,
        summary.overlap_at_price.weighted_trust,
        nearest_str
    ))
}

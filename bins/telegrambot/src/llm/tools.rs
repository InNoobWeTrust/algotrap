//! Module: telegrambot::llm::tools
//!
//! Provides LLM tool definitions with schemas automatically derived from
//! Rust docstrings using `llm_tool` (https://docs.rs/llm-tool), plus tool
//! execution and data extraction logic.

use std::collections::HashMap;

use algotrap::engine::traits::ComputedFrame;
use algotrap::prelude::*;
use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionTool, ChatCompletionTools, FunctionObjectArgs,
};
use llm_tool::{ToolError, ToolRegistry, llm_tool};
use tracing::warn;

use crate::browserless::capture_chart_screenshot;
use crate::chart::{gap_zones_to_chart_json, render_single_tf_chart_html};
use crate::config::{EnvConf, TickerConf};
use algotrap::query::gap_zones::GapZoneRecord;

use super::AnalysisMode;

/// Format configured timeframes using their canonical display values for LLM-facing text.
fn format_available_timeframes(timeframes: &[Timeframe]) -> String {
    timeframes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

// ─── Tool Declarations (Schemas automatically derived via llm_tool) ─────────

/// Get a summary of technical indicator values for a specific timeframe. Returns the last 3 candles of key indicators: RSSI, ATR %, structure power, Sharpe ratio, EMA200, leverage, gap zones.
#[llm_tool]
fn get_indicator_summary(
    /// The timeframe to get indicators for (e.g., '1m', '5m', '15m', '1h', '4h', '1d', '1w', '1M')
    timeframe: Timeframe,
) -> Result<String, ToolError> {
    Ok(timeframe.to_string())
}

/// Get OHLCV price action data for a specific timeframe. Returns the last N candles with open, high, low, close, volume.
#[llm_tool]
fn get_price_action(
    /// The timeframe (e.g., '1h', '4h', '1d')
    timeframe: Timeframe,
    /// Number of recent candles to return (max 20, default 5)
    num_candles: Option<usize>,
) -> Result<String, ToolError> {
    let _ = num_candles;
    Ok(timeframe.to_string())
}

/// Capture a screenshot of the multi-timeframe chart. Returns a chart image that you can analyze visually. Call this when you need to see the chart patterns, support/resistance levels, or visual confirmation of indicators.
#[llm_tool]
fn capture_chart(
    /// Which timeframe to focus the chart on (e.g., '4h'). Defaults to ticker's default timeframe.
    timeframe: Option<Timeframe>,
) -> Result<String, ToolError> {
    Ok(timeframe.map(|tf| tf.to_string()).unwrap_or_default())
}

/// Get a quick overview across ALL configured timeframes. Returns the latest RSSI, ATR %, structure power, Sharpe, and gap zones for each timeframe. Useful for getting a bird's eye view of the market.
#[llm_tool]
fn get_multi_tf_overview() -> Result<String, ToolError> {
    Ok(String::new())
}

/// Read a knowledge base topic. The KB stores persistent insights across scan cycles. Valid topics: market-regimes, indicator-quirks, ticker-personalities, false-signal-patterns, successful-setups, weight-tuning-log, risk-conditions, cross-ticker-signals, timeframe-biases, lessons-learned.
#[llm_tool]
fn read_kb(
    /// The KB topic slug to read (e.g., 'market-regimes', 'lessons-learned')
    topic: String,
) -> Result<String, ToolError> {
    Ok(topic)
}

/// Write or append content to a knowledge base topic. Use markdown format. Content is appended to existing content. Max 2000 chars per write. Valid topics: market-regimes, indicator-quirks, ticker-personalities, false-signal-patterns, successful-setups, weight-tuning-log, risk-conditions, cross-ticker-signals, timeframe-biases, lessons-learned.
#[llm_tool]
fn write_kb(
    /// The KB topic slug to write to (e.g., 'lessons-learned')
    topic: String,
    /// The markdown content to append to the topic file (max 2000 chars)
    content: String,
) -> Result<String, ToolError> {
    Ok(format!("{topic}: {content}"))
}

/// Save analysis notes to your in-session scratchpad. Use this to record key observations, conflicts, or intermediate conclusions as you analyze. Notes persist across context resets within this session but are discarded at scan end. Overwrites existing content for the same key.
#[llm_tool]
fn write_notes(
    /// A short label for this note (e.g., 'observations', 'conflicts', 'handoff')
    key: String,
    /// The note content to save
    content: String,
) -> Result<String, ToolError> {
    Ok(format!("{key}: {content}"))
}

/// Read your analysis notes from the in-session scratchpad. Call with a specific key to read one note, or omit the key to read all notes.
#[llm_tool]
fn read_notes(
    /// Optional: specific note key to read. Omit to read all notes.
    key: Option<String>,
) -> Result<String, ToolError> {
    Ok(key.unwrap_or_default())
}

/// Build the full `ToolRegistry` containing all available LLM tools.
pub fn create_tool_registry() -> ToolRegistry {
    ToolRegistry::new()
        .with_tool(GetIndicatorSummary)
        .with_tool(GetPriceAction)
        .with_tool(CaptureChart)
        .with_tool(GetMultiTfOverview)
        .with_tool(ReadKb)
        .with_tool(WriteKb)
        .with_tool(WriteNotes)
        .with_tool(ReadNotes)
}

/// Build LLM tool definitions derived automatically from `llm_tool`.
///
/// In `AlertScan` mode, `capture_chart` is excluded to avoid wasteful
/// Browserless calls for below-threshold tickers.
pub fn build_tools(
    _conf: &EnvConf,
    mode: AnalysisMode,
) -> Result<Vec<ChatCompletionTools>, Box<dyn core::error::Error + Send + Sync>> {
    let registry = create_tool_registry();
    let definitions = registry.definitions();

    let tools = definitions
        .into_iter()
        .filter(|def| {
            // In alert scan mode, exclude capture_chart (ADR-5)
            if mode == AnalysisMode::AlertScan {
                def.name != "capture_chart"
            } else {
                true
            }
        })
        .map(|def| {
            ChatCompletionTools::Function(ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name(def.name.to_string())
                    .description(def.description.to_string())
                    .parameters(def.parameter_schema)
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
    all_dfs: &HashMap<Timeframe, Box<dyn ComputedFrame>>,
    gap_zones: &HashMap<Timeframe, Vec<GapZoneRecord>>,
    conf: &EnvConf,
    ticker: &TickerConf,
    _ic: &crate::memory::IndicatorConfig,
    scratchpad: &mut HashMap<String, String>,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    match tool_call.function.name.as_str() {
        "get_indicator_summary" => {
            let params: GetIndicatorSummaryParams =
                serde_json::from_str(&tool_call.function.arguments)?;
            let tf = params.timeframe;
            let tf_str = tf.to_string();
            match all_dfs.get(&tf) {
                Some(df) => {
                    let last_rows = df.slice_last(3)?;
                    let mut summary = extract_indicator_summary(&*last_rows, &tf)?;
                    let zones = gap_zones.get(&tf).map(Vec::as_slice).unwrap_or(&[]);
                    if let Some(gap_ctx) = compute_gap_zone_context(zones) {
                        summary.push('\n');
                        summary.push_str(&gap_ctx);
                    }
                    Ok(summary)
                }
                None => Ok(format!(
                    "Timeframe {tf_str} not available. Available: {}",
                    format_available_timeframes(&ticker.tfs)
                )),
            }
        }
        "get_price_action" => {
            let params: GetPriceActionParams = serde_json::from_str(&tool_call.function.arguments)?;
            let tf = params.timeframe;
            let tf_str = tf.to_string();
            let num = params.num_candles.unwrap_or(5).min(20);
            match all_dfs.get(&tf) {
                Some(df) => {
                    let rows = df.slice_last(num)?;
                    let price_data = extract_price_action(&*rows)?;
                    Ok(price_data)
                }
                None => Ok(format!(
                    "Timeframe {tf_str} not available. Available: {}",
                    format_available_timeframes(&ticker.tfs)
                )),
            }
        }
        "capture_chart" => {
            let params: CaptureChartParams = serde_json::from_str(&tool_call.function.arguments)
                .unwrap_or(CaptureChartParams { timeframe: None });
            let tf = params.timeframe.unwrap_or(ticker.default_tf);
            let tf_str = tf.to_string();

            // Render per-TF chart HTML
            let df = match all_dfs.get(&tf) {
                Some(df) => df,
                None => {
                    return Ok(format!(
                        "Timeframe {tf_str} not available. Available: {}",
                        format_available_timeframes(&ticker.tfs)
                    ));
                }
            };
            let last_rssi = crate::chart::last_rssi_from_df(df.as_ref());
            let rssi_tint = crate::chart::rssi_tint_class(last_rssi);
            let gap_zones_json =
                gap_zones_to_chart_json(gap_zones.get(&tf).map(Vec::as_slice).unwrap_or(&[]));
            let chart_html =
                render_single_tf_chart_html(&tf, df.as_ref(), ticker, &gap_zones_json, rssi_tint)?;

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
            let overview = build_multi_tf_overview(all_dfs, ticker, _ic, gap_zones)?;
            Ok(overview)
        }
        "read_kb" => {
            let params: ReadKbParams = serde_json::from_str(&tool_call.function.arguments)?;
            let content = crate::kb::read_topic(&conf.memory_dir, &params.topic);
            if content.is_empty() {
                Ok(format!("KB topic '{}' is empty.", params.topic))
            } else {
                Ok(content)
            }
        }
        "write_kb" => {
            let params: WriteKbParams = serde_json::from_str(&tool_call.function.arguments)?;
            match crate::kb::write_topic(&conf.memory_dir, &params.topic, &params.content) {
                Ok(msg) => Ok(msg),
                Err(e) => Ok(format!("Failed to write KB: {e}")),
            }
        }
        "write_notes" => {
            let params: WriteNotesParams = serde_json::from_str(&tool_call.function.arguments)?;
            scratchpad.insert(params.key, params.content);
            Ok("Noted.".to_string())
        }
        "read_notes" => {
            let params: ReadNotesParams = serde_json::from_str(&tool_call.function.arguments)
                .unwrap_or(ReadNotesParams { key: None });
            match params.key.as_deref() {
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
    df: &dyn ComputedFrame,
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
        if df.has_column(col_name) {
            let rows = df.len();
            let mut values = Vec::new();
            for i in 0..rows {
                if let Ok(Some(v)) = df.f64_at(col_name, i) {
                    values.push(format!("{}", v));
                } else if let Ok(Some(v)) = df.string_at(col_name, i) {
                    values.push(v);
                } else {
                    values.push("null".to_string());
                }
            }
            lines.push(format!("  {col_name}: [{}]", values.join(", ")));
        }
    }

    // Also include latest OHLC for context
    for col_name in ["open", "high", "low", "close", "volume"] {
        if df.has_column(col_name) {
            let rows = df.len();
            let mut values = Vec::new();
            for i in 0..rows {
                if let Ok(Some(v)) = df.f64_at(col_name, i) {
                    values.push(format!("{}", v));
                } else if let Ok(Some(v)) = df.string_at(col_name, i) {
                    values.push(v);
                } else {
                    values.push("null".to_string());
                }
            }
            lines.push(format!("  {col_name}: [{}]", values.join(", ")));
        }
    }

    Ok(lines.join("\n"))
}

fn extract_price_action(
    df: &dyn ComputedFrame,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let mut lines = vec!["=== Price Action ===".to_string()];

    for i in 0..df.len() {
        let mut row_parts = Vec::new();
        for col_name in ["time", "open", "high", "low", "close", "volume"] {
            if df.has_column(col_name) {
                if let Ok(Some(v)) = df.f64_at(col_name, i) {
                    row_parts.push(format!("{col_name}={}", v));
                } else if let Ok(Some(v)) = df.string_at(col_name, i) {
                    row_parts.push(format!("{col_name}={v}"));
                }
            }
        }
        lines.push(format!("  Candle {}: {}", i + 1, row_parts.join(", ")));
    }

    Ok(lines.join("\n"))
}

fn build_multi_tf_overview(
    all_dfs: &HashMap<Timeframe, Box<dyn ComputedFrame>>,
    ticker: &TickerConf,
    _ic: &crate::memory::IndicatorConfig,
    gap_zones: &HashMap<Timeframe, Vec<GapZoneRecord>>,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let mut lines = vec![format!(
        "=== {} Multi-Timeframe Overview ===",
        ticker.symbol
    )];

    let mut tfs: Vec<Timeframe> = all_dfs.keys().cloned().collect();
    tfs.sort_by_key(|tf| tf.weight());

    // Indicator lines keep the existing ascending-weight order.
    for tf in &tfs {
        if let Some(df) = all_dfs.get(tf) {
            let last = df.slice_last(1)?;
            let get_val = |name: &str| -> String {
                if last.has_column(name) {
                    last.f64_at(name, 0)
                        .ok()
                        .flatten()
                        .map(|v| format!("{v}"))
                        .unwrap_or_else(|| "N/A".to_string())
                } else {
                    "N/A".to_string()
                }
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
        }
    }

    // Zone blocks iterate highest weight first with a combined ~64 zone-line
    // ceiling. Indicator-line order above is unchanged.
    let mut tfs_desc = tfs.clone();
    tfs_desc.sort_by_key(|tf| std::cmp::Reverse(tf.weight()));
    let mut emitted_zone_lines: usize = 0;
    for tf in &tfs_desc {
        if emitted_zone_lines >= 64 {
            break;
        }
        let zones = gap_zones.get(tf).map(Vec::as_slice).unwrap_or(&[]);
        if zones.is_empty() {
            continue;
        }
        // Per-timeframe hard cap (newest 32) then aggregate-budget truncation
        // (newest `remaining`) so the combined digest never exceeds 64 lines.
        let capped: &[GapZoneRecord] = if zones.len() > 32 {
            &zones[zones.len() - 32..]
        } else {
            zones
        };
        let remaining = 64 - emitted_zone_lines;
        let emit: &[GapZoneRecord] = if capped.len() > remaining {
            &capped[capped.len() - remaining..]
        } else {
            capped
        };
        if let Some(ctx) = compute_gap_zone_context(emit) {
            let mut ctx_lines = ctx.lines();
            if let Some(envelope) = ctx_lines.next() {
                lines.push(format!("  {tf} {envelope}"));
                for zone_line in ctx_lines {
                    lines.push(format!("    {zone_line}"));
                }
                emitted_zone_lines += emit.len();
            }
        }
    }

    Ok(lines.join("\n"))
}

fn compute_gap_zone_context(zones: &[GapZoneRecord]) -> Option<String> {
    // Zones arrive pre-budgeted by `recent_gap_zones` SQL LIMIT; enforce the
    // hard 32/timeframe cap here regardless of config, keeping the newest.
    let capped: &[GapZoneRecord] = if zones.len() > 32 {
        &zones[zones.len() - 32..]
    } else {
        zones
    };
    // Zones arrive pre-truncated by SQL LIMIT to the per-timeframe config
    // budget, so the true available count is unknown here; report honestly
    // instead of fabricating M/truncated.
    let mut lines = vec![format!(
        "Gap zones ({} returned, available=unknown, truncated=unknown)",
        capped.len()
    )];
    for z in capped {
        let direction = match z.direction {
            algotrap::query::gap_zones::GapZoneDirection::Bullish => "bullish",
            algotrap::query::gap_zones::GapZoneDirection::Bearish => "bearish",
            algotrap::query::gap_zones::GapZoneDirection::Flat => "flat",
        };
        let mut line = format!(
            "{} | {}/{}/{}/{} | {}-{} | {}",
            z.time_ms, z.open, z.high, z.low, z.close, z.body_bottom, z.body_top, direction
        );
        if let Some(ratio) = z.body_ratio {
            line.push_str(&format!(" | {ratio}"));
        }
        lines.push(line);
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_openai::types::chat::ChatCompletionTools;

    fn sample_ticker() -> TickerConf {
        TickerConf {
            symbol: "BTC-USDT".to_string(),
            sl_percent: 0.02,
            tol_percent: 0.01,
            tfs: vec![Timeframe::H1],
            default_tf: Timeframe::H1,
        }
    }

    fn sample_klines() -> Vec<Kline> {
        (0..240)
            .map(|index| {
                let open = 100.0 + index as f64;
                Kline {
                    open,
                    high: open + 4.0,
                    low: open - 2.0,
                    close: open + if index % 2 == 0 { 2.0 } else { -1.0 },
                    volume: 1_000.0 + index as f64,
                    time: 1_700_000_000_000 + index as i64 * 60_000,
                    adjclose: None,
                }
            })
            .collect()
    }

    fn dummy_env_conf() -> EnvConf {
        let env: HashMap<String, String> = [
            (
                "TICKERS",
                r#"[{"symbol":"BTC-USDT","sl_percent":0.1,"tol_percent":0.618,"tfs":"4h","default_tf":"4h"}]"#,
            ),
            ("TELEGRAM_BOT_TOKEN", "test"),
            ("TELEGRAM_CHAT_ID", "-100"),
            ("LLM_API_BASE", "http://localhost:4000/v1"),
            ("LLM_API_KEY", "sk-test"),
            ("LLM_MODEL", "test-model"),
            ("BROWSERLESS_URL", "http://localhost:3000"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        envy::from_iter(env).unwrap()
    }

    #[test]
    fn test_registry_contains_all_tools() {
        let registry = create_tool_registry();
        let defs = registry.definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_ref()).collect();

        assert_eq!(names.len(), 8);
        assert!(names.contains(&"get_indicator_summary"));
        assert!(names.contains(&"get_price_action"));
        assert!(names.contains(&"capture_chart"));
        assert!(names.contains(&"get_multi_tf_overview"));
        assert!(names.contains(&"read_kb"));
        assert!(names.contains(&"write_kb"));
        assert!(names.contains(&"write_notes"));
        assert!(names.contains(&"read_notes"));
    }

    #[test]
    fn test_build_tools_full_analysis_mode() {
        let conf = dummy_env_conf();
        let tools = build_tools(&conf, AnalysisMode::FullAnalysis).unwrap();
        assert_eq!(tools.len(), 8);
    }

    #[test]
    fn test_build_tools_alert_scan_mode_excludes_capture_chart() {
        let conf = dummy_env_conf();
        let tools = build_tools(&conf, AnalysisMode::AlertScan).unwrap();
        assert_eq!(tools.len(), 7);

        for tool in &tools {
            if let ChatCompletionTools::Function(func) = tool {
                assert_ne!(func.function.name, "capture_chart");
            }
        }
    }

    #[test]
    fn test_format_available_timeframes_uses_canonical_display_values() {
        let timeframes = [Timeframe::M15, Timeframe::H1, Timeframe::H4];
        let rendered = format_available_timeframes(&timeframes);

        assert_eq!(rendered, "15m, 1h, 4h");
        assert!(!rendered.contains("M15"));
        assert!(!rendered.contains("H1"));
        assert!(!rendered.contains("H4"));
    }

    #[test]
    fn test_tool_docstrings_and_schema_properties() {
        let registry = create_tool_registry();
        let defs = registry.definitions();

        // 1. get_indicator_summary
        let ind_def = defs
            .iter()
            .find(|d| d.name == "get_indicator_summary")
            .unwrap();
        assert!(ind_def.description.contains("technical indicator values"));
        assert!(
            ind_def.parameter_schema["properties"]["timeframe"]["description"]
                .as_str()
                .unwrap()
                .contains("timeframe to get indicators for")
        );
        let req = ind_def.parameter_schema["required"].as_array().unwrap();
        assert!(req.contains(&serde_json::json!("timeframe")));

        // 2. get_price_action
        let pa_def = defs.iter().find(|d| d.name == "get_price_action").unwrap();
        assert!(pa_def.description.contains("OHLCV price action"));
        assert!(
            pa_def.parameter_schema["properties"]["num_candles"]["description"]
                .as_str()
                .unwrap()
                .contains("Number of recent candles")
        );

        // 3. get_multi_tf_overview
        let multi_def = defs
            .iter()
            .find(|d| d.name == "get_multi_tf_overview")
            .unwrap();
        assert!(multi_def.description.contains("overview across ALL"));

        // 4. read_kb
        let read_kb_def = defs.iter().find(|d| d.name == "read_kb").unwrap();
        assert!(read_kb_def.description.contains("knowledge base topic"));
        assert!(
            read_kb_def.parameter_schema["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("topic"))
        );

        // 5. write_kb
        let write_kb_def = defs.iter().find(|d| d.name == "write_kb").unwrap();
        let write_req = write_kb_def.parameter_schema["required"]
            .as_array()
            .unwrap();
        assert!(write_req.contains(&serde_json::json!("topic")));
        assert!(write_req.contains(&serde_json::json!("content")));

        // 6. write_notes
        let write_notes_def = defs.iter().find(|d| d.name == "write_notes").unwrap();
        let notes_req = write_notes_def.parameter_schema["required"]
            .as_array()
            .unwrap();
        assert!(notes_req.contains(&serde_json::json!("key")));
        assert!(notes_req.contains(&serde_json::json!("content")));

        // 7. read_notes (key should be optional)
        let read_notes_def = defs.iter().find(|d| d.name == "read_notes").unwrap();
        assert!(read_notes_def.description.contains("in-session scratchpad"));
        if let Some(req_array) = read_notes_def
            .parameter_schema
            .get("required")
            .and_then(|r| r.as_array())
        {
            assert!(!req_array.contains(&serde_json::json!("key")));
        }
    }

    #[test]
    fn test_timeframe_params_bundled_with_canonical_enum_schema() {
        let conf = dummy_env_conf();
        let tools = build_tools(&conf, AnalysisMode::FullAnalysis).unwrap();

        let canonical: Vec<serde_json::Value> = Timeframe::ALL_CANONICAL
            .iter()
            .map(|tf| serde_json::json!(tf))
            .collect();

        for tool in &tools {
            let ChatCompletionTools::Function(func) = tool else {
                continue;
            };
            if !["get_indicator_summary", "get_price_action", "capture_chart"]
                .contains(&func.function.name.as_str())
            {
                continue;
            }

            // `capture_chart` accepts `Option<Timeframe>`, so schemars appends `null`.
            let mut expected = canonical.clone();
            if func.function.name == "capture_chart" {
                expected.push(serde_json::Value::Null);
            }
            let expected = serde_json::Value::Array(expected);

            let tf_schema = &func
                .function
                .parameters
                .as_ref()
                .expect("parameters should be set")["properties"]["timeframe"];
            assert_eq!(
                tf_schema.get("type").and_then(|t| t.as_str()),
                Some("string"),
                "timeframe type mismatch for {}",
                func.function.name
            );
            let enum_values = tf_schema.get("enum").unwrap_or_else(|| {
                panic!(
                    "timeframe should carry the enum bundled from `Timeframe::JsonSchema` for {}",
                    func.function.name
                )
            });
            assert_eq!(
                enum_values, &expected,
                "timeframe enum mismatch for {}",
                func.function.name
            );
        }
    }

    fn gap_record(
        time_ms: i64,
        direction: algotrap::query::gap_zones::GapZoneDirection,
        body_ratio: Option<f64>,
    ) -> GapZoneRecord {
        let (open, close) = match direction {
            algotrap::query::gap_zones::GapZoneDirection::Bearish => (110.0, 100.0),
            algotrap::query::gap_zones::GapZoneDirection::Flat => (100.0, 100.0),
            algotrap::query::gap_zones::GapZoneDirection::Bullish => (100.0, 110.0),
        };
        GapZoneRecord {
            time_ms,
            open,
            high: 115.0,
            low: 95.0,
            close,
            volume: 1_000.0,
            body_bottom: open.min(close),
            body_top: open.max(close),
            direction,
            body_ratio,
        }
    }

    #[tokio::test]
    async fn test_compute_gap_zone_context_uses_parallel_zones_regardless_of_output_toggles() {
        let mut ic = crate::memory::IndicatorConfig::default();
        ic.outputs.is_atr_gap.active = false;
        ic.outputs.body_ratio.active = false;
        let frame = crate::data::process_data(&sample_klines(), &sample_ticker(), &ic)
            .await
            .unwrap();

        assert!(!frame.has_column("is_atr_gap"));
        assert!(!frame.has_column("body_ratio"));
        // Zones no longer depend on visible-output toggles; digest works off
        // the parallel map entry (empty here) and always yields an envelope.
        let gap_zones: HashMap<Timeframe, Vec<GapZoneRecord>> =
            HashMap::from([(Timeframe::H1, vec![])]);
        let zones = gap_zones.get(&Timeframe::H1).map(Vec::as_slice).unwrap_or(&[]);
        let ctx = compute_gap_zone_context(zones).expect("envelope must exist");
        assert!(ctx.contains("Gap zones (0 returned"));
        assert!(ctx.contains("available=unknown"));
    }

    #[tokio::test]
    async fn test_multi_tf_overview_formats_unavailable_indicator_values_as_na() {
        let ticker = sample_ticker();
        let mut ic = crate::memory::IndicatorConfig::default();
        ic.outputs.rssi.active = false;

        let frame = crate::data::process_data(&sample_klines(), &ticker, &ic)
            .await
            .unwrap();
        let all_dfs = HashMap::from([(Timeframe::H1, frame)]);
        let gap_zones: HashMap<Timeframe, Vec<GapZoneRecord>> = HashMap::new();

        let overview = build_multi_tf_overview(&all_dfs, &ticker, &ic, &gap_zones).unwrap();

        assert!(overview.contains("RSSI=N/A"));
        assert!(overview.contains("close="));
    }

    #[test]
    fn test_gap_zone_digest_budget_envelope_ordering_and_no_legacy_wording() {
        use algotrap::query::gap_zones::GapZoneDirection;

        // Per-tf hard cap: 40 zones in -> 32 lines out + envelope.
        let many: Vec<GapZoneRecord> = (0..40)
            .map(|i| {
                gap_record(
                    1_700_000_000_000 + i * 60_000,
                    GapZoneDirection::Bullish,
                    Some(0.8),
                )
            })
            .collect();
        let ctx = compute_gap_zone_context(&many).unwrap();
        let lines: Vec<&str> = ctx.lines().collect();
        assert!(lines[0].contains("Gap zones (32 returned"));
        assert!(lines[0].contains("available=unknown"));
        assert!(lines[0].contains("truncated=unknown"));
        assert_eq!(lines.len(), 1 + 32);
        // Newest 32 retained (ascending tail: indices 8..39).
        assert!(lines[1].contains("1700000480000"));
        assert!(lines[lines.len() - 1].contains("1700002340000"));
        assert!(!ctx.contains("trust"));
        assert!(!ctx.contains("nearest"));
        assert!(!ctx.contains("overlap"));
        assert!(!ctx.contains("above"));
        // body_ratio skipped when None, present otherwise.
        let mixed = vec![
            gap_record(1_000, GapZoneDirection::Bearish, None),
            gap_record(2_000, GapZoneDirection::Flat, Some(0.7)),
        ];
        let mixed_ctx = compute_gap_zone_context(&mixed).unwrap();
        let mixed_lines: Vec<&str> = mixed_ctx.lines().collect();
        assert_eq!(mixed_lines.len(), 3);
        assert!(mixed_lines[1].contains("bearish"));
        assert!(!mixed_lines[1].contains("body_ratio"));
        assert!(mixed_lines[2].contains("flat"));
        assert!(mixed_lines[2].contains("0.7"));
    }

    #[tokio::test]
    async fn test_multi_tf_overview_aggregate_caps_at_64_highest_weight_first() {
        use algotrap::query::gap_zones::GapZoneDirection;

        let ticker = TickerConf {
            symbol: "BTC-USDT".to_string(),
            sl_percent: 0.02,
            tol_percent: 0.01,
            tfs: vec![Timeframe::M15, Timeframe::H1, Timeframe::H4],
            default_tf: Timeframe::H1,
        };
        let ic = crate::memory::IndicatorConfig::default();
        let frame = crate::data::process_data(&sample_klines(), &ticker, &ic)
            .await
            .unwrap();
        let all_dfs: HashMap<Timeframe, Box<dyn ComputedFrame>> = HashMap::from([
            (Timeframe::M15, crate::data::process_data(&sample_klines(), &ticker, &ic).await.unwrap()),
            (Timeframe::H1, crate::data::process_data(&sample_klines(), &ticker, &ic).await.unwrap()),
            (
                Timeframe::H4,
                frame,
            ),
        ]);
        // 30 zones per tf x 3 tfs = 90 raw -> aggregate must cap at 64.
        let mk = |base: i64| {
            (0..30)
                .map(|i| gap_record(base + i * 60_000, GapZoneDirection::Bullish, Some(0.8)))
                .collect::<Vec<_>>()
        };
        let gap_zones: HashMap<Timeframe, Vec<GapZoneRecord>> = HashMap::from([
            (Timeframe::M15, mk(1_700_000_000_000)),
            (Timeframe::H1, mk(1_700_100_000_000)),
            (Timeframe::H4, mk(1_700_200_000_000)),
        ]);

        let overview = build_multi_tf_overview(&all_dfs, &ticker, &ic, &gap_zones).unwrap();
        assert!(overview.contains("Gap zones ("));
        assert!(!overview.to_lowercase().contains("trust"));
        assert!(!overview.contains("nearest"));
        // Count zone detail lines (contain " | " and a direction token).
        let zone_lines: Vec<&str> = overview
            .lines()
            .filter(|l| l.contains(" | ") && (l.contains("bullish") || l.contains("bearish") || l.contains("flat")))
            .collect();
        assert_eq!(zone_lines.len(), 64, "aggregate ceiling must hold");
        // Highest-weight-first: H4 block appears before H1, H1 before M15.
        let pos_h4 = overview.find("4h Gap zones (").expect("H4 block");
        let pos_h1 = overview.find("1h Gap zones (").expect("H1 block");
        let pos_m15 = overview.find("15m Gap zones (").expect("M15 block");
        assert!(pos_h4 < pos_h1 && pos_h1 < pos_m15);
        // Indicator-line order stays ascending (existing contract).
        let ind_h4 = overview.find("4h: RSSI=").unwrap_or(usize::MAX);
        let ind_m15 = overview.find("15m: RSSI=").expect("M15 indicator line");
        assert!(ind_m15 < ind_h4);
    }
}

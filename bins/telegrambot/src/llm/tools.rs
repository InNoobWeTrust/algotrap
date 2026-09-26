//! Module: telegrambot::llm::tools
//!
//! Provides LLM tool definitions with schemas automatically derived from
//! Rust docstrings using `llm_tool` (https://docs.rs/llm-tool), plus tool
//! execution and data extraction logic.

use std::collections::HashMap;
use std::sync::Arc;

use algotrap::engine::traits::ComputedFrame;
use algotrap::prelude::*;
use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionTool, ChatCompletionTools, FunctionObjectArgs,
};
use llm_tool::{ToolContext, ToolError, ToolRegistry, llm_tool};

use crate::config::{EnvConf, TickerConf};
use algotrap::query::gap_zones::GapZoneRecord;

struct ToolRuntime {
    all_dfs: Arc<HashMap<Timeframe, Box<dyn ComputedFrame>>>,
    gap_zones: Arc<HashMap<Timeframe, Vec<GapZoneRecord>>>,
    conf: Arc<EnvConf>,
    ticker: Arc<TickerConf>,
    ic: Arc<crate::memory::IndicatorConfig>,
}

fn ext<T: Send + Sync + 'static>(ctx: &ToolContext) -> Result<Arc<T>, ToolError> {
    ctx.get_ext::<Arc<T>>()
        .ok_or_else(|| ToolError::new("Missing tool runtime"))
}

/// Format configured timeframes using their canonical display values for LLM-facing text.
fn format_available_timeframes(timeframes: &[Timeframe]) -> String {
    timeframes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

// ─── Tool Declarations (Schemas automatically derived via llm_tool) ─────────

/// Fetch ONE column of indicator/price data for a timeframe over a row range.
/// Returns values oldest→newest ("null" for missing cells). Always-present
/// columns: open,high,low,close,volume,time,adj_close,Date (number*7 + text),
/// iching_original_energy,iching_transformed_energy,iching_mutual_energy,
/// iching_open,iching_high,iching_low,iching_close,iching_moving_line,
/// iching_transformed_close,iching_mutual_close,iching_mutual_high,
/// iching_mutual_low,iching_mutual_mean (all number), plus per-config active
/// outputs (rssi, structure_power, band_reversion, atr_percent, sharpe, ...).
/// On unknown column the error lists ALL live columns with dtypes (number/boolean/text).
#[llm_tool]
fn get_indicator_column(
    ctx: &ToolContext,
    /// The timeframe to get data for (e.g., '1h', '4h', '1d')
    timeframe: Timeframe,
    /// The exact column name to fetch
    column: String,
    /// Number of rows to return (default 5, max 50)
    last_rows: Option<usize>,
    /// Number of newest rows to skip (default 0, max 500)
    skip_rows: Option<usize>,
) -> Result<String, ToolError> {
    let runtime = ext::<ToolRuntime>(ctx)?;
    match runtime.all_dfs.get(&timeframe) {
        Some(df) => {
            if !df.has_column(&column) {
                let available = df
                    .column_dtypes()
                    .iter()
                    .map(|(name, dtype)| format!("{name}({})", format!("{dtype:?}").to_lowercase()))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Ok(format!(
                    "Column '{column}' not found. Available columns: {available}"
                ));
            }
            let len = df.len();
            let skip = skip_rows.unwrap_or(0).min(500);
            if skip >= len {
                return Ok(format!("Out of range: frame has {len} rows"));
            }
            let end = len - skip;
            let start = end.saturating_sub(last_rows.unwrap_or(5).min(50));
            if start == end {
                return Ok("No rows in range.".to_string());
            }
            let values = (start..end)
                .map(|i| format_cell(&**df, &column, i))
                .collect::<Vec<_>>();
            Ok(format!("{timeframe} {column}: [{}]", values.join(", ")))
        }
        None => Ok(format!(
            "Timeframe {timeframe} not available. Available: {}",
            format_available_timeframes(&runtime.ticker.tfs)
        )),
    }
}

/// Get OHLCV price action data for a specific timeframe. Returns the last N candles with open, high, low, close, volume.
#[llm_tool]
fn get_price_action(
    ctx: &ToolContext,
    /// The timeframe (e.g., '1h', '4h', '1d')
    timeframe: Timeframe,
    /// Number of recent candles to return (max 20, default 5)
    num_candles: Option<usize>,
) -> Result<String, ToolError> {
    let runtime = ext::<ToolRuntime>(ctx)?;
    let num = num_candles.unwrap_or(5).min(20);
    match runtime.all_dfs.get(&timeframe) {
        Some(df) => {
            let rows = df
                .slice_last(num)
                .map_err(|e| ToolError::new(e.to_string()))?;
            extract_price_action(&*rows).map_err(|e| ToolError::new(e.to_string()))
        }
        None => Ok(format!(
            "Timeframe {timeframe} not available. Available: {}",
            format_available_timeframes(&runtime.ticker.tfs)
        )),
    }
}

/// Get a quick overview across ALL configured timeframes. Returns the last 5 rows of every live column and gap zones for each timeframe.
#[llm_tool]
fn get_multi_tf_overview(ctx: &ToolContext) -> Result<String, ToolError> {
    let runtime = ext::<ToolRuntime>(ctx)?;
    build_multi_tf_overview(
        &runtime.all_dfs,
        &runtime.ticker,
        &runtime.ic,
        &runtime.gap_zones,
    )
    .map_err(|e| ToolError::new(e.to_string()))
}

/// Read a knowledge base topic. The KB stores persistent insights across scan cycles. Valid topics: market-regimes, indicator-quirks, ticker-personalities, false-signal-patterns, successful-setups, weight-tuning-log, risk-conditions, cross-ticker-signals, timeframe-biases, lessons-learned.
#[llm_tool]
fn read_kb(
    ctx: &ToolContext,
    /// The KB topic slug to read (e.g., 'market-regimes', 'lessons-learned')
    topic: String,
) -> Result<String, ToolError> {
    let runtime = ext::<ToolRuntime>(ctx)?;
    let content = crate::kb::read_topic(&runtime.conf.memory_dir, &topic);
    if content.is_empty() {
        Ok(format!("KB topic '{topic}' is empty."))
    } else {
        Ok(content)
    }
}

/// Write or append content to a knowledge base topic. Use markdown format. Content is appended to existing content. Max 2000 chars per write. Valid topics: market-regimes, indicator-quirks, ticker-personalities, false-signal-patterns, successful-setups, weight-tuning-log, risk-conditions, cross-ticker-signals, timeframe-biases, lessons-learned.
#[llm_tool]
fn write_kb(
    ctx: &ToolContext,
    /// The KB topic slug to write to (e.g., 'lessons-learned')
    topic: String,
    /// The markdown content to append to the topic file (max 2000 chars)
    content: String,
) -> Result<String, ToolError> {
    let runtime = ext::<ToolRuntime>(ctx)?;
    match crate::kb::write_topic(&runtime.conf.memory_dir, &topic, &content) {
        Ok(msg) => Ok(msg),
        Err(e) => Ok(format!("Failed to write KB: {e}")),
    }
}

/// Save analysis notes to your in-session scratchpad. Use this to record key observations, conflicts, or intermediate conclusions as you analyze. Notes persist across context resets within this session but are discarded at scan end. Overwrites existing content for the same key.
#[llm_tool]
fn write_notes(
    ctx: &ToolContext,
    /// A short label for this note (e.g., 'observations', 'conflicts', 'handoff')
    key: String,
    /// The note content to save
    content: String,
) -> Result<String, ToolError> {
    let _ = ext::<ToolRuntime>(ctx)?;
    let mut notes = ctx.get_state("notes", serde_json::json!({}));
    let entries = notes
        .as_object_mut()
        .ok_or_else(|| ToolError::new("Invalid notes state"))?;
    entries.insert(key, serde_json::Value::String(content));
    ctx.set_state("notes", notes)?;
    Ok("Noted.".to_string())
}

/// Read your analysis notes from the in-session scratchpad. Call with a specific key to read one note, or omit the key to read all notes.
#[llm_tool]
fn read_notes(
    ctx: &ToolContext,
    /// Optional: specific note key to read. Omit to read all notes.
    key: Option<String>,
) -> Result<String, ToolError> {
    let _ = ext::<ToolRuntime>(ctx)?;
    let notes = ctx.get_state("notes", serde_json::json!({}));
    let entries = notes
        .as_object()
        .ok_or_else(|| ToolError::new("Invalid notes state"))?;
    match key.as_deref() {
        Some(k) => match entries.get(k).and_then(|v| v.as_str()) {
            Some(content) => Ok(content.to_string()),
            None => Ok(format!("No notes found for key '{k}'.")),
        },
        None => {
            let mut keys: Vec<_> = entries.keys().collect();
            if keys.is_empty() {
                Ok("No notes saved yet.".to_string())
            } else {
                keys.sort();
                let lines: Vec<String> = keys
                    .iter()
                    .map(|k| format!("[{k}]: {}", entries[*k].as_str().unwrap_or_default()))
                    .collect();
                Ok(lines.join("\n"))
            }
        }
    }
}

/// Build the full `ToolRegistry` containing all available LLM tools.
pub fn create_tool_registry() -> ToolRegistry {
    ToolRegistry::new()
        .with_tool(GetIndicatorColumn)
        .with_tool(GetPriceAction)
        .with_tool(GetMultiTfOverview)
        .with_tool(ReadKb)
        .with_tool(WriteKb)
        .with_tool(WriteNotes)
        .with_tool(ReadNotes)
}

pub fn create_tool_context(
    all_dfs: &HashMap<Timeframe, Box<dyn ComputedFrame>>,
    gap_zones: &HashMap<Timeframe, Vec<GapZoneRecord>>,
    conf: &EnvConf,
    ticker: &TickerConf,
    ic: &crate::memory::IndicatorConfig,
    scratchpad: &HashMap<String, String>,
) -> Result<ToolContext, Box<dyn core::error::Error + Send + Sync>> {
    let frames = all_dfs
        .iter()
        .map(|(tf, df)| Ok((*tf, df.slice_last(df.len())?)))
        .collect::<Result<HashMap<_, _>, Box<dyn core::error::Error + Send + Sync>>>()?;
    let ctx = ToolContext::new();
    ctx.set_ext(Arc::new(ToolRuntime {
        all_dfs: Arc::new(frames),
        gap_zones: Arc::new(gap_zones.clone()),
        conf: Arc::new(conf.clone()),
        ticker: Arc::new(ticker.clone()),
        ic: Arc::new(ic.clone()),
    }))?;
    ctx.set_state("notes", serde_json::to_value(scratchpad)?)?;
    Ok(ctx)
}

/// Build LLM tool definitions derived automatically from `llm_tool`.
pub fn build_tools(
    _conf: &EnvConf,
) -> Result<Vec<ChatCompletionTools>, Box<dyn core::error::Error + Send + Sync>> {
    let registry = create_tool_registry();
    let definitions = registry.definitions();

    let tools = definitions
        .into_iter()
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
    ic: &crate::memory::IndicatorConfig,
    scratchpad: &mut HashMap<String, String>,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let ctx = create_tool_context(all_dfs, gap_zones, conf, ticker, ic, scratchpad)?;
    let result = dispatch_tool_call(tool_call, &ctx).await?;
    *scratchpad = serde_json::from_value(ctx.get_state("notes", serde_json::json!({})))?;
    Ok(result)
}

pub async fn dispatch_tool_call(
    tool_call: &ChatCompletionMessageToolCall,
    ctx: &ToolContext,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let registry = create_tool_registry();
    if !registry
        .definitions()
        .iter()
        .any(|def| def.name == tool_call.function.name)
    {
        return Ok(format!("Unknown tool: {}", tool_call.function.name));
    }
    Ok(registry
        .dispatch_str(&tool_call.function.name, &tool_call.function.arguments, ctx)
        .await?
        .to_string())
}

// ─── Data Extraction Helpers ─────────────────────────────────────────────────

fn format_cell(df: &dyn ComputedFrame, column: &str, row: usize) -> String {
    if let Ok(Some(value)) = df.f64_at(column, row) {
        value.to_string()
    } else if let Ok(Some(value)) = df.string_at(column, row) {
        value
    } else {
        "null".to_string()
    }
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

    // Column blocks keep ascending-weight order.
    for tf in &tfs {
        if let Some(df) = all_dfs.get(tf) {
            let last = df.slice_last(5)?;
            lines.push(format!("  --- {tf} (last 5 rows) ---"));
            for column in df.columns() {
                let values = (0..last.len())
                    .map(|i| format_cell(&*last, &column, i))
                    .collect::<Vec<_>>();
                lines.push(format!("  {column}: [{}]", values.join(", ")));
            }
        }
    }

    // Zone blocks iterate highest weight first with a combined 16 zone-line
    // ceiling. Column-block order above is unchanged.
    let mut tfs_desc = tfs.clone();
    tfs_desc.sort_by_key(|tf| std::cmp::Reverse(tf.weight()));
    let mut emitted_zone_lines: usize = 0;
    for tf in &tfs_desc {
        if emitted_zone_lines >= 16 {
            break;
        }
        let zones = gap_zones.get(tf).map(Vec::as_slice).unwrap_or(&[]);
        if zones.is_empty() {
            continue;
        }
        // Per-timeframe hard cap (newest 8) then aggregate-budget truncation
        // (newest `remaining`) so the combined digest never exceeds 16 lines.
        let capped: &[GapZoneRecord] = if zones.len() > 8 {
            &zones[zones.len() - 8..]
        } else {
            zones
        };
        let remaining = 16 - emitted_zone_lines;
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
    // hard 8/timeframe cap here regardless of config, keeping the newest.
    let capped: &[GapZoneRecord] = if zones.len() > 8 {
        &zones[zones.len() - 8..]
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

    async fn call_column_tool(
        all_dfs: &HashMap<Timeframe, Box<dyn ComputedFrame>>,
        column: &str,
        last_rows: Option<usize>,
        skip_rows: Option<usize>,
    ) -> String {
        let call: ChatCompletionMessageToolCall = serde_json::from_value(serde_json::json!({
            "id": "test", "type": "function",
            "function": {
                "name": "get_indicator_column",
                "arguments": serde_json::json!({
                    "timeframe": "1h", "column": column,
                    "last_rows": last_rows, "skip_rows": skip_rows
                }).to_string()
            }
        }))
        .unwrap();
        execute_tool_call(
            &call,
            all_dfs,
            &HashMap::new(),
            &dummy_env_conf(),
            &sample_ticker(),
            &crate::memory::IndicatorConfig::default(),
            &mut HashMap::new(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_registry_dispatch_matches_column_and_overview_strings() {
        let ticker = sample_ticker();
        let ic = crate::memory::IndicatorConfig::default();
        let frame = crate::data::process_data(&sample_klines(), &ticker, &ic)
            .await
            .unwrap();
        let expected_column = format!(
            "1h close: [{}]",
            (frame.len() - 2..frame.len())
                .map(|i| format_cell(&*frame, "close", i))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let frames = HashMap::from([(Timeframe::H1, frame)]);
        let zones = HashMap::new();
        let expected_overview = build_multi_tf_overview(&frames, &ticker, &ic, &zones).unwrap();
        let ctx = create_tool_context(
            &frames,
            &zones,
            &dummy_env_conf(),
            &ticker,
            &ic,
            &HashMap::new(),
        )
        .unwrap();
        let registry = create_tool_registry();

        let column = registry
            .dispatch_str(
                "get_indicator_column",
                r#"{"timeframe":"1h","column":"close","last_rows":2}"#,
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(column.to_string(), expected_column);

        let overview = registry
            .dispatch_str("get_multi_tf_overview", "{}", &ctx)
            .await
            .unwrap();
        assert_eq!(overview.to_string(), expected_overview);
    }

    #[tokio::test]
    async fn test_notes_share_state_across_dispatches() {
        let scratchpad = HashMap::from([("seed".to_string(), "existing".to_string())]);
        let ctx = create_tool_context(
            &HashMap::new(),
            &HashMap::new(),
            &dummy_env_conf(),
            &sample_ticker(),
            &crate::memory::IndicatorConfig::default(),
            &scratchpad,
        )
        .unwrap();
        let registry = create_tool_registry();
        assert_eq!(
            registry
                .dispatch_str("read_notes", "{}", &ctx)
                .await
                .unwrap()
                .to_string(),
            "[seed]: existing"
        );
        assert_eq!(
            registry
                .dispatch_str("write_notes", r#"{"key":"next","content":"saved"}"#, &ctx)
                .await
                .unwrap()
                .to_string(),
            "Noted."
        );
        assert_eq!(
            registry
                .dispatch_str("read_notes", "{}", &ctx)
                .await
                .unwrap()
                .to_string(),
            "[next]: saved\n[seed]: existing"
        );
    }

    #[test]
    fn test_registry_contains_all_tools() {
        let registry = create_tool_registry();
        let defs = registry.definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_ref()).collect();

        assert_eq!(names.len(), 7);
        assert!(names.contains(&"get_indicator_column"));
        assert!(names.contains(&"get_price_action"));
        assert!(names.contains(&"get_multi_tf_overview"));
        assert!(names.contains(&"read_kb"));
        assert!(names.contains(&"write_kb"));
        assert!(names.contains(&"write_notes"));
        assert!(names.contains(&"read_notes"));
    }

    #[test]
    fn test_build_tools_returns_seven_tools() {
        let conf = dummy_env_conf();
        let tools = build_tools(&conf).unwrap();
        assert_eq!(tools.len(), 7);
    }

    #[test]
    fn test_build_tools_has_seven_expected_names() {
        let conf = dummy_env_conf();
        let mut names = build_tools(&conf)
            .unwrap()
            .into_iter()
            .filter_map(|tool| match tool {
                ChatCompletionTools::Function(func) => Some(func.function.name),
                _ => None,
            })
            .collect::<Vec<_>>();
        names.sort();
        let expected = vec![
            "get_indicator_column",
            "get_multi_tf_overview",
            "get_price_action",
            "read_kb",
            "read_notes",
            "write_kb",
            "write_notes",
        ];
        assert_eq!(names, expected);
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

        // 1. get_indicator_column
        let ind_def = defs
            .iter()
            .find(|d| d.name == "get_indicator_column")
            .unwrap();
        assert!(ind_def.description.contains("column"));
        assert!(ind_def.description.contains("row range"));
        assert!(
            ind_def.parameter_schema["properties"]["timeframe"]["description"]
                .as_str()
                .unwrap()
                .contains("timeframe to get data for")
        );
        let req = ind_def.parameter_schema["required"].as_array().unwrap();
        assert!(req.contains(&serde_json::json!("timeframe")));
        assert!(req.contains(&serde_json::json!("column")));

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
        let tools = build_tools(&conf).unwrap();

        let canonical: Vec<serde_json::Value> = Timeframe::ALL_CANONICAL
            .iter()
            .map(|tf| serde_json::json!(tf))
            .collect();

        for tool in &tools {
            let ChatCompletionTools::Function(func) = tool else {
                continue;
            };
            if !["get_indicator_column", "get_price_action"].contains(&func.function.name.as_str())
            {
                continue;
            }

            let expected = serde_json::Value::Array(canonical.clone());

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
        let zones = gap_zones
            .get(&Timeframe::H1)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let ctx = compute_gap_zone_context(zones).expect("envelope must exist");
        assert!(ctx.contains("Gap zones (0 returned"));
        assert!(ctx.contains("available=unknown"));
    }

    #[tokio::test]
    async fn test_multi_tf_overview_lists_every_live_column() {
        let ticker = sample_ticker();
        let mut ic = crate::memory::IndicatorConfig::default();
        ic.outputs.rssi.active = false;

        let frame = crate::data::process_data(&sample_klines(), &ticker, &ic)
            .await
            .unwrap();
        let columns = frame.columns();
        let all_dfs = HashMap::from([(Timeframe::H1, frame)]);
        let gap_zones: HashMap<Timeframe, Vec<GapZoneRecord>> = HashMap::new();

        let overview = build_multi_tf_overview(&all_dfs, &ticker, &ic, &gap_zones).unwrap();

        assert!(overview.contains("  --- 1h (last 5 rows) ---"));
        assert!(!overview.contains("  rssi:"));
        for column in columns {
            assert!(
                overview.contains(&format!("  {column}: [")),
                "missing {column}"
            );
        }
        assert!(overview.contains("  iching_mutual_mean: ["));
        let close_values = (all_dfs[&Timeframe::H1].len() - 5..all_dfs[&Timeframe::H1].len())
            .map(|i| format_cell(&*all_dfs[&Timeframe::H1], "close", i))
            .collect::<Vec<_>>();
        assert!(overview.contains(&format!("  close: [{}]", close_values.join(", "))));
    }

    #[tokio::test]
    async fn test_multi_tf_overview_with_fewer_than_five_rows() {
        let ticker = sample_ticker();
        let ic = crate::memory::IndicatorConfig::default();
        let frame = crate::data::process_data(&sample_klines(), &ticker, &ic)
            .await
            .unwrap();
        let last_four = frame.slice_last(4).unwrap();
        let close_values = (0..last_four.len())
            .map(|i| format_cell(&*last_four, "close", i))
            .collect::<Vec<_>>();
        let all_dfs = HashMap::from([(Timeframe::H1, last_four)]);

        let overview = build_multi_tf_overview(&all_dfs, &ticker, &ic, &HashMap::new()).unwrap();
        assert!(overview.contains("  --- 1h (last 5 rows) ---"));
        assert!(overview.contains(&format!("  close: [{}]", close_values.join(", "))));
    }

    #[tokio::test]
    async fn test_indicator_column_includes_iching_energy_values() {
        let ticker = sample_ticker();
        let frame = crate::data::process_data(
            &sample_klines(),
            &ticker,
            &crate::memory::IndicatorConfig::default(),
        )
        .await
        .unwrap();

        let all_dfs = HashMap::from([(Timeframe::H1, frame)]);
        for column in [
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_mutual_energy",
        ] {
            let result = call_column_tool(&all_dfs, column, None, None).await;
            assert!(result.contains(&format!("{column}: [")));
        }
    }

    #[tokio::test]
    async fn test_indicator_column_range_and_errors() {
        let ticker = sample_ticker();
        let frame = crate::data::process_data(
            &sample_klines(),
            &ticker,
            &crate::memory::IndicatorConfig::default(),
        )
        .await
        .unwrap();
        let len = frame.len();
        let expected = (len - 3..len)
            .map(|i| format_cell(&*frame, "close", i))
            .collect::<Vec<_>>();
        let all_dfs = HashMap::from([(Timeframe::H1, frame)]);

        assert_eq!(
            call_column_tool(&all_dfs, "close", Some(3), None).await,
            format!("1h close: [{}]", expected.join(", "))
        );
        assert!(
            call_column_tool(&all_dfs, "bad_column", None, None)
                .await
                .contains("close(number)")
        );
        assert_eq!(
            call_column_tool(&all_dfs, "close", None, Some(len)).await,
            format!("Out of range: frame has {len} rows")
        );
        assert_eq!(
            call_column_tool(&all_dfs, "close", Some(0), None).await,
            "No rows in range."
        );
    }

    #[test]
    fn test_gap_zone_digest_budget_envelope_ordering_and_no_legacy_wording() {
        use algotrap::query::gap_zones::GapZoneDirection;

        // Per-tf hard cap: 40 zones in -> 8 lines out + envelope.
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
        assert!(lines[0].contains("Gap zones (8 returned"));
        assert!(lines[0].contains("available=unknown"));
        assert!(lines[0].contains("truncated=unknown"));
        assert_eq!(lines.len(), 1 + 8);
        // Newest 8 retained (ascending tail: indices 32..39).
        assert!(lines[1].contains("1700001920000"));
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
    async fn test_multi_tf_overview_aggregate_caps_at_16_highest_weight_first() {
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
            (
                Timeframe::M15,
                crate::data::process_data(&sample_klines(), &ticker, &ic)
                    .await
                    .unwrap(),
            ),
            (
                Timeframe::H1,
                crate::data::process_data(&sample_klines(), &ticker, &ic)
                    .await
                    .unwrap(),
            ),
            (Timeframe::H4, frame),
        ]);
        // 30 zones per tf x 3 tfs = 90 raw -> 8 each, aggregate caps at 16.
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
            .filter(|l| {
                l.contains(" | ")
                    && (l.contains("bullish") || l.contains("bearish") || l.contains("flat"))
            })
            .collect();
        assert_eq!(zone_lines.len(), 16, "aggregate ceiling must hold");
        // Highest-weight-first: H4 and H1 use the budget before M15.
        let pos_h4 = overview.find("4h Gap zones (").expect("H4 block");
        let pos_h1 = overview.find("1h Gap zones (").expect("H1 block");
        assert!(pos_h4 < pos_h1);
        assert!(!overview.contains("15m Gap zones ("));
        assert!(overview.contains("4h Gap zones (8 returned"));
        assert!(overview.contains("1h Gap zones (8 returned"));
        // Column-block order stays ascending.
        let ind_h4 = overview.find("--- 4h (last 5 rows) ---").unwrap();
        let ind_m15 = overview.find("--- 15m (last 5 rows) ---").unwrap();
        assert!(ind_m15 < ind_h4);
    }
}

mod tools;

use std::collections::HashMap;

use algotrap::prelude::*;
use async_openai::Client as OpenAIClient;
use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionMessageToolCalls, ChatCompletionRequestAssistantMessageArgs,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use chrono::Utc;
use polars::prelude::*;
use tracing::{debug, info, warn};

use crate::config::{EnvConf, TickerConf};
use crate::memory::TickerMemory;

pub use tools::{build_tools, execute_tool_call};

// ─── Analysis Mode ───────────────────────────────────────────────────────────

/// Whether to run a full analysis (manual mode) or an alert scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisMode {
    /// Full analysis for manual `/analyze` — uses system.txt + user.txt, all tools.
    FullAnalysis,
    /// Alert scan — uses system_alert.txt + user_alert.txt, excludes capture_chart.
    AlertScan,
}

/// Result returned by `run_agent`.
#[derive(Debug)]
pub struct AnalysisResult {
    /// LLM analysis prose (full mode) or summary (alert mode).
    pub text: String,
    /// 0.0–100.0. Meaningful in alert mode; 100.0 in full mode.
    pub confidence: f64,
    /// "LONG" | "SHORT" | "NONE".
    pub direction: String,
    /// Trade plan options (A, B, C) from the LLM.
    pub trade_plans: Vec<crate::memory::TradePlan>,
    /// Per-indicator weights proposed by the LLM (before guardrails).
    pub proposed_weights: Option<HashMap<String, f64>>,
    /// LLM-tuned significance threshold for change detection.
    pub significance_threshold: Option<f64>,
    /// True if trade plan directions align with the declared direction.
    /// NONE direction is always aligned. LONG/SHORT requires ≥2 matching plans.
    pub conviction_aligned: bool,
    /// LLM-proposed indicator parameter tuning (optional, absent = no-op).
    pub proposed_indicator_params: Option<HashMap<String, serde_json::Value>>,
}

// ─── Agent Entry Point ───────────────────────────────────────────────────────

/// Run the multi-turn agentic LLM analysis loop.
///
/// In `FullAnalysis` mode, uses all tools and returns the analysis text.
/// In `AlertScan` mode, excludes `capture_chart` and parses a structured
/// JSON response for confidence + direction.
pub async fn run_agent(
    llm_client: &OpenAIClient<OpenAIConfig>,
    conf: &EnvConf,
    ticker: &TickerConf,
    all_dfs: &HashMap<Timeframe, DataFrame>,
    mode: AnalysisMode,
    memory: Option<&TickerMemory>,
) -> Result<AnalysisResult, Box<dyn core::error::Error + Send + Sync>> {
    let tools = build_tools(conf, mode)?;

    let (system_file, user_file) = match mode {
        AnalysisMode::FullAnalysis => ("system.txt", "user.txt"),
        AnalysisMode::AlertScan => ("system_adaptive.txt", "user_adaptive.txt"),
    };

    let system_prompt = render_prompt(conf, ticker, system_file, memory)?;
    let user_prompt = render_prompt(conf, ticker, user_file, memory)?;

    let mut messages: Vec<ChatCompletionRequestMessage> = vec![
        ChatCompletionRequestSystemMessageArgs::default()
            .content(system_prompt.as_str())
            .build()?
            .into(),
        ChatCompletionRequestUserMessageArgs::default()
            .content(user_prompt.as_str())
            .build()?
            .into(),
    ];

    const MAX_TURNS: usize = 10;

    for turn in 0..MAX_TURNS {
        info!(turn, "LLM agent turn");

        // Compress chat history when it exceeds the configured limit.
        if messages.len() > conf.keep_recent_messages + 1 {
            compress_history(llm_client, conf, &mut messages).await?;
        }

        let mut req_builder = CreateChatCompletionRequestArgs::default();
        req_builder.model(&conf.llm_model).messages(messages.clone());
        if !tools.is_empty() {
            req_builder.tools(tools.clone());
        }
        let request = req_builder.build()?;

        let response = llm_client.chat().create(request).await?;
        let choice = response.choices.first().ok_or("No response from LLM")?;
        let assistant_msg = &choice.message;

        // Check if the LLM wants to call tools
        match &assistant_msg.tool_calls {
            Some(tool_calls) if !tool_calls.is_empty() => {
                let mut assistant_builder =
                    ChatCompletionRequestAssistantMessageArgs::default();
                if let Some(ref content) = assistant_msg.content {
                    assistant_builder.content(content.as_str());
                }
                assistant_builder.tool_calls(tool_calls.clone());
                messages.push(assistant_builder.build()?.into());

                for tool_call_enum in tool_calls {
                    let tool_call = match tool_call_enum {
                        ChatCompletionMessageToolCalls::Function(tc) => tc,
                        _ => continue,
                    };

                    info!(
                        tool = %tool_call.function.name,
                        args = %tool_call.function.arguments,
                        "Executing tool call"
                    );

                    let result = execute_tool_call(tool_call, all_dfs, conf, ticker).await?;

                    debug!(
                        tool = %tool_call.function.name,
                        result_len = result.len(),
                        result = %result,
                        "Tool call result"
                    );

                    messages.push(
                        ChatCompletionRequestToolMessageArgs::default()
                            .tool_call_id(&tool_call.id)
                            .content(result)
                            .build()?
                            .into(),
                    );
                }
            }
            _ => {
                // No tool calls — LLM has finished
                let final_text = assistant_msg
                    .content
                    .as_deref()
                    .unwrap_or("Analysis complete.")
                    .to_string();

                return Ok(parse_analysis_result(final_text, mode));
            }
        }
    }

    warn!("LLM agent reached max turns ({MAX_TURNS}), returning partial analysis");
    Ok(AnalysisResult {
        text: "⚠️ Analysis was truncated after reaching maximum reasoning steps.".to_string(),
        confidence: 0.0,
        direction: "NONE".to_string(),
        trade_plans: vec![],
        proposed_weights: None,
        significance_threshold: None,
        conviction_aligned: true, // NONE is always aligned
        proposed_indicator_params: None,
    })
}

// ─── Result Parsing ──────────────────────────────────────────────────────────

/// Parse the LLM's final text into an `AnalysisResult`.
///
/// In `FullAnalysis` mode, confidence is always 100 and direction is "NONE".
/// In `AlertScan` mode, the text is expected to contain a JSON block with
/// `confidence`, `direction`, and `summary` fields.
fn parse_analysis_result(text: String, mode: AnalysisMode) -> AnalysisResult {
    match mode {
        AnalysisMode::FullAnalysis => AnalysisResult {
            text,
            confidence: 100.0,
            direction: "NONE".to_string(),
            trade_plans: vec![],
            proposed_weights: None,
            significance_threshold: None,
            conviction_aligned: true, // Full analysis has no direction constraint
            proposed_indicator_params: None,
        },
        AnalysisMode::AlertScan => parse_alert_json(&text),
    }
}

/// Try to extract structured JSON from the LLM's alert-mode response.
///
/// Looks for a JSON object anywhere in the text (between `{` and `}`).
/// Falls back to confidence=0 on any parse failure (safe — no false alerts).
fn parse_alert_json(text: &str) -> AnalysisResult {
    // Inline helper: check if trade plans are aligned with declared direction.
    // NONE direction is always aligned. LONG/SHORT requires ≥2 matching plans.
    fn check_conviction(direction: &str, plans: &[crate::memory::TradePlan]) -> bool {
        let dir = direction.to_uppercase();
        if dir == "NONE" {
            return true; // NONE is inherently neutral
        }
        if plans.is_empty() {
            return true; // No plans to check against
        }
        let matching = plans
            .iter()
            .filter(|p| p.direction.to_uppercase() == dir)
            .count();
        matching >= 2 // At least 2 of 3 plans should match
    }
    // Try to find a JSON block in the text
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            let json_str = &text[start..=end];
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                let confidence = v["confidence"]
                    .as_f64()
                    .unwrap_or(0.0)
                    .clamp(0.0, 100.0);
                let direction = v["direction"]
                    .as_str()
                    .unwrap_or("NONE")
                    .to_uppercase();
                let summary = v["summary"]
                    .as_str()
                    .unwrap_or(text)
                    .to_string();

                // Parse trade plans if present
                let trade_plans = v["trade_plans"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|plan| {
                                Some(crate::memory::TradePlan {
                                    label: plan["label"].as_str()?.to_string(),
                                    direction: plan["direction"]
                                        .as_str()
                                        .unwrap_or("WAIT")
                                        .to_string(),
                                    entry: plan["entry"].as_f64(),
                                    target: plan["target"].as_f64(),
                                    stop: plan["stop"].as_f64(),
                                    rationale: plan["rationale"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string(),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                // Parse proposed weights (if present)
                let proposed_weights = v["weights"].as_object().map(|obj| {
                    obj.iter()
                        .filter_map(|(k, val)| val.as_f64().map(|v| (k.clone(), v)))
                        .collect::<HashMap<String, f64>>()
                });
                if proposed_weights.is_none() {
                    warn!("LLM response missing 'weights' — retaining previous cycle weights");
                }

                // Parse significance threshold (if present)
                let significance_threshold = v["significance_threshold"].as_f64();
                if significance_threshold.is_none() {
                    warn!("LLM response missing 'significance_threshold' — retaining previous value");
                }

                // Parse indicator params (if present — absent = no-op)
                let proposed_indicator_params = v["indicator_params"].as_object().map(|obj| {
                    obj.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<HashMap<String, serde_json::Value>>()
                });

                // Conviction check: do trade plans align with declared direction?
                let conviction_aligned = check_conviction(&direction, &trade_plans);
                if !conviction_aligned {
                    let plan_dirs: Vec<&str> = trade_plans
                        .iter()
                        .map(|p| p.direction.as_str())
                        .collect();
                    warn!(
                        direction = %direction,
                        plans = ?plan_dirs,
                        "Low conviction: trade plan directions don't align with declared direction"
                    );
                }

                return AnalysisResult {
                    text: summary,
                    confidence,
                    direction,
                    trade_plans,
                    proposed_weights,
                    significance_threshold,
                    conviction_aligned,
                    proposed_indicator_params,
                };
            }
        }
    }

    // Fallback: unparseable response → confidence 0, no alert
    warn!("Failed to parse alert JSON from LLM response, defaulting to confidence=0");
    AnalysisResult {
        text: text.to_string(),
        confidence: 0.0,
        direction: "NONE".to_string(),
        trade_plans: vec![],
        proposed_weights: None,
        significance_threshold: None,
        conviction_aligned: true, // NONE is always aligned
        proposed_indicator_params: None,
    }
}

// ─── Prompt Loading ──────────────────────────────────────────────────────────

/// Load a prompt template and render all placeholders.
///
/// Base placeholders (all modes): `{{symbol}}`, `{{tfs}}`, `{{default_tf}}`, `{{time}}`
/// Adaptive placeholders (AlertScan): `{{memory_context}}`, `{{weights_context}}`,
/// `{{outcome_summary}}`, `{{weight_min}}`, `{{weight_max}}`, `{{weight_rate_limit}}`
fn render_prompt(
    conf: &EnvConf,
    ticker: &TickerConf,
    filename: &str,
    memory: Option<&TickerMemory>,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let path = std::path::Path::new(&conf.prompts_dir).join(filename);
    let template = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "Failed to read prompt file {}: {e}",
            path.display()
        )
    })?;

    // Base placeholders (shared by all modes)
    let mut rendered = template
        .replace("{{symbol}}", &ticker.symbol)
        .replace("{{tfs}}", &format!("{:?}", ticker.tfs))
        .replace("{{default_tf}}", &ticker.default_tf.to_string())
        .replace(
            "{{time}}",
            &Utc::now().format("%Y-%m-%d %H:%M UTC").to_string(),
        );

    // Adaptive placeholders (only relevant for AlertScan prompt files)
    rendered = rendered
        .replace("{{weight_min}}", &format!("{:.2}", conf.weight_min))
        .replace("{{weight_max}}", &format!("{:.2}", conf.weight_max))
        .replace(
            "{{weight_rate_limit}}",
            &format!("{:.2}", conf.weight_rate_limit),
        )
        .replace(
            "{{tier_watch_threshold}}",
            &format!("{:.0}", conf.tier_watch_threshold),
        )
        .replace(
            "{{tier_alert_threshold}}",
            &format!("{:.0}", conf.tier_alert_threshold),
        )
        .replace(
            "{{tier_watch_threshold_minus_1}}",
            &format!("{:.0}", conf.tier_watch_threshold - 1.0),
        )
        .replace(
            "{{tier_alert_threshold_minus_1}}",
            &format!("{:.0}", conf.tier_alert_threshold - 1.0),
        );

    // Memory-dependent context
    match memory {
        Some(mem) => {
            rendered = rendered
                .replace("{{memory_context}}", &format_memory_context(mem))
                .replace("{{weights_context}}", &format_weights_context(mem))
                .replace("{{outcome_summary}}", &format_outcome_summary(mem))
                .replace("{{kb_rules}}", &format_kb_rules(mem));
        }
        None => {
            rendered = rendered
                .replace(
                    "{{memory_context}}",
                    "No previous predictions. This is a cold start.",
                )
                .replace(
                    "{{weights_context}}",
                    "No previous weights. Use equal attention across all indicators.",
                )
                .replace(
                    "{{outcome_summary}}",
                    "No past predictions to evaluate yet.",
                )
                .replace(
                    "{{kb_rules}}",
                    "No accuracy data yet — KB rules will activate after 3+ scored predictions.",
                );
        }
    }

    Ok(rendered)
}

// ─── Memory Context Formatting ───────────────────────────────────────────────

/// Format memory into a compact text block for prompt injection (≤1600 chars).
///
/// Each prediction is one line: `[timestamp] conf=X dir=DIR outcome=SCORE dir=✓|✗|pending`
fn format_memory_context(mem: &TickerMemory) -> String {
    if mem.predictions.is_empty() {
        return "No previous predictions. This is a cold start.".to_string();
    }

    let mut lines: Vec<String> = Vec::new();
    for pred in &mem.predictions {
        let ts = pred.timestamp.format("%m-%d %H:%M");
        let outcome = match pred.outcome_score {
            Some(score) => {
                let dir_marker = if score >= 0.5 { "✓" } else { "✗" };
                format!("outcome={score:.2} dir={dir_marker}")
            }
            None => "outcome=pending".to_string(),
        };
        lines.push(format!(
            "[{ts}] conf={:.0} dir={} {outcome}",
            pred.confidence, pred.direction
        ));
    }

    let result = lines.join("\n");
    // Budget: ≤1600 characters
    if result.len() > 1600 {
        result[..1600].to_string()
    } else {
        result
    }
}

/// Format current weights as a readable block for prompt injection.
fn format_weights_context(mem: &TickerMemory) -> String {
    if mem.weights.values.is_empty() {
        return "No previous weights. Use equal attention across all indicators.".to_string();
    }

    let mut lines = vec!["Current weights (from previous cycle):".to_string()];
    let mut sorted: Vec<_> = mem.weights.values.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (key, val) in sorted {
        lines.push(format!("  {key}: {val:.2}"));
    }
    lines.push(format!(
        "Significance threshold: {:.2} ({:.0}% change in any key indicator triggers re-notification)",
        mem.weights.significance_threshold,
        mem.weights.significance_threshold * 100.0
    ));
    lines.join("\n")
}

/// Format outcome summary — direction accuracy + composite score.
fn format_outcome_summary(mem: &TickerMemory) -> String {
    let total = mem.predictions.len();
    if total == 0 {
        return "No past predictions to evaluate yet.".to_string();
    }

    let (correct, scored_count, accuracy) =
        crate::scoring::compute_direction_accuracy(&mem.predictions);

    if scored_count == 0 {
        return format!(
            "Your past {total} predictions have not been validated yet."
        );
    }

    let scored: Vec<f64> = mem
        .predictions
        .iter()
        .filter_map(|p| p.outcome_score)
        .collect();
    let avg_composite = scored.iter().sum::<f64>() / scored_count as f64;

    format!(
        "Your past {total} predictions: {scored_count} validated. \
         Direction: {correct}/{scored_count} correct ({accuracy:.0}%). \
         Composite avg: {avg_composite:.2}. \
         High accuracy means your direction calls are reliable. \
         Low accuracy suggests re-evaluating which indicators carry the signal."
    )
}

/// Format conditional KB rules based on direction accuracy stats.
///
/// Rules activate only when ≥3 predictions have been scored, avoiding noise from
/// small sample sizes.
fn format_kb_rules(mem: &TickerMemory) -> String {
    let (correct, total, accuracy) =
        crate::scoring::compute_direction_accuracy(&mem.predictions);

    if total < 3 {
        return "No accuracy data yet — KB rules will activate after 3+ scored predictions.".to_string();
    }

    let mut rules: Vec<String> = Vec::new();

    if accuracy < 0.5 {
        rules.push(format!(
            "⚠️ Your direction accuracy is {correct}/{total} ({accuracy:.0}%), below 50%. \
             You MUST call write_kb('lessons-learned', ...) with your hypothesis about \
             why your direction calls have been wrong."
        ));
    } else if accuracy > 0.7 {
        rules.push(format!(
            "✅ Your direction accuracy is {correct}/{total} ({accuracy:.0}%), above 70%. \
             Call write_kb('successful-setups', ...) to record what indicators/patterns \
             are working."
        ));
    }

    // Check for repeated same-direction errors (3+ of last 5 wrong in same dir)
    let recent_scored: Vec<_> = mem
        .predictions
        .iter()
        .rev()
        .filter(|p| p.outcome_score.is_some())
        .take(5)
        .collect();
    if recent_scored.len() >= 3 {
        let mut dir_fails: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for pred in &recent_scored {
            if pred.outcome_score.unwrap_or(1.0) < 0.5 {
                *dir_fails.entry(&pred.direction).or_insert(0) += 1;
            }
        }
        for (dir, count) in &dir_fails {
            if *count >= 3 {
                rules.push(format!(
                    "🔴 You have {count} recent wrong {dir} calls. \
                     Write to write_kb('false-signal-patterns', ...) your analysis of \
                     why {dir} signals are failing."
                ));
            }
        }
    }

    if rules.is_empty() {
        "Your accuracy is in the normal range. Use KB tools when you notice noteworthy patterns.".to_string()
    } else {
        rules.join("\n")
    }
}


// ─── Chat History Compression ────────────────────────────────────────────────

/// Compress older messages via a separate LLM call for semantic summarization.
///
/// Extracts messages beyond `keep_recent_messages` (excluding the system message),
/// sends them to a fresh LLM with a summarization prompt, and replaces the old
/// messages with the summary.
async fn compress_history(
    llm_client: &OpenAIClient<OpenAIConfig>,
    conf: &EnvConf,
    messages: &mut Vec<ChatCompletionRequestMessage>,
) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    // messages[0] is always the system message — never compress it.
    // Keep the last `keep_recent_messages` non-system messages.
    let non_system_count = messages.len() - 1;
    if non_system_count <= conf.keep_recent_messages {
        return Ok(());
    }

    let to_compress = non_system_count - conf.keep_recent_messages;
    // Old messages are messages[1..=to_compress]
    let old_messages: Vec<String> = messages[1..=to_compress]
        .iter()
        .filter_map(|msg| {
            // Extract text content from each message type
            match msg {
                ChatCompletionRequestMessage::User(m) => {
                    Some(format!("User: {:?}", m.content))
                }
                ChatCompletionRequestMessage::Assistant(m) => {
                    let content = match &m.content {
                        Some(c) => format!("{c:?}"),
                        None => "[tool calls]".to_string(),
                    };
                    Some(format!("Assistant: {content}"))
                }
                ChatCompletionRequestMessage::Tool(m) => {
                    Some(format!("Tool result: {:?}", m.content))
                }
                _ => None,
            }
        })
        .collect();

    if old_messages.is_empty() {
        return Ok(());
    }

    let conversation_text = old_messages.join("\n---\n");

    // Separate LLM call with fresh context for semantic summarization
    let compress_system = "Summarize the following analysis conversation into a concise \
        paragraph. Preserve key indicator readings, timeframe observations, and any \
        patterns noted. Do not add new analysis.";

    let compress_messages: Vec<ChatCompletionRequestMessage> = vec![
        ChatCompletionRequestSystemMessageArgs::default()
            .content(compress_system)
            .build()?
            .into(),
        ChatCompletionRequestUserMessageArgs::default()
            .content(conversation_text.as_str())
            .build()?
            .into(),
    ];

    let request = CreateChatCompletionRequestArgs::default()
        .model(&conf.llm_model)
        .messages(compress_messages)
        .build()?;

    let response = llm_client.chat().create(request).await?;
    let summary = response
        .choices
        .first()
        .and_then(|c| c.message.content.as_deref())
        .unwrap_or("[Earlier analysis data gathered]")
        .to_string();

    info!(
        compressed = to_compress,
        summary_len = summary.len(),
        "Compressed chat history via LLM"
    );

    // Replace old messages with the summary
    let kept_messages: Vec<ChatCompletionRequestMessage> =
        messages[to_compress + 1..].to_vec();
    let summary_msg: ChatCompletionRequestMessage =
        ChatCompletionRequestUserMessageArgs::default()
            .content(format!("[Previous analysis summary]: {summary}"))
            .build()?
            .into();

    // Rebuild: system + summary + kept recent messages
    let system_msg = messages[0].clone();
    messages.clear();
    messages.push(system_msg);
    messages.push(summary_msg);
    messages.extend(kept_messages);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_alert_json_valid() {
        let text = r#"{"confidence": 85.5, "direction": "LONG", "summary": "Strong bullish setup"}"#;
        let result = parse_alert_json(text);
        assert!((result.confidence - 85.5).abs() < f64::EPSILON);
        assert_eq!(result.direction, "LONG");
        assert_eq!(result.text, "Strong bullish setup");
    }

    #[test]
    fn test_parse_alert_json_with_surrounding_text() {
        let text = r#"Here is my analysis:
{"confidence": 42, "direction": "NONE", "summary": "No clear setup at this time."}
That's all."#;
        let result = parse_alert_json(text);
        assert!((result.confidence - 42.0).abs() < f64::EPSILON);
        assert_eq!(result.direction, "NONE");
        assert_eq!(result.text, "No clear setup at this time.");
    }

    #[test]
    fn test_parse_alert_json_missing_fields() {
        let text = r#"{"confidence": 75}"#;
        let result = parse_alert_json(text);
        assert!((result.confidence - 75.0).abs() < f64::EPSILON);
        assert_eq!(result.direction, "NONE"); // default
    }

    #[test]
    fn test_parse_alert_json_invalid_falls_back() {
        let text = "This is not JSON at all";
        let result = parse_alert_json(text);
        assert!((result.confidence - 0.0).abs() < f64::EPSILON);
        assert_eq!(result.direction, "NONE");
        assert_eq!(result.text, text);
    }

    #[test]
    fn test_parse_alert_json_confidence_clamped() {
        let text = r#"{"confidence": 150, "direction": "SHORT", "summary": "Way too confident"}"#;
        let result = parse_alert_json(text);
        assert!((result.confidence - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_alert_json_negative_confidence_clamped() {
        let text = r#"{"confidence": -20, "direction": "LONG", "summary": "Negative"}"#;
        let result = parse_alert_json(text);
        assert!((result.confidence - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_analysis_result_full_mode() {
        let text = "Full analysis here".to_string();
        let result = parse_analysis_result(text.clone(), AnalysisMode::FullAnalysis);
        assert_eq!(result.text, text);
        assert!((result.confidence - 100.0).abs() < f64::EPSILON);
        assert_eq!(result.direction, "NONE");
    }

    #[test]
    fn test_parse_analysis_result_alert_mode() {
        let text = r#"{"confidence": 90, "direction": "SHORT", "summary": "Bearish reversal"}"#.to_string();
        let result = parse_analysis_result(text, AnalysisMode::AlertScan);
        assert!((result.confidence - 90.0).abs() < f64::EPSILON);
        assert_eq!(result.direction, "SHORT");
        assert_eq!(result.text, "Bearish reversal");
    }

    #[test]
    fn test_parse_alert_json_direction_case_insensitive() {
        let text = r#"{"confidence": 80, "direction": "long", "summary": "Go long"}"#;
        let result = parse_alert_json(text);
        assert_eq!(result.direction, "LONG");
    }
}


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
) -> Result<AnalysisResult, Box<dyn core::error::Error + Send + Sync>> {
    let tools = build_tools(conf, mode)?;

    let (system_file, user_file) = match mode {
        AnalysisMode::FullAnalysis => ("system.txt", "user.txt"),
        AnalysisMode::AlertScan => ("system_alert.txt", "user_alert.txt"),
    };

    let system_prompt = load_and_render_prompt(conf, ticker, system_file)?;
    let user_prompt = load_and_render_prompt(conf, ticker, user_file)?;

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
        },
        AnalysisMode::AlertScan => parse_alert_json(&text),
    }
}

/// Try to extract structured JSON from the LLM's alert-mode response.
///
/// Looks for a JSON object anywhere in the text (between `{` and `}`).
/// Falls back to confidence=0 on any parse failure (safe — no false alerts).
fn parse_alert_json(text: &str) -> AnalysisResult {
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

                return AnalysisResult {
                    text: summary,
                    confidence,
                    direction,
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
    }
}

// ─── Prompt Loading ──────────────────────────────────────────────────────────

/// Load a prompt template and render placeholders with ticker-specific values.
fn load_and_render_prompt(
    conf: &EnvConf,
    ticker: &TickerConf,
    filename: &str,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    let path = std::path::Path::new(&conf.prompts_dir).join(filename);
    let template = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "Failed to read prompt file {}: {e}",
            path.display()
        )
    })?;

    let rendered = template
        .replace("{{symbol}}", &ticker.symbol)
        .replace("{{tfs}}", &format!("{:?}", ticker.tfs))
        .replace("{{default_tf}}", &ticker.default_tf.to_string())
        .replace(
            "{{time}}",
            &Utc::now().format("%Y-%m-%d %H:%M UTC").to_string(),
        );

    Ok(rendered)
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


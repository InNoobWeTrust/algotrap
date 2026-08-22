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
use tracing::{debug, info, warn};

use crate::config::{EnvConf, TickerConf};
use crate::memory::{IndicatorConfig, TickerMemory};
use algotrap::engine::traits::ComputedFrame;

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
    /// Direction of the trade prediction.
    pub direction: algotrap::prelude::Direction,
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
    all_dfs: &HashMap<Timeframe, Box<dyn ComputedFrame>>,
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

    // Session-scoped scratchpad for LLM working memory (Scenario 10: empty at start)
    let mut session_scratchpad: HashMap<String, String> = HashMap::new();
    let mut handoff_attempts: u8 = 0;

    const MAX_TURNS: usize = 10;

    for turn in 0..MAX_TURNS {
        info!(turn, "LLM agent turn");

        // Handoff check: when messages exceed threshold, manage context
        let handoff_threshold = conf.keep_recent_messages * 2 + 1; // +1 for system msg
        if messages.len() > handoff_threshold {
            if !session_scratchpad.is_empty() {
                // Scenario 4: Scratchpad has entries → instant context reset
                context_reset(&mut messages, &session_scratchpad)?;
                handoff_attempts = 0;
            } else if handoff_attempts == 0 {
                // Scenario 5: First forced handoff directive
                messages.push(
                    ChatCompletionRequestUserMessageArgs::default()
                        .content("⚠️ Context limit approaching. Call write_notes with key 'handoff' to save your current analysis state before history is cleared.")
                        .build()?
                        .into(),
                );
                handoff_attempts = 1;
                continue; // Give the LLM a turn to respond
            } else if handoff_attempts == 1 {
                // Scenario 6: Second forced handoff directive (stronger)
                messages.push(
                    ChatCompletionRequestUserMessageArgs::default()
                        .content("⚠️ You MUST call write_notes('handoff', '<your observations>') NOW. No other actions.")
                        .build()?
                        .into(),
                );
                handoff_attempts = 2;
                continue; // Give the LLM one more turn
            } else {
                // Scenario 7: LLM ignored both directives → fallback to compress_history
                warn!("LLM refused handoff; falling back to compress_history");
                compress_history(llm_client, conf, &mut messages).await?;
                handoff_attempts = 0;
            }
        }

        let mut req_builder = CreateChatCompletionRequestArgs::default();
        req_builder
            .model(&conf.llm_model)
            .messages(messages.clone());
        if !tools.is_empty() {
            req_builder.tools(tools.clone());
        }
        let request = req_builder.build()?;

        let response = llm_client.chat().create(request).await?;
        let choice = response.choices.first().ok_or("No response from LLM")?;
        let assistant_msg = &choice.message;

        // Debug output for internal reasoning and usage
        if conf.llm_debug {
            println!("\n--- [LLM Turn {}] ---", turn);
            let response_json = serde_json::to_value(&response).unwrap_or(serde_json::Value::Null);

            if let Some(usage) = response_json.get("usage") {
                println!(
                    "📊 [Usage]: {}",
                    serde_json::to_string(usage).unwrap_or_default()
                );
            }

            let msg_json = serde_json::to_value(assistant_msg).unwrap_or(serde_json::Value::Null);
            if let Some(reasoning) = msg_json.get("reasoning_content").and_then(|v| v.as_str()) {
                println!("🧠 [Reasoning]:\n{}", reasoning);
            }
            if let Some(ref content) = assistant_msg.content {
                println!("💬 [Content]:\n{}", content);
            }
            println!("-----------------------\n");
        }

        // Check if the LLM wants to call tools
        match &assistant_msg.tool_calls {
            Some(tool_calls) if !tool_calls.is_empty() => {
                let mut assistant_builder = ChatCompletionRequestAssistantMessageArgs::default();
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

                    let default_ic = IndicatorConfig::default();
                    let ic = memory.map(|m| &m.indicator_config).unwrap_or(&default_ic);
                    let result = execute_tool_call(
                        tool_call,
                        all_dfs,
                        conf,
                        ticker,
                        ic,
                        &mut session_scratchpad,
                    )
                    .await?;

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
        direction: algotrap::prelude::Direction::None,
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
            direction: algotrap::prelude::Direction::None,
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
    fn check_conviction(
        direction: algotrap::prelude::Direction,
        plans: &[crate::memory::TradePlan],
    ) -> bool {
        if direction.is_none() {
            return true; // NONE is inherently neutral
        }
        if plans.is_empty() {
            return true; // No plans to check against
        }
        let matching = plans
            .iter()
            .filter(|p| {
                p.direction
                    .parse::<algotrap::prelude::Direction>()
                    .map(|d| d == direction)
                    .unwrap_or(false)
            })
            .count();
        matching >= 2 // At least 2 of 3 plans should match
    }

    // Inline helper: parse direction string, treating WAIT as NONE.
    fn parse_direction(s: &str) -> algotrap::prelude::Direction {
        s.parse::<algotrap::prelude::Direction>()
            .unwrap_or_default()
    }

    // Try to find a JSON block in the text
    if let Some(start) = text.find('{')
        && let Some(end) = text.rfind('}')
    {
        let json_str = &text[start..=end];
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
            let confidence = v["confidence"].as_f64().unwrap_or(0.0).clamp(0.0, 100.0);
            let direction = parse_direction(v["direction"].as_str().unwrap_or("NONE"));
            let summary = v["summary"].as_str().unwrap_or(text).to_string();

            // Parse trade plans if present
            let trade_plans = v["trade_plans"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|plan| {
                            Some(crate::memory::TradePlan {
                                label: plan["label"].as_str()?.to_string(),
                                direction: plan["direction"].as_str().unwrap_or("WAIT").to_string(),
                                entry: plan["entry"].as_f64(),
                                target: plan["target"].as_f64(),
                                stop: plan["stop"].as_f64(),
                                rationale: plan["rationale"].as_str().unwrap_or("").to_string(),
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
            let conviction_aligned = check_conviction(direction, &trade_plans);
            if !conviction_aligned {
                let plan_dirs: Vec<&str> =
                    trade_plans.iter().map(|p| p.direction.as_str()).collect();
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

    // Fallback: unparseable response → confidence 0, no alert
    warn!("Failed to parse alert JSON from LLM response, defaulting to confidence=0");
    AnalysisResult {
        text: text.to_string(),
        confidence: 0.0,
        direction: algotrap::prelude::Direction::None,
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
    let template = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read prompt file {}: {e}", path.display()))?;

    // Base placeholders (shared by all modes)
    let timeframes = format_timeframes(&ticker.tfs);
    let mut rendered = template
        .replace("{{symbol}}", &ticker.symbol)
        .replace("{{tfs}}", &timeframes)
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
        )
        .replace("{{cot_trigger}}", &cot_trigger_text(conf));

    // Memory-dependent context
    match memory {
        Some(mem) => {
            rendered = rendered
                .replace("{{memory_context}}", &format_memory_context(mem))
                .replace("{{memory_patterns}}", &format_memory_patterns(mem))
                .replace("{{weights_context}}", &format_weights_context(mem))
                .replace("{{outcome_summary}}", &format_outcome_summary(mem))
                .replace(
                    "{{kb_context}}",
                    &format_kb_context(&conf.memory_dir, &ticker.symbol, &mem.predictions),
                )
                .replace("{{kb_rules}}", &format_kb_rules(mem))
                .replace(
                    "{{directional_discipline}}",
                    &format_directional_discipline(mem),
                )
                .replace(
                    "{{indicator_config_context}}",
                    &format_indicator_config_context(
                        mem,
                        crate::scoring::is_low_accuracy_streak(&mem.predictions, 5, 0.4),
                    ),
                );
        }
        None => {
            rendered = rendered
                .replace(
                    "{{memory_context}}",
                    "No previous predictions. This is a cold start.",
                )
                .replace(
                    "{{memory_patterns}}",
                    "No patterns yet — this is a cold start.",
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
                    "{{kb_context}}",
                    "No curated KB insights yet. Use read_kb to explore topics when needed.",
                )
                .replace(
                    "{{kb_rules}}",
                    "No accuracy data yet — KB rules will activate after 3+ scored predictions.",
                )
                .replace(
                    "{{directional_discipline}}",
                    "- SHORT is equally valid as LONG. Evaluate both directions with equal rigor.\n\
                     - NONE/WAIT is correct when signals conflict across 2+ higher timeframes.\n\
                     - Never default to any direction because you're uncertain. Uncertainty = NONE.",
                )
                .replace(
                    "{{indicator_config_context}}",
                    "Default indicator settings. All indicators active with standard periods.",
                );
        }
    }

    Ok(rendered)
}

/// Format configured timeframes using their canonical display values for LLM-facing text.
fn format_timeframes(timeframes: &[Timeframe]) -> String {
    timeframes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
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
    // Budget: ≤1600 chars, char-safe truncation
    if result.len() > 1600 {
        result.chars().take(1600).collect()
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
        return format!("Your past {total} predictions have not been validated yet.");
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
/// small sample sizes. Uses softer language (SHOULD not MUST) and requires
/// reading a topic before writing to prevent duplicate entries.
fn format_kb_rules(mem: &TickerMemory) -> String {
    let (correct, total, accuracy) = crate::scoring::compute_direction_accuracy(&mem.predictions);

    if total < 3 {
        return "No accuracy data yet — KB rules will activate after 3+ scored predictions."
            .to_string();
    }

    let mut rules: Vec<String> = Vec::new();

    // Always include the read-before-write rule
    rules.push(
        "Before writing to any KB topic, you MUST call read_kb on that topic first \
         to avoid duplicating existing insights."
            .to_string(),
    );

    if accuracy < 0.5 {
        rules.push(format!(
            "⚠️ Your direction accuracy is {correct}/{total} ({accuracy:.0}%), below 50%. \
             Consider calling write_kb('lessons-learned', ...) with a NEW hypothesis about \
             why your direction calls have been wrong — but only if your observation \
             differs from what's already recorded."
        ));
    } else if accuracy > 0.7 {
        rules.push(format!(
            "✅ Your direction accuracy is {correct}/{total} ({accuracy:.0}%), above 70%. \
             Consider calling write_kb('successful-setups', ...) to record what indicators/patterns \
             are working — but only if this is a genuinely new insight."
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
        let mut dir_fails: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for pred in &recent_scored {
            if pred.outcome_score.unwrap_or(1.0) < 0.5 {
                let key = pred.direction.to_string();
                *dir_fails.entry(key).or_insert(0) += 1;
            }
        }
        for (dir, count) in &dir_fails {
            if *count >= 3 {
                rules.push(format!(
                    "🔴 You have {count} recent wrong {dir} calls. \
                     Consider writing to write_kb('false-signal-patterns', ...) your analysis of \
                     why {dir} signals are failing — but read the topic first."
                ));
            }
        }
    }

    if rules.len() == 1 {
        // Only the read-before-write rule — add a neutral message
        rules.push("Your accuracy is in the normal range. Use KB tools when you notice noteworthy patterns.".to_string());
    }

    rules.join("\n")
}

/// Format distilled KB insights for the current cycle (≤1500 chars).
///
/// Reads selected KB topics, filters by ticker relevance and accuracy regime,
/// and returns a compact evidence block. This is separate from `format_kb_rules()`
/// which handles behavioral instructions about when to write KB.
fn format_kb_context(
    memory_dir: &str,
    symbol: &str,
    predictions: &[crate::memory::Prediction],
) -> String {
    const BUDGET: usize = 1500;

    let (_, scored_count, accuracy) = crate::scoring::compute_direction_accuracy(predictions);

    // Determine which topics to prioritize based on accuracy regime
    let primary_topics: &[&str] = if scored_count >= 3 && accuracy < 0.5 {
        &["false-signal-patterns", "lessons-learned"]
    } else if scored_count >= 3 && accuracy > 0.7 {
        &["successful-setups", "lessons-learned"]
    } else {
        &[
            "lessons-learned",
            "false-signal-patterns",
            "successful-setups",
        ]
    };

    // Extract ticker base symbol for filtering (e.g. "BTC" from "BTC-USDT")
    let ticker_base = symbol.split('-').next().unwrap_or(symbol);

    let mut sections: Vec<String> = Vec::new();

    for topic in primary_topics {
        let content = crate::kb::read_topic(memory_dir, topic);
        if content.trim().is_empty() {
            continue;
        }

        // Split into lines and score relevance
        let lines: Vec<&str> = content.lines().collect();
        let mut relevant: Vec<&str> = Vec::new();

        for line in &lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Prefer ticker-matching lines
            let ticker_match = trimmed.contains(ticker_base);
            if ticker_match {
                relevant.push(line);
            }
        }

        // If no ticker-specific lines, include a few generic high-signal lines
        if relevant.is_empty() {
            let generic: Vec<&&str> = lines
                .iter()
                .filter(|l| {
                    let t = l.trim();
                    !t.is_empty() && !t.starts_with('#') && !t.starts_with("---")
                })
                .take(5)
                .collect();
            if !generic.is_empty() {
                sections.push(format!(
                    "[{topic}] (generic — verify against current {symbol} conditions)\n{}",
                    generic
                        .iter()
                        .map(|l| l.trim())
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        } else {
            let ticker_lines: Vec<&str> = relevant.iter().take(8).copied().collect();
            sections.push(format!("[{topic}]\n{}", ticker_lines.join("\n")));
        }
    }

    if sections.is_empty() {
        return format!(
            "No curated KB insights yet for {symbol}. Use read_kb to explore topics when needed."
        );
    }

    let result = sections.join("\n\n");

    // Char-safe truncation at budget boundary
    if result.len() > BUDGET {
        let truncated: String = result.chars().take(BUDGET).collect();
        format!("{truncated}\n...[truncated]")
    } else {
        result
    }
}

/// Format a compact derived summary of recurring patterns from TickerMemory.
///
/// Extracts: recent accuracy trend, failing directions, streak status.
/// This complements the raw chronological `memory_context` with actionable abstractions.
fn format_memory_patterns(mem: &TickerMemory) -> String {
    let total = mem.predictions.len();
    if total == 0 {
        return "No patterns yet — this is a cold start.".to_string();
    }

    let (correct, scored_count, accuracy) =
        crate::scoring::compute_direction_accuracy(&mem.predictions);

    let mut parts: Vec<String> = Vec::new();

    if scored_count > 0 {
        parts.push(format!(
            "Validated accuracy: {correct}/{scored_count} ({accuracy:.0}%)."
        ));

        // Check for low accuracy streak
        let low_streak = crate::scoring::is_low_accuracy_streak(&mem.predictions, 5, 0.4);
        if low_streak {
            parts.push("⚠️ Low accuracy streak detected (≤40% over last 5 scored).".to_string());
        }

        // Direction-specific failure analysis
        let recent_scored: Vec<_> = mem
            .predictions
            .iter()
            .rev()
            .filter(|p| p.outcome_score.is_some())
            .take(10)
            .collect();

        if recent_scored.len() >= 3 {
            let mut dir_stats: std::collections::HashMap<String, (usize, usize)> =
                std::collections::HashMap::new();
            for pred in &recent_scored {
                let score = pred.outcome_score.unwrap_or(0.0);
                let entry = dir_stats
                    .entry(pred.direction.to_string())
                    .or_insert((0, 0));
                entry.1 += 1;
                if score >= 0.5 {
                    entry.0 += 1;
                }
            }

            let failing_dirs: Vec<String> = dir_stats
                .iter()
                .filter(|(_, (correct, total))| *total >= 2 && *correct == 0)
                .map(|(dir, _)| dir.clone())
                .collect();

            if !failing_dirs.is_empty() {
                parts.push(format!(
                    "Failing directions (last 10 scored): {}. Avoid relying on signals that produce these calls.",
                    failing_dirs.join(", ")
                ));
            }
        }
    } else {
        parts.push(format!(
            "{total} predictions recorded but none validated yet."
        ));
    }

    parts.join(" ")
}

/// Generate CoT trigger text based on model capability.
///
/// When `supports_reasoning` is true (model has native reasoning like reasoning_effort),
/// returns empty string — CoT would cause double-reasoning waste.
/// When false, returns a 3-line chain-of-thought trigger to improve analysis quality.
fn cot_trigger_text(conf: &EnvConf) -> String {
    if conf.supports_reasoning {
        String::new()
    } else {
        "Before producing your final JSON, think step by step:\n\
         1. What do the indicators across timeframes tell me?\n\
         2. Does this align with or contradict my past predictions?\n\
         3. What is my honest confidence level given all evidence?"
            .to_string()
    }
}

/// Format indicator config into a readable block for prompt injection.
///
/// When `low_accuracy_streak` is true and there are dormant indicators, appends
/// a regime-change nudge (Scenario 13).
fn format_indicator_config_context(mem: &TickerMemory, low_accuracy_streak: bool) -> String {
    let ic = &mem.indicator_config;
    let mut lines = vec!["Current indicator settings:".to_string()];

    // Sort indicator names for stable output
    let mut names: Vec<&String> = ic.indicators.keys().collect();
    names.sort();

    for name in names {
        let params = &ic.indicators[name];
        let status = if params.active { "✅" } else { "❌" };
        let mut parts = vec![format!("  {status} {name}")];

        if let Some(ref spec) = params.period {
            parts.push(format!(
                "period={:.0} [{:.0}-{:.0}]",
                spec.value, spec.min, spec.max
            ));
        }
        if let Some(ref spec) = params.smooth {
            parts.push(format!(
                "smooth={:.0} [{:.0}-{:.0}]",
                spec.value, spec.min, spec.max
            ));
        }
        lines.push(parts.join(" "));
    }

    let dormant = ic.dormant_roster();
    if !dormant.is_empty() {
        lines.push("\nDormant indicators (consider reactivating):".to_string());
        for (name, cycles) in &dormant {
            lines.push(format!("  {name}: inactive for {cycles} cycles"));
        }

        // Scenario 13: regime change nudge when accuracy is critically low
        if low_accuracy_streak {
            lines.push("Your accuracy has dropped. Consider re-enabling dormant indicators to see if they carry signal in the current regime.".to_string());
        }
    }

    lines.join("\n")
}

/// Format dynamic directional discipline based on prediction history.
///
/// Computes direction distribution from recent predictions and generates
/// anti-bias warnings when any direction exceeds 60% of calls.
fn format_directional_discipline(mem: &TickerMemory) -> String {
    let mut lines = vec![
        "- SHORT is equally valid as LONG. In bear trends (price below daily EMA200), prefer SHORT unless strong reversal evidence exists.".to_string(),
        "- NONE/WAIT is correct when signals conflict across 2+ higher timeframes. Do not force a direction.".to_string(),
        "- Never default to any direction because you're uncertain. Uncertainty = NONE.".to_string(),
    ];

    if !mem.predictions.is_empty() {
        let total = mem.predictions.len();
        let mut dir_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for p in &mem.predictions {
            *dir_counts.entry(p.direction.to_string()).or_insert(0) += 1;
        }

        for (dir, count) in &dir_counts {
            let pct = *count as f64 / total as f64 * 100.0;
            if pct > 60.0 {
                lines.push(format!(
                    "- ⚠️ Your recent predictions show a {dir} bias ({pct:.0}%). \
                     Actively evaluate other directions with equal rigor."
                ));
            }
        }
    }

    lines.join("\n")
}

// ─── Chat History Compression ────────────────────────────────────────────────

/// Reset context by dropping all messages except system prompt and injecting
/// scratchpad notes as a user message.
///
/// This is the zero-cost alternative to `compress_history` — no extra LLM call needed.
fn context_reset(
    messages: &mut Vec<ChatCompletionRequestMessage>,
    scratchpad: &HashMap<String, String>,
) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    // Format scratchpad entries in deterministic order (Scenario 9)
    let mut keys: Vec<&String> = scratchpad.keys().collect();
    keys.sort();
    let notes: Vec<String> = keys
        .iter()
        .map(|k| format!("[{}]: {}", k, scratchpad[k.as_str()]))
        .collect();
    let notes_text = notes.join("\n");

    // Keep only system message
    let system_msg = messages[0].clone();
    messages.clear();
    messages.push(system_msg);

    // Inject scratchpad as user message
    messages.push(
        ChatCompletionRequestUserMessageArgs::default()
            .content(format!(
                "[Your analysis notes from earlier turns]:\n{notes_text}"
            ))
            .build()?
            .into(),
    );

    info!(
        keys = ?keys,
        notes_len = notes_text.len(),
        "Context reset: injected scratchpad notes"
    );

    Ok(())
}

// ─── KB Compaction ──────────────────────────────────────────────────────────

/// Compact bloated KB topics via LLM summarization.
///
/// Checks all KB topics for size overruns. For any exceeding the limit,
/// sends the content to the LLM with a summarization prompt and replaces
/// the file with the condensed version.
///
/// Call this once per scan cycle (not per ticker) to keep I/O cheap.
pub async fn compact_kb_if_needed(llm_client: &OpenAIClient<OpenAIConfig>, conf: &EnvConf) {
    let topics = crate::kb::needs_compaction(&conf.memory_dir);
    if topics.is_empty() {
        return;
    }

    let target = crate::kb::compact_target_chars();

    for (topic, content) in topics {
        info!(
            topic = %topic,
            content_len = content.len(),
            target_len = target,
            "Compacting KB topic via LLM"
        );

        // If content exceeds ~100K chars (~25K tokens), truncate to fit within
        // the model's context window. Keep the header + most recent entries.
        const MAX_INPUT_CHARS: usize = 100_000;
        let truncated_content = if content.len() > MAX_INPUT_CHARS {
            // Extract the header line (first line, typically "# Topic Name")
            let header = content.lines().next().unwrap_or("# Unknown Topic");
            let tail_start = content
                .len()
                .saturating_sub(MAX_INPUT_CHARS - header.len() - 50);
            format!(
                "{header}\n\n[... older entries truncated for compaction ...]\n\n{}",
                &content[tail_start..]
            )
        } else {
            content.clone()
        };

        let system_prompt = format!(
            "You are a knowledge base curator. Summarize the following trading insights \
             into a concise, deduplicated document of at most {target} characters. \
             Rules:\n\
             - Keep the markdown header (# Topic Name) as the first line.\n\
             - Merge duplicate observations into single bullet points.\n\
             - Preserve specific ticker names, indicator values, and timestamps where they add value.\n\
             - Remove verbose explanations — keep only actionable insights and patterns.\n\
             - Order by recency (newest insights first).\n\
             - Output ONLY the compacted document, no commentary."
        );

        let messages: Vec<ChatCompletionRequestMessage> = vec![
            ChatCompletionRequestSystemMessageArgs::default()
                .content(system_prompt.as_str())
                .build()
                .unwrap()
                .into(),
            ChatCompletionRequestUserMessageArgs::default()
                .content(truncated_content.as_str())
                .build()
                .unwrap()
                .into(),
        ];

        let request = match CreateChatCompletionRequestArgs::default()
            .model(&conf.llm_model)
            .messages(messages)
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                warn!(topic = %topic, "Failed to build compaction request: {e}");
                continue;
            }
        };

        match llm_client.chat().create(request).await {
            Ok(response) => {
                let summary = response
                    .choices
                    .first()
                    .and_then(|c| c.message.content.as_deref())
                    .unwrap_or("# Compaction failed\n");

                if let Err(e) = crate::kb::force_write_topic(&conf.memory_dir, &topic, summary) {
                    warn!(topic = %topic, "Failed to write compacted KB: {e}");
                } else {
                    info!(
                        topic = %topic,
                        old_len = content.len(),
                        new_len = summary.len(),
                        "KB topic compacted successfully"
                    );
                }
            }
            Err(e) => {
                warn!(topic = %topic, "LLM compaction call failed: {e}");
            }
        }
    }
}

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
                ChatCompletionRequestMessage::User(m) => Some(format!("User: {:?}", m.content)),
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
    let kept_messages: Vec<ChatCompletionRequestMessage> = messages[to_compress + 1..].to_vec();
    let summary_msg: ChatCompletionRequestMessage = ChatCompletionRequestUserMessageArgs::default()
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
        let text =
            r#"{"confidence": 85.5, "direction": "LONG", "summary": "Strong bullish setup"}"#;
        let result = parse_alert_json(text);
        assert!((result.confidence - 85.5).abs() < f64::EPSILON);
        assert_eq!(result.direction, algotrap::prelude::Direction::Long);
        assert_eq!(result.text, "Strong bullish setup");
    }

    #[test]
    fn test_parse_alert_json_with_surrounding_text() {
        let text = r#"Here is my analysis:
{"confidence": 42, "direction": "NONE", "summary": "No clear setup at this time."}
That's all."#;
        let result = parse_alert_json(text);
        assert!((result.confidence - 42.0).abs() < f64::EPSILON);
        assert_eq!(result.direction, algotrap::prelude::Direction::None);
        assert_eq!(result.text, "No clear setup at this time.");
    }

    #[test]
    fn test_parse_alert_json_missing_fields() {
        let text = r#"{"confidence": 75}"#;
        let result = parse_alert_json(text);
        assert!((result.confidence - 75.0).abs() < f64::EPSILON);
        assert_eq!(result.direction, algotrap::prelude::Direction::None); // default
    }

    #[test]
    fn test_parse_alert_json_invalid_falls_back() {
        let text = "This is not JSON at all";
        let result = parse_alert_json(text);
        assert!((result.confidence - 0.0).abs() < f64::EPSILON);
        assert_eq!(result.direction, algotrap::prelude::Direction::None);
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
        assert_eq!(result.direction, algotrap::prelude::Direction::None);
    }

    #[test]
    fn test_parse_analysis_result_alert_mode() {
        let text = r#"{"confidence": 90, "direction": "SHORT", "summary": "Bearish reversal"}"#
            .to_string();
        let result = parse_analysis_result(text, AnalysisMode::AlertScan);
        assert!((result.confidence - 90.0).abs() < f64::EPSILON);
        assert_eq!(result.direction, algotrap::prelude::Direction::Short);
        assert_eq!(result.text, "Bearish reversal");
    }

    #[test]
    fn test_parse_alert_json_direction_case_insensitive() {
        let text = r#"{"confidence": 80, "direction": "long", "summary": "Go long"}"#;
        let result = parse_alert_json(text);
        assert_eq!(result.direction, algotrap::prelude::Direction::Long);
    }

    #[test]
    fn test_format_timeframes_uses_canonical_display_values() {
        let timeframes = [Timeframe::M15, Timeframe::H1, Timeframe::H4];
        let rendered = format_timeframes(&timeframes);

        assert_eq!(rendered, "15m, 1h, 4h");
        assert!(!rendered.contains("M15"));
        assert!(!rendered.contains("H1"));
        assert!(!rendered.contains("H4"));
    }

    #[test]
    fn test_context_reset_injects_notes() {
        // Scenario 4 + 9: context reset with sorted keys
        let mut messages: Vec<ChatCompletionRequestMessage> = vec![
            ChatCompletionRequestSystemMessageArgs::default()
                .content("system prompt")
                .build()
                .unwrap()
                .into(),
            ChatCompletionRequestUserMessageArgs::default()
                .content("old user msg")
                .build()
                .unwrap()
                .into(),
            ChatCompletionRequestUserMessageArgs::default()
                .content("another old msg")
                .build()
                .unwrap()
                .into(),
        ];

        let mut scratchpad = HashMap::new();
        scratchpad.insert("observations".to_string(), "RSSI at 72".to_string());
        scratchpad.insert("conflicts".to_string(), "TF disagreement".to_string());

        context_reset(&mut messages, &scratchpad).unwrap();

        // Should have system + notes injection = 2 messages
        assert_eq!(messages.len(), 2);
        // Notes should be in alphabetical order (conflicts before observations)
        if let ChatCompletionRequestMessage::User(msg) = &messages[1] {
            let content = format!("{:?}", msg.content);
            assert!(content.contains("[conflicts]"));
            assert!(content.contains("[observations]"));
            // Verify alphabetical order
            let conflicts_pos = content.find("[conflicts]").unwrap();
            let observations_pos = content.find("[observations]").unwrap();
            assert!(
                conflicts_pos < observations_pos,
                "Keys should be alphabetically sorted"
            );
        } else {
            panic!("Expected user message with notes");
        }
    }

    #[test]
    fn test_cot_trigger_when_no_reasoning() {
        use crate::config::EnvConf;

        // Build a minimal EnvConf with supports_reasoning = false
        let env: HashMap<String, String> = [
            ("TICKERS", r#"[{"symbol":"BTC-USDT","sl_percent":0.1,"tol_percent":0.618,"tfs":"4h","default_tf":"4h"}]"#),
            ("TELEGRAM_BOT_TOKEN", "test"),
            ("TELEGRAM_CHAT_ID", "-100"),
            ("LLM_API_BASE", "http://localhost:4000/v1"),
            ("LLM_API_KEY", "sk-test"),
            ("LLM_MODEL", "test-model"),
            ("BROWSERLESS_URL", "http://localhost:3000"),
        ].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();

        let conf: EnvConf = envy::from_iter(env).unwrap();
        assert!(!conf.supports_reasoning);
        let trigger = cot_trigger_text(&conf);
        assert!(trigger.contains("step by step"));
    }

    #[test]
    fn test_cot_trigger_suppressed_when_reasoning() {
        use crate::config::EnvConf;

        let env: HashMap<String, String> = [
            ("TICKERS", r#"[{"symbol":"BTC-USDT","sl_percent":0.1,"tol_percent":0.618,"tfs":"4h","default_tf":"4h"}]"#),
            ("TELEGRAM_BOT_TOKEN", "test"),
            ("TELEGRAM_CHAT_ID", "-100"),
            ("LLM_API_BASE", "http://localhost:4000/v1"),
            ("LLM_API_KEY", "sk-test"),
            ("LLM_MODEL", "test-model"),
            ("BROWSERLESS_URL", "http://localhost:3000"),
            ("SUPPORTS_REASONING", "true"),
        ].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();

        let conf: EnvConf = envy::from_iter(env).unwrap();
        assert!(conf.supports_reasoning);
        let trigger = cot_trigger_text(&conf);
        assert!(trigger.is_empty());
    }

    #[test]
    fn test_format_memory_patterns_cold_start() {
        let mem = TickerMemory::new("BTC-USDT");
        let result = format_memory_patterns(&mem);
        assert!(result.contains("cold start"));
    }

    #[test]
    fn test_format_memory_patterns_unscored() {
        use chrono::Utc;
        let mut mem = TickerMemory::new("ETH-USDT");
        mem.predictions.push(crate::memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: algotrap::prelude::Direction::Long,
            summary: "test".into(),
            trade_plans: vec![],
            indicators: HashMap::new(),
            outcome_score: None,
        });
        let result = format_memory_patterns(&mem);
        assert!(result.contains("none validated"));
    }

    #[test]
    fn test_format_memory_patterns_low_accuracy_streak() {
        use chrono::Utc;
        let mut mem = TickerMemory::new("SOL-USDT");
        for _ in 0..5 {
            mem.predictions.push(crate::memory::Prediction {
                timestamp: Utc::now(),
                confidence: 55.0,
                direction: algotrap::prelude::Direction::Long,
                summary: "test".into(),
                trade_plans: vec![],
                indicators: HashMap::new(),
                outcome_score: Some(0.0),
            });
        }
        let result = format_memory_patterns(&mem);
        assert!(result.contains("Low accuracy streak"));
    }

    #[test]
    fn test_format_memory_patterns_failing_direction() {
        use chrono::Utc;
        let mut mem = TickerMemory::new("BTC-USDT");
        // 4 wrong SHORT calls in recent history
        for _ in 0..4 {
            mem.predictions.push(crate::memory::Prediction {
                timestamp: Utc::now(),
                confidence: 70.0,
                direction: algotrap::prelude::Direction::Short,
                summary: "test".into(),
                trade_plans: vec![],
                indicators: HashMap::new(),
                outcome_score: Some(0.0),
            });
        }
        // 2 correct LONG calls
        for _ in 0..2 {
            mem.predictions.push(crate::memory::Prediction {
                timestamp: Utc::now(),
                confidence: 65.0,
                direction: algotrap::prelude::Direction::Long,
                summary: "test".into(),
                trade_plans: vec![],
                indicators: HashMap::new(),
                outcome_score: Some(0.9),
            });
        }
        let result = format_memory_patterns(&mem);
        assert!(result.contains("Failing directions"));
        assert!(result.contains("SHORT"));
    }

    #[test]
    fn test_format_kb_context_empty_kb() {
        let dir = std::env::temp_dir().join("telegrambot_test_kb_context");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap();
        crate::kb::seed_kb(dir_str).unwrap();

        let result = format_kb_context(dir_str, "BTC-USDT", &[]);
        assert!(result.contains("No curated KB insights"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_kb_context_ticker_filtering() {
        let dir = std::env::temp_dir().join("telegrambot_test_kb_filter");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap();
        crate::kb::seed_kb(dir_str).unwrap();

        // Write ticker-specific content on separate lines
        crate::kb::write_topic(
            dir_str,
            "lessons-learned",
            "BTC tends to gap fill on Monday opens.\nETH is more range-bound.\nBTC RSI divergence works well on 4h.",
        )
        .unwrap();

        let result = format_kb_context(dir_str, "BTC-USDT", &[]);
        assert!(result.contains("BTC"));
        // ETH-only line should not appear in BTC context
        assert!(!result.contains("ETH is more range-bound"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_kb_context_truncation() {
        let dir = std::env::temp_dir().join("telegrambot_test_kb_trunc");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap();
        crate::kb::seed_kb(dir_str).unwrap();

        // Write a very long line
        let long_content = "BTC ".repeat(500); // 2000 chars
        crate::kb::write_topic(dir_str, "lessons-learned", &long_content).unwrap();

        let result = format_kb_context(dir_str, "BTC-USDT", &[]);
        assert!(result.len() <= 1520); // budget + "...[truncated]"
        assert!(result.contains("truncated") || result.len() <= 1500);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_kb_context_poor_accuracy_routing() {
        use chrono::Utc;
        let dir = std::env::temp_dir().join("telegrambot_test_kb_routing");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap();
        crate::kb::seed_kb(dir_str).unwrap();

        crate::kb::write_topic(
            dir_str,
            "false-signal-patterns",
            "BTC fake breakouts occur on low volume 4h candles.",
        )
        .unwrap();
        crate::kb::write_topic(
            dir_str,
            "lessons-learned",
            "BTC RSI divergence is unreliable in choppy markets.",
        )
        .unwrap();

        // Simulate poor accuracy predictions
        let predictions: Vec<crate::memory::Prediction> = (0..5)
            .map(|_| crate::memory::Prediction {
                timestamp: Utc::now(),
                confidence: 60.0,
                direction: algotrap::prelude::Direction::Long,
                summary: "test".into(),
                trade_plans: vec![],
                indicators: HashMap::new(),
                outcome_score: Some(0.0),
            })
            .collect();

        let result = format_kb_context(dir_str, "BTC-USDT", &predictions);
        // Should include false-signal-patterns and lessons-learned (poor accuracy routing)
        assert!(result.contains("false-signal-patterns") || result.contains("lessons-learned"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_memory_context_char_safe_truncation() {
        use chrono::Utc;
        let mut mem = TickerMemory::new("BTC-USDT");
        // Add many predictions to exceed budget
        for i in 0..100 {
            mem.predictions.push(crate::memory::Prediction {
                timestamp: Utc::now(),
                confidence: 50.0 + (i as f64 % 50.0),
                direction: algotrap::prelude::Direction::Long,
                summary: format!("prediction {i}"),
                trade_plans: vec![],
                indicators: HashMap::new(),
                outcome_score: Some(0.7),
            });
        }
        let result = format_memory_context(&mem);
        // Truncation is by char count; byte length may exceed 1600 due to Unicode markers
        assert!(result.chars().count() <= 1600);
        assert!(!result.is_empty());
    }
}

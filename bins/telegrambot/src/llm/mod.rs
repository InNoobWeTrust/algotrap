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
use tracing::{info, warn};

use crate::chart;
use crate::config::EnvConf;

pub use tools::{build_tools, execute_tool_call};

/// Run the multi-turn agentic LLM analysis loop.
///
/// The LLM iteratively calls tools (get indicators, capture charts, etc.)
/// until it produces a final analysis response with no further tool calls.
/// Returns the analysis text and an optional chart screenshot.
pub async fn run_agent(
    llm_client: &OpenAIClient<OpenAIConfig>,
    conf: &EnvConf,
    all_dfs: &HashMap<Timeframe, DataFrame>,
) -> Result<(String, Option<Vec<u8>>), Box<dyn core::error::Error + Send + Sync>> {
    let chart_html = chart::render_chart_html(all_dfs, conf)?;
    let tools = build_tools();
    let mut chart_screenshots: HashMap<String, Vec<u8>> = HashMap::new();
    let mut latest_chart_png: Option<Vec<u8>> = None;

    let system_prompt = build_system_prompt(conf);

    let mut messages: Vec<ChatCompletionRequestMessage> = vec![
        ChatCompletionRequestSystemMessageArgs::default()
            .content(system_prompt.as_str())
            .build()?
            .into(),
        ChatCompletionRequestUserMessageArgs::default()
            .content(format!(
                "Please analyze {symbol} now. Current time: {time}. \
                 Use your tools to gather data and provide a comprehensive analysis.",
                symbol = conf.symbol,
                time = Utc::now().format("%Y-%m-%d %H:%M UTC"),
            ))
            .build()?
            .into(),
    ];

    const MAX_TURNS: usize = 10;

    for turn in 0..MAX_TURNS {
        info!(turn, "LLM agent turn");

        let request = CreateChatCompletionRequestArgs::default()
            .model(&conf.llm_model)
            .messages(messages.clone())
            .tools(tools.clone())
            .build()?;

        let response = llm_client.chat().create(request).await?;
        let choice = response.choices.first().ok_or("No response from LLM")?;

        let assistant_msg = &choice.message;

        // Check if the LLM wants to call tools
        match &assistant_msg.tool_calls {
            Some(tool_calls) if !tool_calls.is_empty() => {
                // Add assistant message with tool calls to history
                let mut assistant_builder =
                    ChatCompletionRequestAssistantMessageArgs::default();
                if let Some(ref content) = assistant_msg.content {
                    assistant_builder.content(content.as_str());
                }
                assistant_builder.tool_calls(tool_calls.clone());
                messages.push(assistant_builder.build()?.into());

                // Execute each tool call (pattern-match the enum)
                for tool_call_enum in tool_calls {
                    let tool_call = match tool_call_enum {
                        ChatCompletionMessageToolCalls::Function(tc) => tc,
                        _ => continue, // Skip custom tool calls
                    };

                    info!(
                        tool = %tool_call.function.name,
                        args = %tool_call.function.arguments,
                        "Executing tool call"
                    );

                    let (result, chart_png) = execute_tool_call(
                        tool_call,
                        all_dfs,
                        conf,
                        &chart_html,
                        &mut chart_screenshots,
                    )
                    .await?;

                    if let Some(png) = chart_png {
                        latest_chart_png = Some(png);
                    }

                    // Add tool response to message history
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
                // No tool calls — LLM has finished its analysis
                let final_text = assistant_msg
                    .content
                    .as_deref()
                    .unwrap_or("Analysis complete.")
                    .to_string();
                return Ok((final_text, latest_chart_png));
            }
        }
    }

    warn!("LLM agent reached max turns ({MAX_TURNS}), returning partial analysis");
    Ok((
        "⚠️ Analysis was truncated after reaching maximum reasoning steps.".to_string(),
        latest_chart_png,
    ))
}

// ─── System Prompt ───────────────────────────────────────────────────────────

fn build_system_prompt(conf: &EnvConf) -> String {
    format!(
        "You are an experienced crypto market analyst. You are analyzing {symbol} \
         across multiple timeframes: {tfs:?}.\n\n\
         You have access to tools that let you inspect indicator data and capture chart screenshots. \
         Use them to build a thorough analysis before giving your recommendation.\n\n\
         **Available custom indicators:**\n\
         - **RSSI** (Relative Structure Strength Index, 14-period): Like RSI but based on \
           bar structure bias. >59 = bullish, <41 = bearish, between = neutral.\n\
         - **ATR Reversion %** (42-period ATR, 1.618 multiplier): Measures how far price has \
           moved within the ATR oscillation band. >50 = oversold zone, <-50 = overbought zone.\n\
         - **Structure Power** (9-period RMA of bar bias): Shows the directional strength of \
           price structure. Positive = bullish structure, negative = bearish.\n\
         - **Climax Signal**: Combined signal — overbought when RSSI>54 AND ATR_rev<-50, \
           oversold when RSSI<46 AND ATR_rev>50.\n\
         - **Sharpe Ratio** (200-period): Risk-adjusted return measure.\n\
         - **EMA200**: Long-term trend reference.\n\
         - **Leverage**: Suggested leverage based on ATR and configured stop-loss.\n\n\
         **Approach:**\n\
         1. Start with `get_multi_tf_overview` for a bird's eye view\n\
         2. Drill into specific timeframes with `get_indicator_summary` and `get_price_action`\n\
         3. Optionally `capture_chart` for visual confirmation\n\
         4. Synthesize your findings into a clear recommendation\n\n\
         **Final output format** (when you're done analyzing, respond with this):\n\
         - 📊 **Market Structure**: overall trend assessment\n\
         - 📈 **Momentum**: strength and direction\n\
         - 🎯 **Key Levels**: support/resistance from indicators\n\
         - ⚠️ **Risk Assessment**: volatility, signals, leverage suggestion\n\
         - 💡 **Recommendation**: actionable suggestion with reasoning\n\n\
         Keep your final analysis concise but insightful. Use plain text suitable for Telegram.",
        symbol = conf.symbol,
        tfs = conf.tfs,
    )
}

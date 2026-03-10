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
    let tools = build_tools(conf)?;
    let mut chart_screenshots: HashMap<String, Vec<u8>> = HashMap::new();
    let mut latest_chart_png: Option<Vec<u8>> = None;

    let system_prompt = load_and_render_prompt(conf, "system.txt")?;
    let user_prompt = load_and_render_prompt(conf, "user.txt")?;

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

// ─── Prompt Loading ──────────────────────────────────────────────────────────

/// Load a prompt template from `{prompts_dir}/{filename}` and render placeholders.
fn load_and_render_prompt(
    conf: &EnvConf,
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
        .replace("{{symbol}}", &conf.symbol)
        .replace("{{tfs}}", &format!("{:?}", conf.tfs))
        .replace("{{default_tf}}", &conf.default_tf.to_string())
        .replace(
            "{{time}}",
            &Utc::now().format("%Y-%m-%d %H:%M UTC").to_string(),
        );

    Ok(rendered)
}

use std::sync::Arc;

use async_openai::Client as OpenAIClient;
use async_openai::config::OpenAIConfig;
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;
use tracing::{error, info};

use crate::config::EnvConf;
use crate::{data, llm, telegram};

// ─── Slash Commands ──────────────────────────────────────────────────────────

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
pub enum Command {
    #[command(description = "Show available commands")]
    Help,

    #[command(description = "Run full analysis for a ticker — /analyze BTC-USDT")]
    Analyze(String),

    #[command(description = "List configured tickers")]
    List,
}

// ─── Shared State ────────────────────────────────────────────────────────────

/// Shared state passed to all command handlers.
#[derive(Clone)]
pub struct HandlerState {
    pub conf: Arc<EnvConf>,
    pub bingx: Arc<algotrap::ext::bingx::BingXClient>,
    pub llm_client: Arc<OpenAIClient<OpenAIConfig>>,
}

// ─── Command Dispatcher ─────────────────────────────────────────────────────

/// Start the teloxide command dispatcher (blocking — run in a tokio::spawn).
pub async fn run_command_dispatcher(bot: Bot, state: HandlerState) {
    let handler = Update::filter_message().filter_command::<Command>().endpoint(
        move |bot: Bot, msg: Message, cmd: Command| {
            let state = state.clone();
            async move {
                handle_command(bot, msg, cmd, state).await;
                respond(())
            }
        },
    );

    Dispatcher::builder(bot, handler)
        .default_handler(|_| async {})
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

async fn handle_command(bot: Bot, msg: Message, cmd: Command, state: HandlerState) {
    match cmd {
        Command::Help => {
            let text = Command::descriptions().to_string();
            if let Err(e) = bot.send_message(msg.chat.id, text).await {
                error!("Failed to send help: {e}");
            }
        }
        Command::List => {
            let text = telegram::available_tickers_message(&state.conf);
            if let Err(e) = bot.send_message(msg.chat.id, text).await {
                error!("Failed to send list: {e}");
            }
        }
        Command::Analyze(symbol) => {
            let symbol = symbol.trim().to_uppercase();
            if symbol.is_empty() {
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        "Usage: /analyze <SYMBOL>\nExample: /analyze BTC-USDT",
                    )
                    .await;
                return;
            }

            let ticker = match state.conf.find_ticker(&symbol) {
                Some(tc) => tc.clone(),
                None => {
                    let _ = bot
                        .send_message(
                            msg.chat.id,
                            format!("Unknown ticker: {symbol}. Use /list to see available tickers."),
                        )
                        .await;
                    return;
                }
            };

            info!(symbol = %ticker.symbol, "Manual analysis requested via /analyze");

            // Send a "working…" reply
            let _ = bot
                .send_message(msg.chat.id, format!("🔄 Analyzing {}…", ticker.symbol))
                .await;

            // Run full analysis
            match run_manual_analysis(&state, &ticker).await {
                Ok((analysis, tf_charts)) => {
                    telegram::send_analysis(
                        &bot,
                        msg.chat.id,
                        &ticker.symbol,
                        &analysis.text,
                        &tf_charts,
                    )
                    .await
                    .ok();
                }
                Err(e) => {
                    error!(symbol = %ticker.symbol, "Manual analysis failed: {e:#}");
                    let _ = bot
                        .send_message(msg.chat.id, format!("❌ Analysis failed: {e}"))
                        .await;
                }
            }
        }
    }
}

// ─── Manual Analysis Pipeline ────────────────────────────────────────────────

async fn run_manual_analysis(
    state: &HandlerState,
    ticker: &crate::config::TickerConf,
) -> Result<
    (llm::AnalysisResult, Vec<(String, Vec<u8>)>),
    Box<dyn core::error::Error + Send + Sync>,
> {
    // 1. Fetch market data
    let all_dfs = data::fetch_all_data(&state.bingx, ticker).await?;

    // 2. Capture chart screenshots for all TFs
    let mut tf_charts: Vec<(String, Vec<u8>)> = Vec::new();
    for tf in &ticker.tfs {
        let tf_label = tf.to_string();
        let df = match all_dfs.get(tf) {
            Some(df) => df,
            None => continue,
        };
        let chart_html =
            match crate::chart::render_single_tf_chart_html(tf, df, ticker) {
                Ok(html) => html,
                Err(e) => {
                    error!(tf = %tf_label, "Failed to render chart: {e:#}");
                    continue;
                }
            };
        match crate::browserless::capture_chart_screenshot(
            &chart_html,
            &state.conf.browserless_url,
        )
        .await
        {
            Ok(png) => {
                info!(tf = %tf_label, "Captured chart screenshot");
                tf_charts.push((tf_label, png));
            }
            Err(e) => {
                error!(tf = %tf_label, "Failed to capture chart: {e:#}");
            }
        }
    }

    // 3. Run LLM agent in full analysis mode
    let result = llm::run_agent(
        &state.llm_client,
        &state.conf,
        ticker,
        &all_dfs,
        llm::AnalysisMode::FullAnalysis,
    )
    .await?;

    Ok((result, tf_charts))
}

use std::sync::Arc;

use async_openai::Client as OpenAIClient;
use async_openai::config::OpenAIConfig;
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;
use tracing::{error, info};

use crate::config::EnvConf;
use crate::{data, llm, memory, telegram};

// ─── Slash Commands ──────────────────────────────────────────────────────────

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
pub enum Command {
    #[command(description = "Show available commands")]
    Help,

    #[command(description = "Welcome message + system overview")]
    Start,

    #[command(description = "Run full analysis for a ticker — /analyze BTC-USDT")]
    Analyze(String),

    #[command(description = "List configured tickers")]
    List,

    #[command(description = "Show current scan status + prediction history")]
    Status(String),

    #[command(description = "Show latest prediction digest — /digest BTC-USDT")]
    Digest(String),

    #[command(description = "Show current indicator weights — /weights BTC-USDT")]
    Weights(String),
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
///
/// Handles commands from both group messages and channel posts. Telegram
/// channels emit `ChannelPost` updates (not `Message`), so we must listen
/// to both event types.
pub async fn run_command_dispatcher(bot: Bot, state: HandlerState) {
    let state_for_msg = state.clone();
    let state_for_channel = state.clone();

    let handler = dptree::entry()
        // Branch 1: Regular messages (groups, private chats)
        .branch(
            Update::filter_message().filter_command::<Command>().endpoint(
                move |bot: Bot, msg: Message, cmd: Command| {
                    let state = state_for_msg.clone();
                    async move {
                        handle_command(bot, msg, cmd, state).await;
                        respond(())
                    }
                },
            ),
        )
        // Branch 2: Channel posts
        .branch(
            Update::filter_channel_post()
                .filter_command::<Command>()
                .endpoint(
                    move |bot: Bot, msg: Message, cmd: Command| {
                        let state = state_for_channel.clone();
                        async move {
                            handle_command(bot, msg, cmd, state).await;
                            respond(())
                        }
                    },
                ),
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
        Command::Start => {
            let text = format!(
                "━━━ 🤖 AlgoTrap Bot ━━━\n\
                 Adaptive alert system with self-learning.\n\
                 \n\
                 {} tickers configured\n\
                 Scan interval: {}s\n\
                 Tiers: Alert≥{:.0}%, Watch≥{:.0}%\n\
                 \n\
                 Use /help to see commands.",
                state.conf.tickers.len(),
                state.conf.scan_interval_secs,
                state.conf.tier_alert_threshold,
                state.conf.tier_watch_threshold,
            );
            if let Err(e) = bot.send_message(msg.chat.id, text).await {
                error!("Failed to send start: {e}");
            }
        }
        Command::List => {
            let text = telegram::available_tickers_message(&state.conf);
            if let Err(e) = bot.send_message(msg.chat.id, text).await {
                error!("Failed to send list: {e}");
            }
        }
        Command::Status(symbol) => {
            let symbol = resolve_symbol(&symbol, &state.conf);
            match symbol {
                Some(sym) => {
                    let mem = memory::load_memory(&state.conf.memory_dir, &sym);
                    let text = format_status(&sym, &mem);
                    if let Err(e) = bot.send_message(msg.chat.id, text).await {
                        error!("Failed to send status: {e}");
                    }
                }
                None => {
                    let _ = bot
                        .send_message(msg.chat.id, "Usage: /status <SYMBOL>\nExample: /status BTC-USDT")
                        .await;
                }
            }
        }
        Command::Digest(symbol) => {
            let symbol = resolve_symbol(&symbol, &state.conf);
            match symbol {
                Some(sym) => {
                    let mem = memory::load_memory(&state.conf.memory_dir, &sym);
                    let text = format_digest(&sym, &mem);
                    if let Err(e) = bot.send_message(msg.chat.id, text).await {
                        error!("Failed to send digest: {e}");
                    }
                }
                None => {
                    let _ = bot
                        .send_message(msg.chat.id, "Usage: /digest <SYMBOL>\nExample: /digest BTC-USDT")
                        .await;
                }
            }
        }
        Command::Weights(symbol) => {
            let symbol = resolve_symbol(&symbol, &state.conf);
            match symbol {
                Some(sym) => {
                    let mem = memory::load_memory(&state.conf.memory_dir, &sym);
                    let text = format_weights(&sym, &mem);
                    if let Err(e) = bot.send_message(msg.chat.id, text).await {
                        error!("Failed to send weights: {e}");
                    }
                }
                None => {
                    let _ = bot
                        .send_message(msg.chat.id, "Usage: /weights <SYMBOL>\nExample: /weights BTC-USDT")
                        .await;
                }
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

// ─── Command Helpers ─────────────────────────────────────────────────────────

/// Resolve a symbol argument: if empty and only one ticker configured, use it.
fn resolve_symbol(input: &str, conf: &EnvConf) -> Option<String> {
    let trimmed = input.trim().to_uppercase();
    if trimmed.is_empty() {
        // Auto-resolve if single ticker
        if conf.tickers.len() == 1 {
            Some(conf.tickers[0].symbol.clone())
        } else {
            None
        }
    } else if conf.find_ticker(&trimmed).is_some() {
        Some(trimmed)
    } else {
        None
    }
}

fn format_status(symbol: &str, mem: &memory::TickerMemory) -> String {
    let pred_count = mem.predictions.len();
    let scored = mem
        .predictions
        .iter()
        .filter(|p| p.outcome_score.is_some())
        .count();

    let last_scan = mem
        .predictions
        .last()
        .map(|p| p.timestamp.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "never".to_string());

    let last_tier = mem
        .last_notified
        .tier
        .as_deref()
        .unwrap_or("none");

    format!(
        "━━━ 📊 {symbol} Status ━━━\n\
         Last scan: {last_scan}\n\
         Predictions: {pred_count} ({scored} validated)\n\
         Weights: {} indicators\n\
         Last tier: {last_tier}",
        mem.weights.values.len(),
    )
}

fn format_digest(symbol: &str, mem: &memory::TickerMemory) -> String {
    if let Some(latest) = mem.predictions.last() {
        let age = chrono::Utc::now() - latest.timestamp;
        let age_str = if age.num_hours() > 0 {
            format!("{}h ago", age.num_hours())
        } else {
            format!("{}m ago", age.num_minutes())
        };

        let price_str = latest
            .indicators
            .get("close")
            .map(|p| format!("{p:.2}"))
            .unwrap_or_else(|| "N/A".to_string());

        let mut text = format!(
            "━━━ 📝 {symbol} Digest ━━━\n\
             Confidence: {:.0}% | {} | {age_str}\n\
             Last-scan price: {price_str}\n\
             \n\
             {}",
            latest.confidence, latest.direction, latest.summary,
        );

        if !latest.trade_plans.is_empty() {
            text.push_str("\n\n📋 Plans:");
            for plan in &latest.trade_plans {
                text.push_str(&format!("\n  {} {} ", plan.label, plan.direction));
                if let Some(e) = plan.entry {
                    text.push_str(&format!("entry={e:.2} "));
                }
                if let Some(t) = plan.target {
                    text.push_str(&format!("target={t:.2} "));
                }
                if let Some(s) = plan.stop {
                    text.push_str(&format!("stop={s:.2}"));
                }
            }
        }

        text
    } else {
        format!("📝 {symbol}: No predictions yet. Wait for the next scan cycle.")
    }
}

fn format_weights(symbol: &str, mem: &memory::TickerMemory) -> String {
    if mem.weights.values.is_empty() {
        return format!("⚖️ {symbol}: No weights yet (cold start). Defaults to equal weights.");
    }

    let mut lines = vec![format!("━━━ ⚖️ {symbol} Weights ━━━")];
    let mut weights: Vec<_> = mem.weights.values.iter().collect();
    weights.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (key, val) in &weights {
        let bar_len = (**val * 20.0).round() as usize;
        let bar = "█".repeat(bar_len);
        lines.push(format!("  {key}: {val:.3} {bar}"));
    }

    lines.push(format!(
        "\n🎯 Significance threshold: {:.0}%",
        mem.weights.significance_threshold * 100.0
    ));

    lines.join("\n")
}

// ─── Manual Analysis Pipeline ────────────────────────────────────────────────

async fn run_manual_analysis(
    state: &HandlerState,
    ticker: &crate::config::TickerConf,
) -> Result<
    (llm::AnalysisResult, Vec<(String, Vec<u8>)>),
    Box<dyn core::error::Error + Send + Sync>,
> {
    // 1. Fetch market data (manual mode uses default indicator config)
    let ic = crate::memory::IndicatorConfig::default();
    let all_dfs = data::fetch_all_data(&state.bingx, ticker, &ic).await?;

    // 2. Capture chart screenshots for all TFs
    let mut tf_charts: Vec<(String, Vec<u8>)> = Vec::new();
    for tf in &ticker.tfs {
        let tf_label = tf.to_string();
        let df = match all_dfs.get(tf) {
            Some(df) => df,
            None => continue,
        };
        let last_rssi = crate::chart::last_rssi_from_df(df);
        let rssi_tint = crate::chart::rssi_tint_class(last_rssi);
        let params = ic.gap_zone_params();
        let zones = algotrap::ta::gap_zones::extract_gap_zones(df, &params);
        let gap_zones_json = crate::chart::gap_zones_to_chart_json(&zones, 0.3);
        let chart_html =
            match crate::chart::render_single_tf_chart_html(tf, df, ticker, &gap_zones_json, rssi_tint) {
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
        None,
    )
    .await?;

    Ok((result, tf_charts))
}

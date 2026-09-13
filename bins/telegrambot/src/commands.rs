use std::sync::Arc;

use async_openai::Client as OpenAIClient;
use async_openai::config::OpenAIConfig;
use teloxide::RequestError;
use teloxide::errors::ApiError;
use teloxide::prelude::*;
use teloxide::update_listeners;
use teloxide::utils::command::BotCommands;
use tracing::{error, info, warn};

use crate::config::EnvConf;
use crate::{data, llm, memory, telegram};

// ─── Slash Commands ──────────────────────────────────────────────────────────

/// Commands supported by the Telegram bot dispatcher.
#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
pub enum Command {
    /// Display the available command descriptions.
    #[command(description = "Show available commands")]
    Help,

    /// Display the welcome message and system overview.
    #[command(description = "Welcome message + system overview")]
    Start,

    /// Run a full analysis for a ticker.
    #[command(description = "Run full analysis for a ticker — /analyze BTC-USDT")]
    Analyze(String),

    /// List configured tickers.
    #[command(description = "List configured tickers")]
    List,

    /// Display scan status and prediction history for a ticker.
    #[command(description = "Show current scan status + prediction history")]
    Status(String),

    /// Display the latest prediction digest for a ticker.
    #[command(description = "Show latest prediction digest — /digest BTC-USDT")]
    Digest(String),

    /// Display current indicator weights for a ticker.
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
            Update::filter_message()
                .filter_command::<Command>()
                .endpoint(move |bot: Bot, msg: Message, cmd: Command| {
                    let state = state_for_msg.clone();
                    async move {
                        handle_command(bot, msg, cmd, state).await;
                        respond(())
                    }
                }),
        )
        // Branch 2: Channel posts
        .branch(
            Update::filter_channel_post()
                .filter_command::<Command>()
                .endpoint(move |bot: Bot, msg: Message, cmd: Command| {
                    let state = state_for_channel.clone();
                    async move {
                        handle_command(bot, msg, cmd, state).await;
                        respond(())
                    }
                }),
        );

    let mut dispatcher = Dispatcher::builder(bot.clone(), handler)
        .default_handler(|_| async {})
        .enable_ctrlc_handler()
        .build();

    let listener = update_listeners::polling_default(bot).await;
    let listener_error_handler = Arc::new(|err| async move {
        if matches!(
            err,
            RequestError::Api(ApiError::TerminatedByOtherGetUpdates)
        ) {
            warn!(
                "Telegram long polling was terminated by another getUpdates request; teloxide will retry"
            );
        } else {
            error!("An error from the update listener: {err:?}");
        }
    });

    dispatcher
        .dispatch_with_listener(listener, listener_error_handler)
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
                        .send_message(
                            msg.chat.id,
                            "Usage: /status <SYMBOL>\nExample: /status BTC-USDT",
                        )
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
                        .send_message(
                            msg.chat.id,
                            "Usage: /digest <SYMBOL>\nExample: /digest BTC-USDT",
                        )
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
                        .send_message(
                            msg.chat.id,
                            "Usage: /weights <SYMBOL>\nExample: /weights BTC-USDT",
                        )
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
                            format!(
                                "Unknown ticker: {symbol}. Use /list to see available tickers."
                            ),
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

// ─── Trade-plan presentation (U5) ────────────────────────────────────────────
//
// Directional (LONG/SHORT) plans always render `tf=<canonical>` (`tf=n/a`
// when missing). Settled plans render `status=take_profit|stop_loss|ambiguous`;
// unresolved plans render `status=pending`. Plan tallies are display-only:
// numeric accuracy stays based on `Prediction.outcome_score` and
// ambiguous-only outcomes remain excluded from scoring.

/// True for actionable directional plans (LONG/SHORT). WAIT/NONE never settle.
fn is_directional_plan(plan: &memory::TradePlan) -> bool {
    plan.direction
        .parse::<algotrap::prelude::Direction>()
        .is_ok_and(|d| d.is_actionable())
}

/// Snake-case settlement status, or `pending` when unresolved.
fn plan_status_str(plan: &memory::TradePlan) -> &'static str {
    match plan.outcome.as_ref().map(|o| o.kind) {
        Some(memory::TradePlanOutcomeKind::TakeProfit) => "take_profit",
        Some(memory::TradePlanOutcomeKind::StopLoss) => "stop_loss",
        Some(memory::TradePlanOutcomeKind::Ambiguous) => "ambiguous",
        None => "pending",
    }
}

/// Timeframe token: directional plans always `Some` (`tf=n/a` when missing);
/// WAIT plans `Some` only when a timeframe is present.
fn plan_timeframe_token(plan: &memory::TradePlan) -> Option<String> {
    match plan.timeframe {
        Some(tf) => Some(format!("tf={tf}")),
        None if is_directional_plan(plan) => Some("tf=n/a".to_string()),
        None => None,
    }
}

/// Single-line plan rendering preserving the `entry=/target=/stop=` contract.
fn format_trade_plan_line(plan: &memory::TradePlan) -> String {
    let mut line = format!("  {} {}", plan.label, plan.direction);
    if let Some(tf_token) = plan_timeframe_token(plan) {
        line.push_str(&format!(" {tf_token}"));
    }
    if let Some(e) = plan.entry {
        line.push_str(&format!(" entry={e:.2}"));
    }
    if let Some(t) = plan.target {
        line.push_str(&format!(" target={t:.2}"));
    }
    if let Some(s) = plan.stop {
        line.push_str(&format!(" stop={s:.2}"));
    }
    line.push_str(&format!(" status={}", plan_status_str(plan)));
    line
}

/// Count plan outcomes across a slice: (take_profit, stop_loss, ambiguous, pending).
fn count_plan_outcomes(plans: &[memory::TradePlan]) -> (usize, usize, usize, usize) {
    let mut tp = 0usize;
    let mut sl = 0usize;
    let mut amb = 0usize;
    let mut pending = 0usize;
    for plan in plans {
        match plan.outcome.as_ref().map(|o| o.kind) {
            Some(memory::TradePlanOutcomeKind::TakeProfit) => tp += 1,
            Some(memory::TradePlanOutcomeKind::StopLoss) => sl += 1,
            Some(memory::TradePlanOutcomeKind::Ambiguous) => amb += 1,
            None => pending += 1,
        }
    }
    (tp, sl, amb, pending)
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

    let last_tier = mem.last_notified.tier.as_deref().unwrap_or("none");

    let all_plans: Vec<memory::TradePlan> = mem
        .predictions
        .iter()
        .flat_map(|p| p.trade_plans.iter().cloned())
        .collect();
    let plans_line = if all_plans.is_empty() {
        "Plans: none yet".to_string()
    } else {
        let (tp, sl, amb, pending) = count_plan_outcomes(&all_plans);
        format!(
            "Plans: {tp} take_profit, {sl} stop_loss, {amb} ambiguous (excluded), {pending} pending"
        )
    };

    format!(
        "━━━ 📊 {symbol} Status ━━━\n\
         Last scan: {last_scan}\n\
         Predictions: {pred_count} ({scored} validated)\n\
         Weights: {} indicators\n\
         Last tier: {last_tier}\n\
         {plans_line}",
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
            .copied()
            .flatten()
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
                text.push_str(&format!("\n{}", format_trade_plan_line(plan)));
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
) -> Result<(llm::AnalysisResult, Vec<(String, Vec<u8>)>), Box<dyn core::error::Error + Send + Sync>>
{
    // 1. Fetch market data (manual mode uses default indicator config)
    let ic = crate::memory::IndicatorConfig::default();
    let data = data::fetch_all_data(&state.bingx, ticker, &ic).await?;
    let all_dfs = &data.dfs;
    let gap_zones = &data.gap_zones;

    // 2. Capture chart screenshots for all TFs
    let mut tf_charts: Vec<(String, Vec<u8>)> = Vec::new();
    for tf in &ticker.tfs {
        let tf_label = tf.to_string();
        let df = match all_dfs.get(tf) {
            Some(df) => df,
            None => continue,
        };
        let last_rssi = crate::chart::last_rssi_from_df(df.as_ref());
        let rssi_tint = crate::chart::rssi_tint_class(last_rssi);
        let gap_zones_json =
            crate::chart::gap_zones_to_chart_json(gap_zones.get(tf).map(Vec::as_slice).unwrap_or(&[]));
        let chart_html = match crate::chart::render_single_tf_chart_html(
            tf,
            df.as_ref(),
            ticker,
            &gap_zones_json,
            rssi_tint,
        ) {
            Ok(html) => html,
            Err(e) => {
                error!(tf = %tf_label, "Failed to render chart: {e:#}");
                continue;
            }
        };
        match crate::browserless::capture_chart_screenshot(&chart_html, &state.conf.browserless_url)
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
        all_dfs,
        gap_zones,
        llm::AnalysisMode::FullAnalysis,
        None,
    )
    .await?;

    Ok((result, tf_charts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_format_digest_handles_unavailable_snapshot_values() {
        let mut mem = memory::TickerMemory::new("BTC-USDT");
        let mut indicators = memory::CANONICAL_TRACKED_INDICATOR_KEYS
            .iter()
            .map(|key| ((*key).to_string(), None))
            .collect::<std::collections::HashMap<_, _>>();
        indicators.insert("close".into(), None);
        mem.predictions.push(memory::Prediction {
            timestamp: Utc::now(),
            confidence: 55.0,
            direction: algotrap::prelude::Direction::Long,
            summary: "test".into(),
            trade_plans: vec![],
            indicators,
            outcome_score: None,
        });

        let digest = format_digest("BTC-USDT", &mem);
        assert!(digest.contains("Last-scan price: N/A"));
    }

    // ─── U5 trade-plan presentation ──────────────────────────────────────

    fn u5_plan(
        label: &str,
        direction: &str,
        timeframe: Option<algotrap::prelude::Timeframe>,
        outcome: Option<memory::TradePlanOutcomeKind>,
    ) -> memory::TradePlan {
        memory::TradePlan {
            label: label.to_string(),
            direction: direction.to_string(),
            entry: Some(100.0),
            target: Some(110.0),
            stop: Some(95.0),
            rationale: String::new(),
            timeframe,
            outcome: outcome.map(|kind| memory::TradePlanOutcome {
                kind,
                entry_hit_at: None,
                resolved_at: chrono::Utc::now(),
                resolution_timeframe: algotrap::prelude::Timeframe::H4,
                lowest_reached: 95.0,
                highest_reached: 110.0,
            }),
        }
    }

    fn u5_prediction_with_plans(plans: Vec<memory::TradePlan>) -> memory::Prediction {
        let indicators = memory::CANONICAL_TRACKED_INDICATOR_KEYS
            .iter()
            .map(|key| ((*key).to_string(), None))
            .collect::<std::collections::HashMap<_, _>>();
        memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: algotrap::prelude::Direction::Long,
            summary: "u5".into(),
            trade_plans: plans,
            indicators,
            outcome_score: None,
        }
    }

    #[test]
    fn test_u5_digest_directional_plan_shows_timeframe_and_pending() {
        let mut mem = memory::TickerMemory::new("BTC-USDT");
        mem.predictions.push(u5_prediction_with_plans(vec![u5_plan(
            "A",
            "LONG",
            Some(algotrap::prelude::Timeframe::H4),
            None,
        )]));
        let digest = format_digest("BTC-USDT", &mem);
        assert!(
            digest.contains("A LONG"),
            "label+direction preserved: {digest}"
        );
        assert!(
            digest.contains("tf=4h"),
            "exact timeframe required: {digest}"
        );
        assert!(
            digest.contains("status=pending"),
            "pending distinguished: {digest}"
        );
    }

    #[test]
    fn test_u5_digest_settled_statuses_use_snake_case() {
        let mut mem = memory::TickerMemory::new("BTC-USDT");
        mem.predictions.push(u5_prediction_with_plans(vec![
            u5_plan(
                "A",
                "LONG",
                Some(algotrap::prelude::Timeframe::H4),
                Some(memory::TradePlanOutcomeKind::TakeProfit),
            ),
            u5_plan(
                "B",
                "SHORT",
                Some(algotrap::prelude::Timeframe::D1),
                Some(memory::TradePlanOutcomeKind::StopLoss),
            ),
            u5_plan(
                "C",
                "LONG",
                Some(algotrap::prelude::Timeframe::H4),
                Some(memory::TradePlanOutcomeKind::Ambiguous),
            ),
        ]));
        let digest = format_digest("BTC-USDT", &mem);
        assert!(digest.contains("status=take_profit"), "{digest}");
        assert!(digest.contains("status=stop_loss"), "{digest}");
        assert!(digest.contains("status=ambiguous"), "{digest}");
        assert!(digest.contains("tf=1d"), "exact 1d timeframe: {digest}");
    }

    #[test]
    fn test_u5_status_preserves_legacy_fields_and_reports_plan_tally() {
        let mut mem = memory::TickerMemory::new("BTC-USDT");
        mem.predictions.push(u5_prediction_with_plans(vec![
            u5_plan(
                "A",
                "LONG",
                Some(algotrap::prelude::Timeframe::H4),
                Some(memory::TradePlanOutcomeKind::TakeProfit),
            ),
            u5_plan("B", "SHORT", Some(algotrap::prelude::Timeframe::H4), None),
        ]));
        let status = format_status("BTC-USDT", &mem);
        assert!(
            status.contains("Last scan:"),
            "legacy field preserved: {status}"
        );
        assert!(
            status.contains("Predictions:"),
            "legacy field preserved: {status}"
        );
        assert!(
            status.contains("Last tier:"),
            "legacy field preserved: {status}"
        );
        assert!(status.contains("1 take_profit"), "{status}");
        assert!(status.contains("1 pending"), "{status}");
        assert!(status.contains("ambiguous"), "{status}");
    }

    #[test]
    fn test_u5_status_without_plans_reports_none_yet() {
        let mem = memory::TickerMemory::new("BTC-USDT");
        let status = format_status("BTC-USDT", &mem);
        assert!(status.contains("Predictions: 0 (0 validated)"), "{status}");
        assert!(status.contains("Plans: none yet"), "{status}");
    }
}

use std::collections::HashMap;
use std::sync::Arc;

use async_openai::Client as OpenAIClient;
use async_openai::config::OpenAIConfig;
use core::error::Error;
use core::time::Duration;
use dotenv::dotenv;
use reqwest;
use teloxide::prelude::*;
use tracing::{error, info, warn};

use algotrap::engine::traits::ComputedFrame;
use telegrambot::commands::{self, HandlerState};
use telegrambot::config::EnvConf;
use telegrambot::{data, llm, telegram};

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::fmt::init();
    dotenv().ok();

    let conf: EnvConf = envy::from_env()?;
    conf.validate()?;
    info!(
        tickers = conf.tickers.len(),
        scan_interval = conf.scan_interval_secs,
        tier_alert = conf.tier_alert_threshold,
        tier_watch = conf.tier_watch_threshold,
        "Starting telegrambot — adaptive alert mode"
    );

    for tc in &conf.tickers {
        info!(
            symbol = %tc.symbol,
            tfs = ?tc.tfs,
            default_tf = %tc.default_tf,
            "Loaded ticker config"
        );
    }

    // Single shared Bot instance — clone() shares the internal Arc<reqwest::Client>
    // rather than creating separate HTTP clients that race on Telegram's getUpdates.
    let bot = Bot::new(&conf.telegram_bot_token);
    let scan_bot = bot.clone();
    let cmd_bot = bot;
    let bingx = Arc::new(algotrap::ext::bingx::BingXClient::default());
    let openai_config = OpenAIConfig::new()
        .with_api_base(&conf.llm_api_base)
        .with_api_key(&conf.llm_api_key);
    let llm_http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .expect("Failed to build reqwest client for LLM");
    let llm_client = Arc::new(OpenAIClient::build(
        llm_http_client,
        openai_config,
        Default::default(),
    ));
    let conf = Arc::new(conf);

    // Shared state for command handlers
    let state = HandlerState {
        conf: Arc::clone(&conf),
        bingx: Arc::clone(&bingx),
        llm_client: Arc::clone(&llm_client),
    };

    // ─── Spawn concurrent tasks (ADR-3) ──────────────────────────────────

    // Task 1: Alert scan loop
    let scan_conf = Arc::clone(&conf);
    let scan_bingx = Arc::clone(&bingx);
    let scan_llm = Arc::clone(&llm_client);
    let scan_handle = tokio::spawn(async move {
        run_alert_scan_loop(scan_conf, &scan_bot, &scan_bingx, &scan_llm).await;
    });

    // Task 2: Telegram command dispatcher
    let cmd_handle = tokio::spawn(async move {
        commands::run_command_dispatcher(cmd_bot, state).await;
    });

    // Wait for either task to finish (shouldn't happen unless error/shutdown)
    tokio::select! {
        _ = scan_handle => warn!("Alert scan loop exited unexpectedly"),
        _ = cmd_handle => warn!("Command dispatcher exited unexpectedly"),
    }

    Ok(())
}

// ─── Alert Scan Loop ─────────────────────────────────────────────────────────

async fn run_alert_scan_loop(
    conf: Arc<EnvConf>,
    bot: &Bot,
    _bingx: &algotrap::ext::bingx::BingXClient,
    llm_client: &OpenAIClient<OpenAIConfig>,
) {
    // Semaphore to limit concurrent ticker scans (avoids overloading Browserless)
    let semaphore = Arc::new(tokio::sync::Semaphore::new(3));

    loop {
        let cycle_start = tokio::time::Instant::now();
        info!(
            "Starting alert scan cycle for {} tickers",
            conf.tickers.len()
        );

        // Spawn all tickers concurrently, bounded by semaphore
        let mut handles = Vec::new();
        for ticker in &conf.tickers {
            let sem = Arc::clone(&semaphore);
            let conf = Arc::clone(&conf);
            let bot = bot.clone();
            let bingx_client = algotrap::ext::bingx::BingXClient::default();
            let llm = llm_client.clone();
            let ticker = ticker.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("Semaphore closed");
                match tokio::time::timeout(
                    Duration::from_secs(600),
                    scan_ticker(&conf, &bot, &bingx_client, &llm, &ticker),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => error!(symbol = %ticker.symbol, "Scan failed: {e:#}"),
                    Err(_) => error!(symbol = %ticker.symbol, "Scan timed out after 10 minutes"),
                }
            }));
        }

        // Wait for all tickers to finish
        for handle in handles {
            let _ = handle.await;
        }

        // Compact bloated KB topics (once per cycle, not per ticker)
        llm::compact_kb_if_needed(llm_client, &conf).await;

        let elapsed = cycle_start.elapsed();
        info!(
            elapsed_secs = elapsed.as_secs(),
            "Alert scan cycle complete"
        );

        // Sleep for the remaining interval (accounting for scan duration)
        let interval = Duration::from_secs(conf.scan_interval_secs);
        if elapsed < interval {
            tokio::time::sleep(interval - elapsed).await;
        } else {
            warn!(
                elapsed_secs = elapsed.as_secs(),
                interval_secs = conf.scan_interval_secs,
                "Scan cycle exceeded interval — starting next cycle immediately"
            );
        }
    }
}

async fn scan_ticker(
    conf: &EnvConf,
    bot: &Bot,
    bingx: &algotrap::ext::bingx::BingXClient,
    llm_client: &OpenAIClient<OpenAIConfig>,
    ticker: &telegrambot::config::TickerConf,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!(symbol = %ticker.symbol, "Scanning ticker");

    // 1. Load persistent memory
    let mut mem = telegrambot::memory::load_memory(&conf.memory_dir, &ticker.symbol);

    // 1.5. Structural compatibility check — compare stored indicator key set
    // against the current pipeline. Mismatch = reset predictions + weights.
    const INDICATOR_KEYS: &[&str] = &[
        "rssi",
        "structure_power",
        "band_reversion",
        "atr_percent",
        "sharpe",
        "close",
    ];
    telegrambot::memory::check_schema_compatibility(&mut mem, INDICATOR_KEYS);

    // 2. Fetch market data
    let all_dfs = data::fetch_all_data(bingx, ticker, &mem.indicator_config).await?;
    info!(
        symbol = %ticker.symbol,
        timeframes = all_dfs.len(),
        "Fetched market data"
    );

    // 3. Outcome validation at scan start — validate non-scored predictions
    if let Some(current_price) = get_latest_close(&all_dfs, &ticker.default_tf) {
        for pred in &mut mem.predictions {
            if pred.outcome_score.is_none() {
                let entry_price = pred
                    .indicators
                    .get("close")
                    .copied()
                    .unwrap_or(current_price);
                let atr = telegrambot::scoring::reconstruct_atr(&pred.indicators);
                let score = telegrambot::scoring::compute_outcome_score(
                    pred.direction,
                    entry_price,
                    current_price,
                    atr,
                );
                pred.outcome_score = Some(score);
                info!(
                    symbol = %ticker.symbol,
                    ts = %pred.timestamp,
                    direction = %pred.direction,
                    score,
                    atr = ?atr,
                    "Validated prediction outcome"
                );
            }
        }
    }

    // 4. Seed KB on first run
    if let Err(e) = telegrambot::kb::seed_kb(&conf.memory_dir) {
        warn!(symbol = %ticker.symbol, "KB seed failed: {e}");
    }

    // 5. Run LLM agent in adaptive scan mode
    let result = llm::run_agent(
        llm_client,
        conf,
        ticker,
        &all_dfs,
        llm::AnalysisMode::AlertScan,
        Some(&mem),
    )
    .await?;

    info!(
        symbol = %ticker.symbol,
        confidence = result.confidence,
        direction = %result.direction,
        "Adaptive scan result"
    );

    // 6. Classify tier
    let tier = telegrambot::scoring::classify_tier(
        result.confidence,
        conf.tier_alert_threshold,
        conf.tier_watch_threshold,
    );

    // 7. Extract current indicator snapshot for change detection
    let current_indicators = extract_indicator_snapshot(&all_dfs, &ticker.default_tf);

    // 8. Check significant change
    let indicator_keys =
        telegrambot::scoring::parse_indicator_keys(&conf.change_detection_indicators);
    let (has_change, max_delta) = telegrambot::scoring::detect_significant_change(
        &mem.last_notified.indicators,
        &current_indicators,
        &indicator_keys,
        mem.weights.significance_threshold,
    );

    // 9. Decide whether to notify
    let prev_tier = mem.last_notified.tier.as_deref();
    let should_send = telegrambot::scoring::should_notify(
        tier,
        prev_tier,
        has_change,
        mem.last_notified.timestamp,
        conf.notification_cooldown_secs,
        result.direction,
    );

    info!(
        symbol = %ticker.symbol,
        tier = %tier,
        should_send,
        max_delta,
        direction = %result.direction,
        conviction_aligned = result.conviction_aligned,
        "Notification decision"
    );

    // 10. Update weights from LLM response (apply guardrails)
    if let Some(ref proposed) = result.proposed_weights {
        let guarded = telegrambot::memory::apply_weight_guardrails(
            &mem.weights.values,
            proposed,
            conf.weight_min,
            conf.weight_max,
            conf.weight_rate_limit,
        );
        mem.weights.values = guarded;
    }
    if let Some(threshold) = result.significance_threshold {
        mem.weights.significance_threshold = threshold.clamp(0.0, 1.0);
    }

    // 10.5. Apply indicator parameter tuning (if LLM proposed any)
    if let Some(ref proposed_params) = result.proposed_indicator_params {
        mem.indicator_config.apply_proposed(proposed_params);
    }
    // Tick dormant indicator cycle counters
    mem.indicator_config.tick_dormant();

    // 11. Store prediction in memory (always, even if Silent)
    let prediction = telegrambot::memory::Prediction {
        timestamp: chrono::Utc::now(),
        confidence: result.confidence,
        direction: result.direction,
        summary: result.text.clone(),
        trade_plans: result.trade_plans.clone(),
        indicators: current_indicators.clone(),
        outcome_score: None,
    };
    telegrambot::memory::append_prediction(&mut mem, prediction, conf.max_predictions);

    // 11. If notifying, capture charts (Alert: always, Watch: only if ≥ 50%) and send
    if should_send {
        let chat_id = ChatId(conf.telegram_chat_id);

        // Capture charts if confidence ≥ 50
        let tf_charts = if result.confidence >= 50.0 {
            capture_ticker_charts(conf, ticker, &all_dfs, &mem.indicator_config).await
        } else {
            vec![]
        };

        match tier {
            telegrambot::scoring::Tier::Alert => {
                telegram::send_alert(
                    bot,
                    chat_id,
                    &ticker.symbol,
                    result.direction,
                    result.confidence,
                    &result.text,
                    &tf_charts,
                )
                .await?;
            }
            telegrambot::scoring::Tier::Watch => {
                // Prepend conviction marker if misaligned
                let summary_text = if !result.conviction_aligned {
                    format!("⚠️ Low conviction\n\n{}", result.text)
                } else {
                    result.text.clone()
                };
                telegram::send_watch_notification(
                    bot,
                    chat_id,
                    &ticker.symbol,
                    result.direction,
                    result.confidence,
                    &summary_text,
                    &result.trade_plans,
                    &tf_charts,
                )
                .await?;
            }
            telegrambot::scoring::Tier::Silent => {
                // Should not reach here (should_notify returns false for Silent)
            }
        }

        // Update last-notified snapshot
        mem.last_notified = telegrambot::memory::NotifiedSnapshot {
            indicators: current_indicators,
            timestamp: Some(chrono::Utc::now()),
            tier: Some(tier.to_string()),
        };
    }

    // 12. Save memory
    if let Err(e) = telegrambot::memory::save_memory(&conf.memory_dir, &mem) {
        error!(symbol = %ticker.symbol, "Failed to save memory: {e}");
    }

    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Get the latest close price from the market data for a given timeframe.
fn get_latest_close(
    all_dfs: &HashMap<algotrap::prelude::Timeframe, Box<dyn ComputedFrame>>,
    tf: &algotrap::prelude::Timeframe,
) -> Option<f64> {
    all_dfs.get(tf).and_then(|df| {
        df.f64_at("close", df.len().saturating_sub(1))
            .ok()
            .flatten()
    })
}

/// Extract indicator values from the latest candle for change detection.
fn extract_indicator_snapshot(
    all_dfs: &HashMap<algotrap::prelude::Timeframe, Box<dyn ComputedFrame>>,
    default_tf: &algotrap::prelude::Timeframe,
) -> std::collections::HashMap<String, f64> {
    let mut snapshot = std::collections::HashMap::new();

    if let Some(df) = all_dfs.get(default_tf) {
        let last = match df.slice_last(1) {
            Ok(last) => last,
            Err(_) => return snapshot,
        };
        for col_name in &[
            "rssi",
            "structure_power",
            "band_reversion",
            "atr_percent",
            "sharpe",
            "close",
        ] {
            if let Ok(Some(f)) = last.f64_at(col_name, 0) {
                snapshot.insert((*col_name).to_string(), f);
            }
        }
    }

    snapshot
}

/// Capture chart screenshots for all configured timeframes.
async fn capture_ticker_charts(
    conf: &EnvConf,
    ticker: &telegrambot::config::TickerConf,
    all_dfs: &HashMap<algotrap::prelude::Timeframe, Box<dyn ComputedFrame>>,
    ic: &telegrambot::memory::IndicatorConfig,
) -> Vec<(String, Vec<u8>)> {
    let mut tf_charts = Vec::new();

    for tf in &ticker.tfs {
        let tf_label = tf.to_string();
        let df = match all_dfs.get(tf) {
            Some(df) => df,
            None => continue,
        };
        let last_rssi = telegrambot::chart::last_rssi_from_df(df.as_ref());
        let rssi_tint = telegrambot::chart::rssi_tint_class(last_rssi);
        let params = ic.gap_zone_params();
        let raw_df = df.as_dataframe();
        let zones = algotrap::ta::gap_zones::extract_gap_zones(raw_df, &params);
        let gap_zones_json = telegrambot::chart::gap_zones_to_chart_json(&zones, 0.3);
        let chart_html = match telegrambot::chart::render_single_tf_chart_html(
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
        match telegrambot::browserless::capture_chart_screenshot(&chart_html, &conf.browserless_url)
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

    tf_charts
}

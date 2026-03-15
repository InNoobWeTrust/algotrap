use std::sync::Arc;

use async_openai::Client as OpenAIClient;
use async_openai::config::OpenAIConfig;
use core::error::Error;
use core::time::Duration;
use dotenv::dotenv;
use teloxide::prelude::*;
use tracing::{error, info, warn};

use telegrambot::commands::{self, HandlerState};
use telegrambot::config::EnvConf;
use telegrambot::{data, llm, telegram};

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::fmt::init();
    dotenv().ok();

    let conf: EnvConf = envy::from_env()?;
    info!(
        tickers = conf.tickers.len(),
        scan_interval = conf.scan_interval_secs,
        confidence_threshold = conf.confidence_threshold,
        "Starting telegrambot — multi-ticker alert mode"
    );

    for tc in &conf.tickers {
        info!(
            symbol = %tc.symbol,
            tfs = ?tc.tfs,
            default_tf = %tc.default_tf,
            "Loaded ticker config"
        );
    }

    let bot = Bot::new(&conf.telegram_bot_token);
    let bingx = Arc::new(algotrap::ext::bingx::BingXClient::default());
    let openai_config = OpenAIConfig::new()
        .with_api_base(&conf.llm_api_base)
        .with_api_key(&conf.llm_api_key);
    let llm_client = Arc::new(OpenAIClient::with_config(openai_config));
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
    let scan_bot = bot.clone();
    let scan_bingx = Arc::clone(&bingx);
    let scan_llm = Arc::clone(&llm_client);
    let scan_handle = tokio::spawn(async move {
        run_alert_scan_loop(&scan_conf, &scan_bot, &scan_bingx, &scan_llm).await;
    });

    // Task 2: Telegram command dispatcher
    let cmd_bot = bot.clone();
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
    conf: &EnvConf,
    bot: &Bot,
    bingx: &algotrap::ext::bingx::BingXClient,
    llm_client: &OpenAIClient<OpenAIConfig>,
) {
    loop {
        let cycle_start = tokio::time::Instant::now();
        info!("Starting alert scan cycle for {} tickers", conf.tickers.len());

        for ticker in &conf.tickers {
            match scan_ticker(conf, bot, bingx, llm_client, ticker).await {
                Ok(()) => {}
                Err(e) => {
                    error!(symbol = %ticker.symbol, "Scan failed: {e:#}");
                    // Continue to next ticker — don't block the cycle
                }
            }
        }

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
    info!(symbol = %ticker.symbol, "Scanning ticker for entry");

    // 1. Fetch market data
    let all_dfs = data::fetch_all_data(bingx, ticker).await?;
    info!(
        symbol = %ticker.symbol,
        timeframes = all_dfs.len(),
        "Fetched market data"
    );

    // 2. Run LLM agent in alert scan mode (no capture_chart tool)
    let result = llm::run_agent(llm_client, conf, ticker, &all_dfs, llm::AnalysisMode::AlertScan)
        .await?;

    info!(
        symbol = %ticker.symbol,
        confidence = result.confidence,
        direction = %result.direction,
        "Alert scan result"
    );

    // 3. Check confidence threshold
    if result.confidence < conf.confidence_threshold {
        info!(
            symbol = %ticker.symbol,
            confidence = result.confidence,
            threshold = conf.confidence_threshold,
            "Below threshold, skipping alert"
        );
        return Ok(());
    }

    // 4. Confidence meets threshold — capture charts first, then send alert
    info!(
        symbol = %ticker.symbol,
        confidence = result.confidence,
        direction = %result.direction,
        "🎯 Entry detected! Capturing charts and sending alert"
    );

    let mut tf_charts: Vec<(String, Vec<u8>)> = Vec::new();
    for tf in &ticker.tfs {
        let tf_label = tf.to_string();
        let df = match all_dfs.get(tf) {
            Some(df) => df,
            None => continue,
        };
        let chart_html =
            match telegrambot::chart::render_single_tf_chart_html(tf, df, ticker) {
                Ok(html) => html,
                Err(e) => {
                    error!(tf = %tf_label, "Failed to render chart: {e:#}");
                    continue;
                }
            };
        match telegrambot::browserless::capture_chart_screenshot(
            &chart_html,
            &conf.browserless_url,
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

    // 5. Send alert to Telegram
    let chat_id = ChatId(conf.telegram_chat_id);
    telegram::send_alert(
        bot,
        chat_id,
        &ticker.symbol,
        &result.direction,
        result.confidence,
        &result.text,
        &tf_charts,
    )
    .await?;

    Ok(())
}

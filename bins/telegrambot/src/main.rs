use async_openai::Client as OpenAIClient;
use async_openai::config::OpenAIConfig;
use core::error::Error;
use core::time::Duration;
use dotenv::dotenv;
use teloxide::prelude::*;
use tracing::{error, info};

use telegrambot::config::EnvConf;
use telegrambot::{data, llm, telegram};

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::fmt::init();
    dotenv().ok();

    let conf: EnvConf = envy::from_env()?;
    info!(
        symbol = %conf.symbol,
        tfs = ?conf.tfs,
        interval = conf.analysis_interval_secs,
        "Starting telegrambot"
    );

    let bot = Bot::new(&conf.telegram_bot_token);
    let bingx = algotrap::ext::bingx::BingXClient::default();
    let openai_config = OpenAIConfig::new()
        .with_api_base(&conf.llm_api_base)
        .with_api_key(&conf.llm_api_key);
    let llm_client = OpenAIClient::with_config(openai_config);

    loop {
        info!("Starting analysis cycle");
        match run_analysis_cycle(&conf, &bot, &bingx, &llm_client).await {
            Ok(()) => info!("Analysis cycle completed successfully"),
            Err(e) => error!("Analysis cycle failed: {e:#}"),
        }
        info!(
            interval = conf.analysis_interval_secs,
            "Sleeping until next cycle"
        );
        tokio::time::sleep(Duration::from_secs(conf.analysis_interval_secs)).await;
    }
}

// ─── Analysis Cycle ──────────────────────────────────────────────────────────

async fn run_analysis_cycle(
    conf: &EnvConf,
    bot: &Bot,
    bingx: &algotrap::ext::bingx::BingXClient,
    llm_client: &OpenAIClient<OpenAIConfig>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // 1. Fetch market data across all timeframes
    let all_dfs = data::fetch_all_data(bingx, conf).await?;
    info!(
        timeframes = all_dfs.len(),
        "Fetched and processed market data"
    );

    // 2. Capture chart screenshots for ALL timeframes (per-TF rendering)
    let mut tf_charts: Vec<(String, Vec<u8>)> = Vec::new();
    for tf in &conf.tfs {
        let tf_label = tf.to_string();
        let df = match all_dfs.get(tf) {
            Some(df) => df,
            None => {
                error!(tf = %tf_label, "No data for timeframe, skipping chart");
                continue;
            }
        };
        let chart_html =
            match telegrambot::chart::render_single_tf_chart_html(tf, df, conf) {
                Ok(html) => html,
                Err(e) => {
                    error!(tf = %tf_label, "Failed to render chart HTML: {e:#}");
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

    // 3. Run agentic LLM analysis loop
    let (analysis_text, _) = llm::run_agent(llm_client, conf, &all_dfs).await?;
    info!(
        analysis_len = analysis_text.len(),
        chart_count = tf_charts.len(),
        "LLM analysis complete"
    );

    // 4. Send results to Telegram
    let chat_id = ChatId(conf.telegram_chat_id);
    telegram::send_analysis(bot, chat_id, &conf.symbol, &analysis_text, &tf_charts).await?;

    Ok(())
}

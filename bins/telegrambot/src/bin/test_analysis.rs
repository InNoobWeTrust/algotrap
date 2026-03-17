/// Standalone test: runs the analysis pipeline on ALL configured tickers
/// without Telegram, printing results to stdout.
///
/// Usage:
///   cargo run -p telegrambot --bin test_analysis
use async_openai::Client as OpenAIClient;
use async_openai::config::OpenAIConfig;
use dotenv::dotenv;

use telegrambot::config::EnvConf;
use telegrambot::{data, llm};

#[tokio::main]
async fn main() -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();
    dotenv().ok();

    let conf: EnvConf = envy::from_env()?;

    println!("═══════════════════════════════════════════════════════════════");
    println!(
        "  🧪 Telegrambot Multi-Ticker Test — {} tickers",
        conf.tickers.len()
    );
    println!("  LLM: {} @ {}", conf.llm_model, conf.llm_api_base);
    println!("  Browserless: {}", conf.browserless_url);
    println!("═══════════════════════════════════════════════════════════════");

    let bingx = algotrap::ext::bingx::BingXClient::default();
    let openai_config = OpenAIConfig::new()
        .with_api_base(&conf.llm_api_base)
        .with_api_key(&conf.llm_api_key);
    let llm_client = OpenAIClient::with_config(openai_config);

    for (i, ticker) in conf.tickers.iter().enumerate() {
        let bar = "━".repeat(60);
        println!(
            "\n\n{bar}\n  [{}/{}] 📊 {} — Alert Scan Mode\n{bar}",
            i + 1,
            conf.tickers.len(),
            ticker.symbol,
        );

        // 1. Fetch data
        println!("\n📡 Fetching market data for {}...", ticker.symbol);
        let all_dfs = data::fetch_all_data(&bingx, ticker).await?;
        println!("✅ Fetched {} timeframes", all_dfs.len());

        for (tf, df) in &all_dfs {
            println!("   {tf}: {} candles", df.height());
        }

        // 2. Run LLM agent in alert scan mode
        println!("\n🤖 Running LLM alert scan...");
        let mem = telegrambot::memory::load_memory(&conf.memory_dir, &ticker.symbol);
        let result = llm::run_agent(
            &llm_client,
            &conf,
            ticker,
            &all_dfs,
            llm::AnalysisMode::AlertScan,
            Some(&mem),
        )
        .await?;

        // 3. Print result
        let tier = telegrambot::scoring::classify_tier(
            result.confidence,
            conf.tier_alert_threshold,
            conf.tier_watch_threshold,
        );

        println!("\n  ┌─────────────────────────────────────────┐");
        println!(
            "  │ {} — Confidence: {:.0}%",
            ticker.symbol, result.confidence
        );
        println!("  │ Direction: {}", result.direction);
        println!("  │ Tier: {tier}");
        if !result.trade_plans.is_empty() {
            for plan in &result.trade_plans {
                println!(
                    "  │ Plan {}: {} entry={:?} target={:?} stop={:?}",
                    plan.label, plan.direction, plan.entry, plan.target, plan.stop
                );
            }
        }
        println!("  └─────────────────────────────────────────┘");
        println!("\n  Summary: {}", result.text);
    }

    println!("\n\n═══════════════════════════════════════════════════════════════");
    println!("  ✅ All {} tickers scanned!", conf.tickers.len());
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}

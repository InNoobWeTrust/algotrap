/// Standalone test: runs the analysis pipeline on BTC-USDT
/// without Telegram, printing the result to stdout.
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
    println!("  🧪 Telegrambot Analysis Test — {}", conf.symbol);
    println!("  Timeframes: {:?}", conf.tfs);
    println!("  LLM: {} @ {}", conf.llm_model, conf.llm_api_base);
    println!("  Browserless: {}", conf.browserless_url);
    println!("═══════════════════════════════════════════════════════════════");

    // 1. Fetch data
    println!("\n📡 Fetching market data...");
    let bingx = algotrap::ext::bingx::BingXClient::default();
    let all_dfs = data::fetch_all_data(&bingx, &conf).await?;
    println!("✅ Fetched {} timeframes", all_dfs.len());

    for (tf, df) in &all_dfs {
        println!("   {tf}: {} candles", df.height());
    }

    // 2. Run LLM agent
    println!("\n🤖 Running LLM agent loop...");
    let openai_config = OpenAIConfig::new()
        .with_api_base(&conf.llm_api_base)
        .with_api_key(&conf.llm_api_key);
    let llm_client = OpenAIClient::with_config(openai_config);

    let (analysis, chart_png) = llm::run_agent(&llm_client, &conf, &all_dfs).await?;

    // 3. Print results
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("  📊 Analysis Result");
    println!("═══════════════════════════════════════════════════════════════");
    println!("{analysis}");

    if let Some(png) = &chart_png {
        let path = "/tmp/telegrambot_test_chart.png";
        std::fs::write(path, png)?;
        println!("\n📸 Chart saved to: {path}");
    }

    println!("\n✅ Test complete!");
    Ok(())
}

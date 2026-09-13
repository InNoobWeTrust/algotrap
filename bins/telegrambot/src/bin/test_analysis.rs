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
    conf.validate()?;

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
        let ic = telegrambot::memory::IndicatorConfig::default();
        let data = data::fetch_all_data(&bingx, ticker, &ic).await?;
        let all_dfs = &data.dfs;
        let gap_zones = &data.gap_zones;
        println!("✅ Fetched {} timeframes", all_dfs.len());

        for (tf, df) in all_dfs {
            println!("   {tf}: {} candles", df.len());
        }

        // 2. Run LLM agent in alert scan mode
        println!("\n🤖 Running LLM alert scan...");
        let mem = telegrambot::memory::load_memory(&conf.memory_dir, &ticker.symbol);
        let result = llm::run_agent(
            &llm_client,
            &conf,
            ticker,
            all_dfs,
            gap_zones,
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
                println!("  │ {}", format_flight_plan_line(plan));
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

// ─── Flight output presentation (U5) ─────────────────────────────────────────
//
// Directional (LONG/SHORT) plans always include `tf=<canonical>` (`tf=n/a`
// when missing). Settled plans show `status=take_profit|stop_loss|ambiguous`;
// unresolved plans show `status=pending`. The `entry=/target=/stop=` contract
// is preserved; `tf=` + `status=` are appended.

/// True for actionable directional plans (LONG/SHORT). WAIT/NONE never settle.
fn is_directional_plan(plan: &telegrambot::memory::TradePlan) -> bool {
    plan.direction
        .parse::<algotrap::prelude::Direction>()
        .is_ok_and(|d| d.is_actionable())
}

/// Snake-case settlement status, or `pending` when unresolved.
fn flight_plan_status_str(plan: &telegrambot::memory::TradePlan) -> &'static str {
    match plan.outcome.as_ref().map(|o| o.kind) {
        Some(telegrambot::memory::TradePlanOutcomeKind::TakeProfit) => "take_profit",
        Some(telegrambot::memory::TradePlanOutcomeKind::StopLoss) => "stop_loss",
        Some(telegrambot::memory::TradePlanOutcomeKind::Ambiguous) => "ambiguous",
        None => "pending",
    }
}

/// Single-line flight rendering preserving entry/target/stop debug formatting.
fn format_flight_plan_line(plan: &telegrambot::memory::TradePlan) -> String {
    let mut line = format!("Plan {}: {}", plan.label, plan.direction);
    match plan.timeframe {
        Some(tf) => line.push_str(&format!(" tf={tf}")),
        None if is_directional_plan(plan) => line.push_str(" tf=n/a"),
        None => {}
    }
    line.push_str(&format!(
        " entry={:?} target={:?} stop={:?} status={}",
        plan.entry,
        plan.target,
        plan.stop,
        flight_plan_status_str(plan)
    ));
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flight_plan(
        label: &str,
        direction: &str,
        timeframe: Option<algotrap::prelude::Timeframe>,
        outcome: Option<telegrambot::memory::TradePlanOutcomeKind>,
    ) -> telegrambot::memory::TradePlan {
        telegrambot::memory::TradePlan {
            label: label.to_string(),
            direction: direction.to_string(),
            entry: Some(100.0),
            target: Some(110.0),
            stop: Some(95.0),
            rationale: String::new(),
            timeframe,
            outcome: outcome.map(|kind| telegrambot::memory::TradePlanOutcome {
                kind,
                entry_hit_at: None,
                resolved_at: chrono::Utc::now(),
                resolution_timeframe: algotrap::prelude::Timeframe::H4,
                lowest_reached: 95.0,
                highest_reached: 110.0,
            }),
        }
    }

    #[test]
    fn test_u5_flight_directional_includes_timeframe_and_pending() {
        let plan = flight_plan("A", "LONG", Some(algotrap::prelude::Timeframe::H4), None);
        let line = format_flight_plan_line(&plan);
        assert!(line.contains("Plan A: LONG"), "{line}");
        assert!(line.contains("tf=4h"), "{line}");
        assert!(line.contains("status=pending"), "{line}");
        assert!(line.contains("entry="), "levels preserved: {line}");
    }

    #[test]
    fn test_u5_flight_settled_statuses_use_snake_case() {
        for (kind, expected) in [
            (
                telegrambot::memory::TradePlanOutcomeKind::TakeProfit,
                "status=take_profit",
            ),
            (
                telegrambot::memory::TradePlanOutcomeKind::StopLoss,
                "status=stop_loss",
            ),
            (
                telegrambot::memory::TradePlanOutcomeKind::Ambiguous,
                "status=ambiguous",
            ),
        ] {
            let plan = flight_plan(
                "A",
                "LONG",
                Some(algotrap::prelude::Timeframe::H4),
                Some(kind),
            );
            let line = format_flight_plan_line(&plan);
            assert!(line.contains(expected), "{line}");
        }
    }

    #[test]
    fn test_u5_flight_directional_missing_timeframe_is_explicit() {
        let plan = flight_plan("B", "SHORT", None, None);
        let line = format_flight_plan_line(&plan);
        assert!(line.contains("tf=n/a"), "{line}");
        assert!(line.contains("status=pending"), "{line}");
    }
}

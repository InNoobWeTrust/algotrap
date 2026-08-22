/// Quick chart render test — fetches real BingX data, renders chart HTML
/// to /tmp, and opens in default browser for visual inspection.
///
/// Usage:
///   cargo run -p telegrambot --bin test_chart_render
use std::io::Write;

use algotrap::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    // Use production ticker configs
    let tickers = vec![
        telegrambot::config::TickerConf {
            symbol: "BTC-USDT".into(),
            sl_percent: 0.1,
            tol_percent: 0.618,
            tfs: vec![Timeframe::M15, Timeframe::H1, Timeframe::H4],
            default_tf: Timeframe::H1,
        },
        telegrambot::config::TickerConf {
            symbol: "ETH-USDT".into(),
            sl_percent: 0.08,
            tol_percent: 0.5,
            tfs: vec![Timeframe::M15, Timeframe::H1, Timeframe::H4],
            default_tf: Timeframe::H4,
        },
    ];

    let bingx = algotrap::ext::bingx::BingXClient::default();
    let ic = telegrambot::memory::IndicatorConfig::default();

    let out_dir = std::path::PathBuf::from("/tmp/algotrap_chart_test");
    std::fs::create_dir_all(&out_dir)?;

    let mut html_paths = Vec::new();

    for ticker in &tickers {
        println!("📡 Fetching data for {}...", ticker.symbol);
        let all_dfs = telegrambot::data::fetch_all_data(&bingx, ticker, &ic).await?;
        println!("✅ Fetched {} timeframes", all_dfs.len());

        for tf in &ticker.tfs {
            let df = match all_dfs.get(tf) {
                Some(df) => df,
                None => continue,
            };
            println!("  📊 {} {} — {} candles", ticker.symbol, tf, df.len());

            let last_rssi = telegrambot::chart::last_rssi_from_df(df.as_ref());
            let rssi_tint = telegrambot::chart::rssi_tint_class(last_rssi);

            // Extract gap zones for this TF
            let params = ic.gap_zone_params();
            let zones =
                algotrap::engine::gap_zones::extract_gap_zones_from_frame(df.as_ref(), &params)?;
            let gap_zones_json = telegrambot::chart::gap_zones_to_chart_json(&zones, 0.3);
            println!(
                "    gap zones: {} (of {} raw)",
                zones.len().min(10),
                zones.len()
            );

            let chart_html = telegrambot::chart::render_single_tf_chart_html(
                tf,
                df.as_ref(),
                ticker,
                &gap_zones_json,
                rssi_tint,
            )?;

            let filename = format!(
                "{}_{}.html",
                ticker.symbol.replace('-', "_").to_lowercase(),
                tf
            );
            let path = out_dir.join(&filename);
            let mut f = std::fs::File::create(&path)?;
            f.write_all(chart_html.as_bytes())?;
            println!("  ✅ Written: {}", path.display());
            html_paths.push(path);
        }
    }

    // Create an index page linking to all charts
    let mut index = String::from(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"/><title>Chart Render Test</title>
<style>
body { font-family: system-ui; background: #1a1a1a; color: #eee; padding: 20px; }
a { color: #64b5f6; text-decoration: none; font-size: 18px; }
a:hover { text-decoration: underline; }
li { margin: 8px 0; }
h1 { color: #fff; }
</style></head><body>
<h1>🧪 Chart Render Test</h1><ul>
"#,
    );
    for p in &html_paths {
        let name = p.file_stem().unwrap().to_string_lossy();
        index.push_str(&format!(
            "<li><a href=\"{}\" target=\"_blank\">{}</a></li>\n",
            p.display(),
            name
        ));
    }
    index.push_str("</ul></body></html>");
    let index_path = out_dir.join("index.html");
    std::fs::write(&index_path, &index)?;

    println!("\n🌐 Opening index in browser...");
    std::process::Command::new("open")
        .arg(&index_path)
        .spawn()?;

    println!("✅ Done! Charts written to {}", out_dir.display());
    Ok(())
}

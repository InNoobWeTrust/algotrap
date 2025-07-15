use algotrap::ext::{webdriver::*, yfinance};
use algotrap::prelude::*;
use chrono::{TimeZone, Utc};
use core::error::Error;
use fantoccini::Locator;
use std::io::IsTerminal;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    setup_tracing();
    let geckodriver = GeckoDriver::default();
    let client = geckodriver.create_client(false).await?;
    client
        .goto("https://farside.co.uk/bitcoin-etf-flow-all-data/")
        .await?;
    let elem = client.find(Locator::Css("table.etf")).await?;
    let tb_df = client
        .extract_table(
            &elem,
            Some(
                r#"
const table = arguments[0];
const rows = table.rows;
const headers = [];
const jsonData = [];

// Extract headers
for (let i = 0; i < rows[0].cells.length; i++) {
    headers.push(rows[0].cells[i].innerText);
}

// Extract data
for (let i = 2; i < rows.length - 4; i++) {
    const rowObject = {};
    const cells = rows[i].cells;
    for (let j = 0; j < cells.length; j++) {
        let innerTxt = cells[j].innerText;
        if (innerTxt == '-') {
            rowObject[headers[j]] = null;
        } else {
            rowObject[headers[j]] = cells[j].innerText;
        }
    }
    jsonData.push(rowObject);
}

return jsonData
"#
                .to_owned(),
            ),
        )
        .await?;
    // Dispose webdriver before printing to avoid polluting console logs
    drop(client);
    drop(geckodriver);
    dbg!(&tb_df);
    let client = yfinance::YfinanceClient::new();
    let start = Utc
        .with_ymd_and_hms(2009, 1, 3, 0, 0, 0)
        .unwrap()
        .timestamp();
    let end = Utc
        .with_ymd_and_hms(2025, 7, 7, 23, 59, 59)
        .unwrap()
        .timestamp();
    // returns historic quotes with daily interval
    let klines = client
        .get_quote_history("BTC-USD", start, end, yfinance::YfinanceInterval::D1)
        .await?;
    let df = klines.iter().cloned().to_dataframe().unwrap();
    println!("BTC-USD quotes:\n{df:#?}");
    Ok(())
}

fn setup_tracing() {
    let subscriber = tracing_subscriber::Registry::default()
        .with(
            // stdout layer, to view everything in the console
            tracing_subscriber::fmt::layer()
                .compact()
                .with_ansi(std::io::stdin().is_terminal())
                .with_file(true)
                .with_line_number(true)
                .with_filter(tracing::level_filters::LevelFilter::INFO),
        )
        .with(
            tracing_subscriber::filter::targets::Targets::new()
                .with_target("etf_dashboard", tracing::level_filters::LevelFilter::DEBUG),
        );
    tracing::subscriber::set_global_default(subscriber).unwrap();
}

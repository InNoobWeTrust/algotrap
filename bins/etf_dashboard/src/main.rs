use algotrap::ext::{webdriver::*, yfinance};
use algotrap::prelude::*;
use core::error::Error;
use fantoccini::Locator;
use polars::prelude::*;
use std::io::IsTerminal;
use tracing::info;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    setup_tracing();
    let mut etf_dfs = Vec::new();
    let mut ticker_dfs = Vec::new();
    let srcs = vec![
        (BTC_TICKER, ETF_BTC_URL, ETF_BTC_EXTRACT_SCRIPT),
        (ETH_TICKER, ETF_ETH_URL, ETF_ETH_EXTRACT_SCRIPT),
        (SOL_TICKER, ETF_SOL_URL, ETF_SOL_EXTRACT_SCRIPT),
    ];
    for (ticker, url, script) in srcs {
        // Inner scope to automatically dispose webdriver before printing to avoid polluting console logs
        let geckodriver = GeckoDriver::default();
        let client = geckodriver.create_client(false).await?;
        info!("Going to {url}...");
        client.goto(url).await?;
        let elem = client.find(Locator::Css("table.etf")).await?;
        let etf_df = client
            .extract_table(&elem, Some(script.to_string()))
            .await?;
        let etf_df = etf_df
            .lazy()
            .with_column(col("Date").str().to_date(StrptimeOptions {
                format: Some("%d %b %Y".into()),
                strict: false,
                exact: true,
                cache: false,
            }))
            .collect()?;
        let start_date = etf_df.clone().column("Date")?.date()?.first().unwrap();
        let end_date = etf_df.clone().column("Date")?.date()?.last().unwrap();
        let start_timestamp = start_date as i64 * 86_400;
        let end_timestamp = end_date as i64 * 86_400;
        etf_dfs.push(etf_df);
        client.close().await?;

        // To yfinance after we got the starting date
        info!("Fetch ticker {ticker}...");
        let client = yfinance::YfinanceClient::new();
        // returns historic quotes with daily interval
        let klines = client
            .get_quote_history(
                ticker,
                start_timestamp,
                end_timestamp,
                yfinance::YfinanceInterval::D1,
            )
            .await?;
        let ticker_df = klines.iter().cloned().to_dataframe().unwrap();
        let ticker_df = ticker_df
            .lazy()
            .with_column(
                (col("time") * lit(1000))
                    .cast(DataType::Datetime(TimeUnit::Milliseconds, None))
                    .alias("Date"),
            )
            .collect()?;
        ticker_dfs.push(ticker_df);
    }
    dbg!(&etf_dfs, &ticker_dfs);
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

const BTC_TICKER: &str = "BTC-USD";
const ETH_TICKER: &str = "ETH-USD";
const SOL_TICKER: &str = "SOL-USD";

const ETF_BTC_URL: &str = "https://farside.co.uk/bitcoin-etf-flow-all-data/";
const ETF_ETH_URL: &str = "https://farside.co.uk/ethereum-etf-flow-all-data/";
const ETF_SOL_URL: &str = "https://farside.co.uk/sol/";

const ETF_BTC_EXTRACT_SCRIPT: &str = r#"
const table = arguments[0];
const rows = table.rows;
const headers = [];
const jsonData = [];

// Extract headers
headers.push(...[...rows[0].cells].slice(0,-1).map(e => e.innerText));

// Extract data
for (let i = 2; i < rows.length - 4; i++) {
    const rowObject = {};
    const cells = [...rows[i].cells].slice(0,-1);
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
"#;

const ETF_ETH_EXTRACT_SCRIPT: &str = r#"
const table = arguments[0];
const rows = table.rows;
const headers = [];
const jsonData = [];

// Extract headers
headers.push("Date", ...[...rows[1].cells].slice(1,-1).map(e => e.innerText));

// Extract data
for (let i = 5; i < rows.length - 1; i++) {
    const rowObject = {};
    const cells = [...rows[i].cells].slice(0,-1);
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
"#;

const ETF_SOL_EXTRACT_SCRIPT: &str = r#"
const table = arguments[0];
const rows = table.rows;
const headers = [];
const jsonData = [];

// Extract headers
headers.push("Date", ...[...rows[1].cells].slice(1,-1).map(e => e.innerText));

// Extract data
for (let i = 5; i < rows.length - 1; i++) {
    const rowObject = {};
    const cells = [...rows[i].cells].slice(0,-1);
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
"#;

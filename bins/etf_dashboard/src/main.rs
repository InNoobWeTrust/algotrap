use algotrap::ext::yfinance;
use chrono::{TimeZone, Utc};
use core::error::Error;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
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
    let resp = tokio_test::block_on(client.get_quote_history(
        "BTC-USD",
        start,
        end,
        yfinance::YfinanceInterval::D1,
    ))?;
    println!("BTC-USD quotes: {resp:#?}");
    Ok(())
}

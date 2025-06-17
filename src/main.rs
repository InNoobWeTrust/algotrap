use algotrap::BingXClient;

#[tokio::main]
async fn main() {
    //let api_key = "";
    //let secret_key = "";

    let client = BingXClient::default();

    // Fetch 360 5-minute candles for BTC-USDT perpetual
    match client.get_futures_klines("BTC-USDT", "5m", 640).await {
        Ok(klines) => {
            println!("Fetched {} candles:", klines.len());
            for kline in klines.iter().take(640) {
                println!(
                    "Time: {}, O: {}, H: {}, L: {}, C: {}",
                    kline.time, kline.open, kline.high, kline.low, kline.close
                );
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}

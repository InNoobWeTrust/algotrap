use algotrap::BingXClient;
use cli_candlestick_chart::{Candle, Chart};

#[tokio::main]
async fn main() {
    //let api_key = "";
    //let secret_key = "";

    let client = BingXClient::default();

    // Fetch 15-minute candles for BTC-USDT perpetual
    match client.get_futures_klines("BTC-USDT", "15m", 1440).await {
        Ok(klines) => {
            println!("Fetched {} candles:", klines.len());
            let candles = klines
                .iter()
                .map(|k| Candle::new(k.open, k.high, k.low, k.close, Some(k.volume), Some(k.time)))
                .collect::<Vec<_>>();
            // Create and display the chart
            let mut chart = Chart::new(&candles);

            // Set the chart title
            chart.set_name(String::from("BTC/USDT"));

            // Set customs colors
            chart.set_bear_color(1, 205, 254);
            chart.set_bull_color(255, 107, 153);
            chart.set_vol_bull_color(1, 205, 254);
            chart.set_vol_bear_color(255, 107, 153);

            chart.set_volume_pane_height(6);
            chart.set_volume_pane_enabled(true);

            chart.draw();
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}

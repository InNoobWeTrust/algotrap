use algotrap::prelude::*;
use charming::{
    Chart, HtmlRenderer, ImageFormat, ImageRenderer,
    component::{Axis, DataZoom, DataZoomType, Grid},
    element::{
        AreaStyle, AxisPointer, AxisPointerType, DataBackground, LineStyle, SplitLine, TextStyle,
        Tooltip, Trigger,
    },
    series::Candlestick,
};
use chrono::prelude::*;
use core::error::Error;
use polars::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = ext::bingx::BingXClient::default();

    // Fetch 15-minute candles for BTC-USDT perpetual
    match client.get_futures_klines("BTC-USDT", "15m", 1440).await {
        Ok(klines) => {
            let df = klines.iter().rev().cloned().to_dataframe().unwrap();
            let df_with_indicators = df
                .lazy()
                .with_columns([
                    col("time")
                        .cast(DataType::Datetime(
                            TimeUnit::Milliseconds,
                            Some("UTC".into()),
                        ))
                        .alias("Date"),
                    ta::experimental::bias_reversion_smoothed(
                        &[col("open"), col("high"), col("low"), col("close")],
                        9,
                    )
                    .alias("Bias_Reversion"),
                    ta::rsi(&col("close"), 14).alias("RSI"),
                    ta::experimental::rssi(
                        &[col("open"), col("high"), col("low"), col("close")],
                        14,
                    )
                    .alias("RSSI"),
                ])
                .collect()
                .unwrap();
            render(&klines);
            println!("{df_with_indicators:?}");
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            Err(e)
        }
    }
}

fn render(klines: &[Kline]) {
    let chart = Chart::new()
        .tooltip(
            Tooltip::new().trigger(Trigger::Axis).axis_pointer(
                AxisPointer::new()
                    .animation(false)
                    .type_(AxisPointerType::Cross)
                    .line_style(LineStyle::new().color("#376df4").width(2).opacity(1)),
            ),
        )
        .grid(Grid::new().bottom(80))
        .data_zoom(
            DataZoom::new()
                .text_style(TextStyle::new().color("#8392A5"))
                .data_background(
                    DataBackground::new()
                        .area_style(AreaStyle::new().color("#8392A5"))
                        .line_style(LineStyle::new().color("#8392A5").opacity(0.8)),
                )
                .brush_select(true)
                .type_(DataZoomType::Inside),
        )
        .data_zoom(
            DataZoom::new()
                .text_style(TextStyle::new().color("#8392A5"))
                .data_background(
                    DataBackground::new()
                        .area_style(AreaStyle::new().color("#8392A5"))
                        .line_style(LineStyle::new().color("#8392A5").opacity(0.8)),
                )
                .brush_select(true),
        )
        .x_axis(
            Axis::new()
                //.type_(charming::element::AxisType::Time)
                .data(
                    klines
                        .iter()
                        .rev()
                        .cloned()
                        //.map(|k| format!("{}", Utc.timestamp_opt(k.time / 1000, 0).unwrap().format("%F %T")))
                        .map(|k| Utc.timestamp_opt(k.time / 1000, 0).unwrap().to_string())
                        .collect(),
                ),
        )
        .y_axis(
            Axis::new()
                .scale(true),
        )
        .series(
            Candlestick::new().data(
                klines
                    .iter()
                    .rev()
                    .cloned()
                    .map(|k| vec![k.close, k.open, k.low, k.high])
                    .collect(),
            ),
        );

    HtmlRenderer::new("BTC/USDT", 1280, 720)
        .theme(charming::theme::Theme::Walden)
        .save(&chart, "chart.html")
        .expect("Failed to save html");
    ImageRenderer::new(640, 480)
        .theme(charming::theme::Theme::Walden)
        .save_format(ImageFormat::Png, &chart, "chart.png")
        .expect("Failed to save image");
    viuer::print_from_file(
        "chart.png",
        &viuer::Config {
            ..Default::default()
        },
    )
    .expect("Image printing failed");
}

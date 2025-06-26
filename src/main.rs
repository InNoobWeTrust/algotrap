use algotrap::{prelude::*, ta::ExprMa};
use core::error::Error;
use minijinja::render;
use polars::prelude::*;
use std::io::Cursor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = ext::bingx::BingXClient::default();

    for tf in ["5m", "15m", "1h", "4h"] {
        // Fetch 15-minute candles for BTC-USDT perpetual
        match client.get_futures_klines("BTC-USDT", tf, 1440).await {
            Ok(klines) => {
                process_charts(&klines, "BingX.BTC-USDT", tf).await?;
                Ok(())
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                Err(e)
            }
        }?;
    }
    Ok(())
}

async fn process_charts(klines: &[Kline], symbol: &str, tf: &str) -> Result<(), Box<dyn Error>> {
    let df = klines.iter().rev().cloned().to_dataframe().unwrap();
    let ohlc = [col("open"), col("high"), col("low"), col("close")];
    let mut df_with_indicators = df
        .lazy()
        .with_columns([
            col("time")
                .cast(DataType::Datetime(
                    TimeUnit::Milliseconds,
                    Some("UTC".into()),
                ))
                .alias("Date"),
            col("volume").sma(20).alias("Volume SMA"),
            col("close").ema(200).alias("EMA200"),
            ta::experimental::bias_reversion_smoothed(&ohlc, 9).alias("Bias Reversion"),
            ta::experimental::atr_band(&ohlc, 42, 1.618)[0]
                .clone()
                .alias("ATR Upperband"),
            ta::experimental::atr_band(&ohlc, 42, 1.618)[1]
                .clone()
                .alias("ATR Lowerband"),
            ta::experimental::atr_reversion_percent(&ohlc, 9, 42, 1.618)
                .alias("ATR Reversion Percent"),
            ta::experimental::rssi(&ohlc, 14).alias("RSSI"),
            ta::experimental::rssi(&ohlc, 14).ema(9).alias("RSSI MA"),
            ta::bar_bias(&ohlc).rma(9).alias("Structure Power"),
            ta::bar_bias(&ohlc)
                .rma(9)
                .sma(42)
                .alias("Structure Power SMA"),
        ])
        .collect()
        .unwrap();
    println!("{df_with_indicators:?}");
    let mut file = Cursor::new(Vec::new());
    JsonWriter::new(&mut file)
        .with_json_format(JsonFormat::Json)
        .finish(&mut df_with_indicators)
        .unwrap();
    let df_json = String::from_utf8(file.into_inner()).unwrap();
    let tdv_html = render_tdv_html(df_json, format!("{symbol} {tf}"));
    tokio::fs::write(format!("tdv.{symbol}.{tf}.html"), tdv_html).await?;
    Ok(())
}

fn render_tdv_html(data: String, watermark: String) -> String {
    render!(TDV_HTML_TEMPLATE, data => data, watermark => watermark)
}

const TDV_HTML_TEMPLATE: &str = r#"
<html>
  <head>
    <meta charset="utf-8" />
    <title>ECharts</title>
    <script src="https://unpkg.com/lightweight-charts/dist/lightweight-charts.standalone.production.js"></script>
  </head>
  <body style="width: 100dvw;height:100dvh;margin: 0;">
    <div id="container" style="width: 100%;height:100%;"></div>
    <script id="data" type="application/json">
        {{ data }}
    </script>
    <script type="text/javascript">
        const data = JSON.parse(document.getElementById('data').textContent).map(d => ({
            ...d,
            time: Math.floor(d.time / 1000),
        }));
        const chart = LightweightCharts.createChart(document.getElementById('container'), {
            autoSize: true,
            layout: {
                background: { color: '#222' },
                textColor: '#DDD',
            },
            grid: {
                vertLines: { color: '#444' },
                horzLines: { color: '#444' },
            },
            timeScale: {
                timeVisible: true,
            },
        });
        const candlestickSeries = chart.addSeries(LightweightCharts.CandlestickSeries);
        candlestickSeries.setData(data);
        const volumeSeries = chart.addSeries(LightweightCharts.HistogramSeries, {
            priceFormat: {
                type: 'volume',
            },
            priceScaleId: '', // set as an overlay by setting a blank priceScaleId
        });
        volumeSeries.priceScale().applyOptions({
            // set the positioning of the volume series
            scaleMargins: {
                top: 0.8, // highest point of the series will be 80% away from the top
                bottom: 0,
            },
        });
        volumeSeries.setData(data.map(d => ({
            time: d.time,
            value: d.volume,
            color: d.close >= d.open ? '#4CAF504C' : '#F236454C'
        })));
        const volumeSmaSeries = chart.addSeries(LightweightCharts.AreaSeries, {
            lineColor: '#00000000',
            topColor: '#FDD8354C',
            bottomColor: '#FDD8352F',
            priceFormat: {
                type: 'volume',
            },
            priceScaleId: '', // set as an overlay by setting a blank priceScaleId
        });
        volumeSmaSeries.priceScale().applyOptions({
            // set the positioning of the volume series
            scaleMargins: {
                top: 0.8, // highest point of the series will be 80% away from the top
                bottom: 0,
            },
        });
        volumeSmaSeries.setData(data.map(d => ({
            time: d.time,
            value: d['Volume SMA'],
        })));
        const ema200Series = chart.addSeries(LightweightCharts.LineSeries, { color: '#9C27B080' });
        ema200Series.setData(data.map(d => ({
            time: d.time,
            value: d.EMA200,
        })));
        const biasRevSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#B2B5BE4C' });
        biasRevSeries.setData(data.map(d => ({
            time: d.time,
            value: d['Bias Reversion'],
        })));
        const atrUpperBandSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#4CAF504C' });
        atrUpperBandSeries.setData(data.map(d => ({
            time: d.time,
            value: d['ATR Upperband'],
        })));
        const atrLowerBandSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#F236454C' });
        atrLowerBandSeries.setData(data.map(d => ({
            time: d.time,
            value: d['ATR Lowerband'],
        })));
        const structurePwrSeries = chart.addSeries(LightweightCharts.HistogramSeries, {}, 1);
        structurePwrSeries.setData(data.map(d => ({
            time: d.time,
            value: d['Structure Power'],
            color: d['Structure Power'] >= 0 ? '#00897B80' : '#880E4F80'
        })));
        const structurePwrSmaSeries = chart.addSeries(LightweightCharts.BaselineSeries, {
            baseValue: { type: 'price', price: 0 },
            topLineColor: '#00897B00',
            topFillColor1: '#00897B4C',
            topFillColor2: '#00897B80',
            bottomLineColor: '#880E4F00',
            bottomFillColor1: '#880E4F4C',
            bottomFillColor2: '#880E4F80',
        }, 1);
        structurePwrSmaSeries.setData(data.map(d => ({
            time: d.time,
            value: d['Structure Power SMA'],
        })));
        const rssiSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 2);
        rssiSeries.setData(data.map(d => ({
            time: d.time,
            value: d.RSSI,
            color: d.RSSI > 59 ? '#4CAF5080' : d.RSSI < 41 ? '#F2364580': '#2962FF4C'
        })));
        const rssiMaSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 2);
        rssiMaSeries.setData(data.map(d => ({
            time: d.time,
            value: d['RSSI MA'],
            color: '#FDD83580'
        })));
        const atrRevSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 3);
        atrRevSeries.setData(data.map(d => ({
            time: d.time,
            value: d['ATR Reversion Percent'],
            color: d['ATR Reversion Percent'] > 99 ? '#4CAF5080' : d['ATR Reversion Percent'] < -99 ? '#F2364580': '#2962FF4C'
        })));
        const reversionUp = (d) => (d.RSSI < 46 && d['ATR Reversion Percent'] > 99);
        const reversionDown = (d) => (d.RSSI > 54 && d['ATR Reversion Percent'] < -99);
        const markers = data.filter(d => reversionUp(d) || reversionDown(d)).map(d => ({
            time: d.time,
            position: reversionUp(d) ? 'belowBar' : 'aboveBar',
            color: reversionUp(d) ? '#2196F3' : '#e91e63',
            shape: reversionUp(d) ? 'arrowUp' : 'arrowDown',
        }))
        LightweightCharts.createSeriesMarkers(candlestickSeries, markers);
        LightweightCharts.createTextWatermark(chart.panes()[0], {
            horzAlign: 'left',
            vertAlign: 'top',
            lines: [
                {
                    text: '{{ watermark }}',
                    color: 'rgba(178, 181, 190, 0.5)',
                    fontSize: 24,
                },
            ],
        });
        LightweightCharts.createTextWatermark(chart.panes()[1], {
            horzAlign: 'left',
            vertAlign: 'top',
            lines: [
                {
                    text: 'Structure Power',
                    color: 'rgba(178, 181, 190, 0.5)',
                    fontSize: 18,
                },
            ],
        });
        LightweightCharts.createTextWatermark(chart.panes()[2], {
            horzAlign: 'left',
            vertAlign: 'top',
            lines: [
                {
                    text: 'RSSI',
                    color: 'rgba(178, 181, 190, 0.5)',
                    fontSize: 18,
                },
            ],
        });
        LightweightCharts.createTextWatermark(chart.panes()[3], {
            horzAlign: 'left',
            vertAlign: 'top',
            lines: [
                {
                    text: 'ATR Reversion',
                    color: 'rgba(178, 181, 190, 0.5)',
                    fontSize: 18,
                },
            ],
        });
        chart.timeScale().setVisibleLogicalRange({ from: data.length - 147, to: data.length });
        const containerHeight = document.getElementById("container").getClientRects()[0].height;
        chart.panes()[0].setHeight(Math.floor(containerHeight * 0.60));
    </script>
  </body>
</html>
"#;

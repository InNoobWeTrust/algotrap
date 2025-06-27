use algotrap::{prelude::*, ta::ExprMa};
use core::error::Error;
use futures::future::join_all;
use minijinja::render;
use polars::prelude::*;
use std::collections::HashMap;
use std::io::Cursor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = ext::bingx::BingXClient::default();

    let tfs = ["1m", "5m", "15m", "1h", "4h"];
    let all_dfs = join_all(tfs.iter().map(async |tf| {
        // Fetch 15-minute candles for BTC-USDT perpetual
        client
            .get_futures_klines("BTC-USDT", tf, 1440)
            .await
            .map(|k| (tf.to_string(), k))
    }))
    .await
    .into_iter()
    .filter_map(|res| match res {
        Ok((tf, klines)) => Some((tf, process_data(&klines).1)),
        Err(err) => {
            eprintln!("Error: {:?}", err);
            None
        }
    })
    .collect::<HashMap<_, _>>();
    let df_json = serde_json::to_string(&all_dfs)?;
    let tfs_json = serde_json::to_string(&tfs)?;
    process_chart(&df_json, "BingX.BTC-USDT", &tfs_json, tfs[1]).await?;
    Ok(())
}

fn process_data(klines: &[Kline]) -> (DataFrame, String) {
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
    (df_with_indicators, df_json)
}

async fn process_chart(
    dataset: &str,
    symbol: &str,
    tfs: &str,
    default_tf: &str,
) -> Result<(), Box<dyn Error>> {
    let tdv_html = render_tdv_html(&dataset, symbol, tfs, default_tf);
    tokio::fs::write(format!("tdv.{symbol}.html"), tdv_html).await?;
    Ok(())
}

fn render_tdv_html(dataset: &str, symbol: &str, tfs: &str, default_tf: &str) -> String {
    render!(TDV_HTML_TEMPLATE, dataset => dataset, symbol => symbol, tfs => tfs, default_tf => default_tf)
        .trim()
        .to_string()
}

const TDV_HTML_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html class="sl-theme-dark">
  <head>
    <meta charset="utf-8" />
    <title>ECharts</title>
    <script src="https://unpkg.com/lightweight-charts/dist/lightweight-charts.standalone.production.js"></script>
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/@shoelace-style/shoelace@2.20.1/cdn/themes/dark.css" />
    <script type="module" src="https://cdn.jsdelivr.net/npm/@shoelace-style/shoelace@2.20.1/cdn/shoelace-autoloader.js"></script>
    <style>
        html, body {
            height: 100%;
            margin: 0;
            padding: 0;
        }

        body {
            min-height: 100%;
            box-sizing: border-box;
            background: #222;
        }

        #container {
            height: 100%;
            background: lightblue;
        }

        #tf_btns {
            position: absolute;
            top: 5%;
            left: 2%;
            z-index: 9999;
        }
    </style>
  </head>
  <body>
    <div id="container" data-tf="{{ default_tf }}"></div>
    <sl-radio-group id="tf_btns">
    </sl-radio-group>
    <script id="dataset" type="application/json">
        {{ dataset }}
    </script>
    <script id="tfs" type="application/json">
        {{ tfs }}
    </script>
    <script type="text/javascript">
        const dataset = Object.fromEntries(
            Object.entries(
                JSON.parse(document.getElementById('dataset').textContent)
            ).map(([key, value]) => [key, JSON.parse(value)])
        );
        const tfs = JSON.parse(document.getElementById('tfs').textContent);
        const tf_btns = document.getElementById('tf_btns');
        const container = document.getElementById('container');
        const chart = LightweightCharts.createChart(container, {
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
        const ema200Series = chart.addSeries(LightweightCharts.LineSeries, { color: '#9C27B080' });
        const biasRevSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#B2B5BE4C' });
        const atrUpperBandSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#4CAF504C' });
        const atrLowerBandSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#F236454C' });
        const structurePwrSeries = chart.addSeries(LightweightCharts.HistogramSeries, {}, 1);
        const structurePwrSmaSeries = chart.addSeries(LightweightCharts.BaselineSeries, {
            baseValue: { type: 'price', price: 0 },
            topLineColor: 'rgba(76, 175, 80, 0.1)',
            topFillColor1: 'rgba(76, 175, 80, 0.2)',
            topFillColor2: 'rgba(76, 175, 80, 0.5)',
            bottomLineColor: 'rgba(242, 54, 69, 0.1)',
            bottomFillColor1: 'rgba(242, 54, 69, 0.2)',
            bottomFillColor2: 'rgba(242, 54, 69, 0.5)',
        }, 1);
        const rssiSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 2);
        const rssiMaSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 2);
        const atrRevSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 3);
        const markersSeries = LightweightCharts.createSeriesMarkers(candlestickSeries, []);
        const textWatermarks = [
            LightweightCharts.createTextWatermark(chart.panes()[0], {}),
            LightweightCharts.createTextWatermark(chart.panes()[1], {}),
            LightweightCharts.createTextWatermark(chart.panes()[2], {}),
            LightweightCharts.createTextWatermark(chart.panes()[3], {}),
        ];

        const onIntervalUpdate = (tf) => { 
            const data = dataset[tf].map(d => ({
                ...d,
                time: Math.floor(d.time / 1000),
            }));
            candlestickSeries.setData(data);
            volumeSeries.setData(data.map(d => ({
                time: d.time,
                value: d.volume,
                color: d.close >= d.open ? 'rgba(76, 175, 80, 0.3)' : 'rgba(242, 54, 69, 0.3)'
            })));
            volumeSmaSeries.setData(data.map(d => ({
                time: d.time,
                value: d['Volume SMA'],
            })));
            ema200Series.setData(data.map(d => ({
                time: d.time,
                value: d.EMA200,
            })));
            biasRevSeries.setData(data.map(d => ({
                time: d.time,
                value: d['Bias Reversion'],
            })));
            atrUpperBandSeries.setData(data.map(d => ({
                time: d.time,
                value: d['ATR Upperband'],
            })));
            atrLowerBandSeries.setData(data.map(d => ({
                time: d.time,
                value: d['ATR Lowerband'],
            })));
            structurePwrSeries.setData(data.map(d => ({
                time: d.time,
                value: d['Structure Power'],
                color: d['Structure Power'] >= 0 ? '#00897B80' : '#880E4F80'
            })));
            structurePwrSmaSeries.setData(data.map(d => ({
                time: d.time,
                value: d['Structure Power SMA'],
            })));
            rssiSeries.setData(data.map(d => ({
                time: d.time,
                value: d.RSSI,
                color: d.RSSI > 59 ? '#4CAF5080' : d.RSSI < 41 ? '#F2364580': '#2962FF4C'
            })));
            rssiMaSeries.setData(data.map(d => ({
                time: d.time,
                value: d['RSSI MA'],
                color: '#FDD83580'
            })));
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
            markersSeries.setMarkers(markers);
        }
        const watermarkUpdate = () => {
            const watermarks = [
                {
                    horzAlign: 'left',
                    vertAlign: 'top',
                    lines: [
                        {
                            text: `{{ symbol }} ${tf_btns.value || container.dataset.tf || tfs[0]}`,
                            color: 'rgba(178, 181, 190, 0.5)',
                            fontSize: 24,
                        },
                    ],
                },
                {
                    horzAlign: 'left',
                    vertAlign: 'top',
                    lines: [
                        {
                            text: 'Structure Power',
                            color: 'rgba(178, 181, 190, 0.5)',
                            fontSize: 18,
                        },
                    ],
                },
                {
                    horzAlign: 'left',
                    vertAlign: 'top',
                    lines: [
                        {
                            text: 'RSSI',
                            color: 'rgba(178, 181, 190, 0.5)',
                            fontSize: 18,
                        },
                    ],
                },
                {
                    horzAlign: 'left',
                    vertAlign: 'top',
                    lines: [
                        {
                            text: 'ATR Reversion',
                            color: 'rgba(178, 181, 190, 0.5)',
                            fontSize: 18,
                        },
                    ],
                },
            ];
            Object.entries(textWatermarks).forEach(([k,v]) => {
                v.applyOptions(watermarks[k]);
            });
        }
        const onSizeUpdate = () => {
            const tmpSeries = chart.panes()[0].getSeries()[0];
            const len = tmpSeries.data().length;
            chart.timeScale().setVisibleLogicalRange({ from: len - 144, to: len + 5 });
            const containerHeight = document.getElementById("container").getClientRects()[0].height;
            chart.panes()[0].setHeight(Math.floor(containerHeight * 0.60));
            watermarkUpdate();
        }
        const resizeObserver = new ResizeObserver((entries) => {
            requestAnimationFrame(() => {
                onSizeUpdate();
            });
        });
        resizeObserver.observe(container);
        tfs.forEach(tf => {
            const tf_btn = document.createElement('sl-radio-button');
            tf_btn.innerText = tf;
            tf_btn.value = tf
            tf_btn.addEventListener('click', () => {
                onIntervalUpdate(tf);
                watermarkUpdate();
            });
            tf_btns.appendChild(tf_btn);
        });
        // Click default timeframe
        requestAnimationFrame(() => {
            [...tf_btns.children].find(b => b.textContent == container.dataset.tf)?.click();
        })
        onSizeUpdate();
    </script>
  </body>
</html>
"#;

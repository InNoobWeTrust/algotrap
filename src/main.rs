use algotrap::prelude::*;
use algotrap::ta::experimental::OhlcExperimental;
use algotrap::ta::prelude::*;
use serde_json::Value;
use core::error::Error;
use futures::future::join_all;
use minijinja::render;
use polars::prelude::*;
use std::collections::HashMap;
use std::io::Cursor;

const SYMBOL: &str = "BTC-USDT";
const SL_PERCENT: f64 = 0.1;
const TOL_PERCENT: f64 = 0.618;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = ext::bingx::BingXClient::default();

    let tfs = ["1m", "5m", "15m", "1h", "4h"];
    let all_dfs = join_all(tfs.iter().map(async |tf| {
        // Fetch 15-minute candles for BTC-USDT perpetual
        client
            .get_futures_klines(SYMBOL, tf, 1440)
            .await
            .map(|k| (tf.to_string(), k))
    }))
    .await
    .into_iter()
    .filter_map(|res| match res {
        Ok((tf, klines)) => Some((tf, process_data(&klines).expect("Failed to process data").1)),
        Err(err) => {
            eprintln!("Error: {:?}", err);
            None
        }
    })
    .collect::<HashMap<_, _>>();
    let df_json = serde_json::to_string(&all_dfs)?;
    let tfs_json = serde_json::to_string(&tfs)?;
    let html_vars = TdvHtmlVars {
        dataset: df_json,
        symbol: "BingX:BTC-USDT".to_string(),
        tfs: tfs_json,
        default_tf: "5m".to_string(),
        sl_percent: format!("{:.0}", SL_PERCENT * 100.),
        tol_percent: format!("{:.2}", TOL_PERCENT * 100.),
    };
    let tdv_html = render_tdv_html(&html_vars);
    tokio::fs::write(format!("tdv.BingX.{SYMBOL}.html"), tdv_html).await?;
    Ok(())
}

fn process_data(klines: &[Kline]) -> Result<(DataFrame, Value), Box<dyn Error>> {
    let df = klines.iter().rev().cloned().to_dataframe().unwrap();
    let ohlc: ta::Ohlc = [col("open"), col("high"), col("low"), col("close")];
    let lvrg_adjust = SL_PERCENT / (1. + TOL_PERCENT);
    let mut df_with_indicators = df
        .lazy()
        .with_columns([
            col("time")
                .cast(DataType::Datetime(
                    TimeUnit::Milliseconds,
                    Some("UTC".into()),
                ))
                .alias("Date"),
            col("volume").ema(20).alias("Volume SMA"),
            ohlc.bias_reversion_smoothed(9).alias("Bias Reversion"),
            ohlc.atr(42).alias("ATR"),
            ohlc.rssi(14).alias("RSSI"),
            ohlc.bar_bias().rma(9).alias("Structure Power"),
        ])
        .with_columns([
            (col("ATR") * lit(1.618)).alias("ATR Oscillation"),
            (col("ATR") / ohlc[0].clone()).alias("ATR Percent"), // For calculating leverage
        ])
        .with_columns([
            (ohlc[0].clone() + col("ATR Oscillation")).alias("ATR Upperband"),
            (ohlc[0].clone() - col("ATR Oscillation")).alias("ATR Lowerband"),
            ohlc.band_reversion_percent(&col("ATR Oscillation"), &col("Bias Reversion"))
                .alias("ATR Reversion Percent"),
            col("RSSI").ema(9).alias("RSSI MA"),
            col("Structure Power").sma(16).alias("Structure Power SMA"),
            (lit(lvrg_adjust) * ohlc[0].clone() / col("ATR")).alias("Leverage"),
        ])
        .collect()
        .unwrap();
    println!("{df_with_indicators:?}");
    let mut file = Cursor::new(Vec::new());
    JsonWriter::new(&mut file)
        .with_json_format(JsonFormat::Json)
        .finish(&mut df_with_indicators)
        .unwrap();
    //let df_json = String::from_utf8(file.into_inner()).unwrap();
    let df_json = serde_json::from_slice(&file.into_inner())?;
    Ok((df_with_indicators, df_json))
}

struct TdvHtmlVars {
    dataset: String,
    symbol: String,
    tfs: String,
    default_tf: String,
    sl_percent: String,  // formatted, max 2 decimals
    tol_percent: String, // formatted, max 2 decimals
}

fn render_tdv_html(vars: &TdvHtmlVars) -> String {
    render!(TDV_HTML_TEMPLATE, dataset => vars.dataset, symbol => vars.symbol, tfs => vars.tfs, default_tf => vars.default_tf, sl_percent => vars.sl_percent, tol_percent => vars.tol_percent)
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

        #overlay {
            position: absolute;
            top: 5%;
            left: 2%;
            z-index: 9999;
        }
        #tf-btns {
            display: inline-block;
        }
        #fullscreen-btn {
            position: absolute;
            bottom: 1%;
            right: 1%;
            z-index: 9999;
        }
    </style>
  </head>
  <body>
    <div id="container" data-tf="{{ default_tf }}"></div>
    <div id="overlay">
        <div id="badges">
            <sl-badge variant="danger" pill>SL: {{ sl_percent }}%</sl-badge>
            <sl-badge variant="success" pill>Tol: {{ tol_percent }}%</sl-badge>
            <sl-badge id="atr-percent" variant="warning" pill>ATR: -</sl-badge>
            <sl-badge id="leverage" variant="primary" pill>Lvrg: -</sl-badge>
        </div>
        <sl-divider style="--spacing: 0.25rem;"></sl-divider>
        <sl-radio-group id="tf-btns"></sl-radio-group>
    </div>
    <sl-icon-button
      id="fullscreen-btn"
      name="fullscreen"
      label="Toggle Fullscreen"
      style="font-size: 2rem;"
      onclick="toggleFullscreen()">
    </sl-icon-button>
    <script>
      const fullscreenButton = document.getElementById('fullscreen-btn');

      // Function to request or exit fullscreen
      function toggleFullscreen() {
        if (!document.fullscreenElement) {
          const elem = document.documentElement;
          elem.requestFullscreen?.();
          elem.webkitRequestFullscreen?.();
          elem.msRequestFullscreen?.();
        } else {
          document.exitFullscreen?.();
          document.webkitExitFullscreen?.();
          document.msExitFullscreen?.();
        }
      }

      // Listen for changes in fullscreen state to update the icon
      document.addEventListener('fullscreenchange', () => {
        if (document.fullscreenElement) {
          fullscreenButton.name = 'fullscreen-exit';
        } else {
          fullscreenButton.name = 'fullscreen';
        }
      });
    </script>
    <script id="dataset" type="application/json">
        {{ dataset }}
    </script>
    <script id="tfs" type="application/json">
        {{ tfs }}
    </script>
    <script type="text/javascript">
        const dataset = JSON.parse(document.getElementById('dataset').textContent);
        const tfs = JSON.parse(document.getElementById('tfs').textContent);
        const tf_btns = document.getElementById('tf-btns');
        const container = document.getElementById('container');
        const atr_badge = document.getElementById('atr-percent');
        const lvrg_badge = document.getElementById('leverage');

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

        const watermarkUpdate = () => {
            const tf = tf_btns.value || container.dataset.tf || tfs[0];
            const atr = +(dataset[tf].slice(-1)[0]["ATR Percent"] * 100).toFixed(2);
            const lvrg = Math.floor(dataset[tf].slice(-1)[0]["Leverage"]);
            atr_badge.innerHTML = `ATR: ${atr}%`;
            lvrg_badge.innerHTML = `x${lvrg}`;
            const watermarks = [
                {
                    horzAlign: 'left',
                    vertAlign: 'top',
                    lines: [
                        {
                            text: `{{ symbol }} ${tf}`,
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
            watermarkUpdate();
        }
        const onSizeUpdate = () => {
            const tmpSeries = chart.panes()[0].getSeries()[0];
            const len = tmpSeries.data().length;
            chart.timeScale().setVisibleLogicalRange({ from: len - 128, to: len + 5 });
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
                requestAnimationFrame(() => {
                    onIntervalUpdate(tf);
                });
            });
            tf_btns.appendChild(tf_btn);
        });
        // Click default timeframe
        requestAnimationFrame(() => {
            [...tf_btns.children].find(b => b.textContent == container.dataset.tf)?.click();
        })
    </script>
  </body>
</html>
"#;

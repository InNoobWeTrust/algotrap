use algotrap::ext::ntfy;
use algotrap::prelude::*;
use algotrap::ta::experimental::OhlcExperimental;
use algotrap::ta::prelude::*;
use core::error::Error;
use dotenv::dotenv;
use futures::future::join_all;
use minijinja::render;
use polars::prelude::*;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

const SYMBOL: &str = "BTC-USDT";
const SL_PERCENT: f64 = 0.1;
const TOL_PERCENT: f64 = 0.618;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Load dotenv
    dotenv().ok();

    let client = ext::bingx::BingXClient::default();

    let tfs = ["1m", "5m", "15m", "1h", "4h", "1d", "1w", "1M"];
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
        Ok((tf, klines)) => {
            let df = process_data(&klines).expect("Failed to process data");
            Some((tf, df))
        }
        Err(err) => {
            eprintln!("Error: {err:?}");
            None
        }
    })
    .collect::<HashMap<String, DataFrame>>();
    let all_dfs_serialized: HashMap<String, Value> = all_dfs
        .iter()
        .map(|(tf, df)| {
            let df_json =
                df_to_json(&mut df.clone()).expect("Failed to serialize data frame to json");
            (tf.to_string(), df_json)
        })
        .collect();
    let df_json = serde_json::to_string(&all_dfs_serialized)?;
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
    notify(&all_dfs).await?;
    Ok(())
}

async fn notify(all_dfs: &HashMap<String, DataFrame>) -> Result<(), Box<dyn Error>> {
    let signals: HashMap<String, i32> = all_dfs
        .iter()
        .map(|(tf, df)| {
            let second_last_row = df.slice(-2, 1);
            let signal: i32 = second_last_row
                .column("Climax Signal")
                .expect("Failed to get signal column")
                .get(0)
                .expect("Cannot get signal from last confirmed candle")
                .extract::<i32>()
                .unwrap();
            (tf.to_string(), signal)
        })
        .collect();
    let excluded_tfs: HashSet<_> = [
        "1m".to_string(),
        "5m".to_string(),
        "1d".to_string(),
        "1w".to_string(),
        "1M".to_string(),
    ]
    .into_iter()
    .collect();
    let need_notify = signals
        .into_iter()
        .filter(|(tf, _signal)| !excluded_tfs.contains(tf))
        .all(|(_tf, signal)| signal != 0);

    let force_noti = std::env::var("NTFY_ALWAYS");
    let force_noti = force_noti.ok().is_some_and(|s| !s.is_empty());
    if force_noti || need_notify {
        let records_serialized: HashMap<String, Value> = all_dfs
            .iter()
            .map(|(tf, df)| {
                let df = df
                    .clone()
                    .lazy()
                    .select([col("RSSI"), col("ATR Reversion Percent")])
                    .collect()
                    .expect("Failed to extract columns");
                let df_json = df_to_json(&mut df.slice(-2, 1))
                    .expect("Failed to serialize data frame to json");
                (tf.to_string(), df_json)
            })
            .collect();
        let records_json = serde_json::to_string(&records_serialized)?;

        let topic = std::env::var("NTFY_TOPIC")?;
        let pages_pj_name = std::env::var("CLOUDFLARE_PAGES_PROJECT_NAME")?;
        let action_url = format!("https://{pages_pj_name}.pages.dev/");
        ntfy::send_ntfy_notification(
            &topic,
            &format!("Last stats:\n{records_json}"),
            Some("Abnormal price movements"),
            Some("5"),
            None,
            Some(&action_url),
            Some("Open chart"),
        )
        .await?;
    }
    Ok(())
}

fn indicators() -> Vec<Expr> {
    let ohlc: ta::Ohlc = [col("open"), col("high"), col("low"), col("close")];

    // Axis conversion
    let time_to_date = col("time")
        .cast(DataType::Datetime(
            TimeUnit::Milliseconds,
            Some("UTC".into()),
        ))
        .alias("Date");

    // Volume
    let vol_sma = col("volume").ema(20).alias("Volume SMA");

    // Moving thresholds
    let bias_rev = ohlc.bias_reversion_smoothed(9).alias("Bias Reversion");
    let ema200 = col("close").ema(200).alias("EMA200");
    let bullish_revrsi = col("high").rev_rsi(14, 70.).alias("Bullish RevRSI");
    let bearish_revrsi = col("low").rev_rsi(14, 30.).alias("Bearish RevRSI");

    // Oscillation band
    let atr = ohlc.atr(42).alias("ATR");
    let atr_osc = (atr.clone() * lit(1.618)).alias("ATR Oscillation");
    let atr_upperband = (col("open") + atr_osc.clone()).alias("ATR Upperband");
    let atr_lowerband = (col("open") - atr_osc.clone()).alias("ATR Lowerband");
    let atr_percent = (atr.clone() / col("open")).alias("ATR Percent");

    // Relative structure power
    let structure_pwr = ohlc.bar_bias().rma(9).alias("Structure Power");
    let structure_pwr_sma = structure_pwr.clone().sma(16).alias("Structure Power SMA");

    // Relative structure strength index
    let rssi = ohlc.rssi(14).alias("RSSI");
    let rssi_ma = rssi.clone().ema(9).alias("RSSI MA");

    // Stability indicator
    let atr_rev_percent = ohlc
        .band_reversion_percent(&atr_osc.clone(), &bias_rev.clone())
        .alias("ATR Reversion Percent");

    // Signals
    let overbought = rssi
        .clone()
        .gt(lit(54))
        .logical_and(atr_rev_percent.clone().lt(lit(-50)))
        .alias("Overbought");
    let oversold = rssi
        .clone()
        .lt(lit(46))
        .logical_and(atr_rev_percent.clone().gt(lit(50)))
        .alias("Oversold");
    let climax_signal = when(overbought.clone().not().logical_and(oversold.clone().not()))
        .then(lit(0))
        .otherwise(when(overbought).then(lit(1)).otherwise(lit(-1)))
        .alias("Climax Signal");

    // Miscs
    let lvrg_adjust = SL_PERCENT / (1. + TOL_PERCENT);
    let lvrg = (lit(lvrg_adjust) * ohlc[0].clone() / atr.clone()).alias("Leverage");

    // Selected columns to export
    vec![
        time_to_date,
        vol_sma,
        bias_rev,
        ema200,
        bullish_revrsi,
        bearish_revrsi,
        atr_upperband,
        atr_lowerband,
        rssi,
        rssi_ma,
        structure_pwr,
        structure_pwr_sma,
        atr_percent,
        atr_rev_percent,
        lvrg,
        climax_signal,
    ]
}

fn process_data(klines: &[Kline]) -> Result<DataFrame, Box<dyn Error>> {
    let df = klines.iter().rev().cloned().to_dataframe().unwrap();
    let df_with_indicators = df.lazy().with_columns(indicators()).collect().unwrap();
    println!("{df_with_indicators:?}");
    Ok(df_with_indicators)
}

fn df_to_json(df: &mut DataFrame) -> Result<Value, Box<dyn Error>> {
    let mut file = Cursor::new(Vec::new());
    JsonWriter::new(&mut file)
        .with_json_format(JsonFormat::Json)
        .finish(df)
        .unwrap();
    //let df_json = String::from_utf8(file.into_inner()).unwrap();
    let df_json = serde_json::from_slice(&file.into_inner())?;
    Ok(df_json)
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
    <title>BTC-USDT (InNoobWeTrust™)</title>
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
        const bullishBandSeries = chart.addSeries(LightweightCharts.LineSeries, { color: 'rgba(33,150,243,0.2)', lineWidth: 6 });
        const bearishBandSeries = chart.addSeries(LightweightCharts.LineSeries, { color: 'rgba(255,152,0,0.2)', lineWidth: 6 });
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
            bullishBandSeries.setData(data.map(d => ({
                time: d.time,
                value: d['Bullish RevRSI'],
            })));
            bearishBandSeries.setData(data.map(d => ({
                time: d.time,
                value: d['Bearish RevRSI'],
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
                color: d['ATR Reversion Percent'] > 50 ? '#4CAF5080' : d['ATR Reversion Percent'] < -50 ? '#F2364580': '#2962FF4C'
            })));
            const markers = data.filter(d => d['Climax Signal'] != 0).map(d => ({
                time: d.time,
                position: d['Climax Signal'] < 0 ? 'belowBar' : 'aboveBar',
                color: d['Climax Signal'] < 0 ? '#2196F3' : '#e91e63',
                shape: d['Climax Signal'] < 0 ? 'arrowUp' : 'arrowDown',
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

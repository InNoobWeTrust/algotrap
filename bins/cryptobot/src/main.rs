use algotrap::df_utils::JsonDataframe;
use chrono::Utc;
use core::error::Error;
use core::time::Duration;
use dotenv::dotenv;
use futures::future::join_all;
use minijinja::render;
use polars::prelude::*;
use rayon::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use algotrap::ext::bingx::MAX_LIMIT;
use algotrap::ext::ntfy;
use algotrap::prelude::*;
use algotrap::ta::experimental::OhlcExperimental;
use algotrap::ta::prelude::*;
use algotrap::time_utils::is_closing_timeframe;

#[derive(Debug, Clone, Deserialize)]
struct EnvConf {
    symbol: String,
    sl_percent: f64,
    tol_percent: f64,
    tfs: Vec<Timeframe>,
    default_tf: Timeframe,
    cloudflare_pages_project_name: String,
    ntfy_topic: String,
    ntfy_tf_exclusion: Vec<Timeframe>,
    ntfy_always: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Load dotenv
    dotenv().ok();

    // Load env config
    let conf: EnvConf = envy::from_env()?;

    let client = ext::bingx::BingXClient::default();

    let all_dfs = join_all(
        conf.tfs
            .iter()
            .map(|tf| {
                let client = &client;
                let symbol = conf.symbol.clone();
                async move {
                    // Fetch 15-minute candles for symbol's perpetual
                    client
                        .get_futures_klines(&symbol, &tf.to_string(), MAX_LIMIT)
                        .await
                        .map(|k| (*tf, k))
                }
            })
            .collect::<Vec<_>>(),
    )
    .await
    .into_par_iter()
    .filter_map(|res| match res {
        Ok((tf, klines)) => {
            let df = process_data(klines.as_slice(), &conf).expect("Failed to process data");
            Some((tf, df))
        }
        Err(err) => {
            eprintln!("Error: {err:#?}");
            None
        }
    })
    .collect::<HashMap<Timeframe, DataFrame>>();
    let all_dfs_serialized: HashMap<String, Value> = all_dfs
        .par_iter()
        .map(|(tf, df)| {
            let df_json: JsonDataframe = df
                .try_into()
                .expect("Failed to serialize data frame to json");
            let df_json: Value = df_json.into();
            (tf.to_string(), df_json)
        })
        .collect();
    let df_json = serde_json::to_string(&all_dfs_serialized)?;
    let tfs_json = serde_json::to_string(&conf.tfs)?;
    let html_vars = TdvHtmlVars {
        dataset: df_json,
        symbol: format!("BingX:{}", conf.symbol),
        tfs: tfs_json,
        default_tf: conf.default_tf.to_string(),
        sl_percent: format!("{:.0}", conf.sl_percent * 100.),
        tol_percent: format!("{:.2}", conf.tol_percent * 100.),
    };
    let tdv_html = render_tdv_html(&html_vars);
    tokio::fs::write(format!("tdv.BingX.{}.html", conf.symbol), tdv_html).await?;
    notify(&all_dfs, &conf).await?;
    Ok(())
}

async fn notify(
    all_dfs: &HashMap<Timeframe, DataFrame>,
    conf: &EnvConf,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let excluded_tfs: HashSet<_> = conf.ntfy_tf_exclusion.iter().cloned().collect();
    let signals: HashMap<Timeframe, i32> = all_dfs
        .par_iter()
        .filter(|(tf, _df)| !excluded_tfs.contains(tf))
        .map(|(tf, df)| {
            let second_last_row = df.slice(-2, 1);
            let signal: i32 = second_last_row
                .column("climax_signal")
                .expect("Failed to get signal column")
                .get(0)
                .expect("Cannot get signal from last confirmed candle")
                .extract::<i32>()
                .unwrap();
            // fake value to debug
            let signal = if conf.ntfy_always && signal == 0 {
                1
            } else {
                signal
            };
            (*tf, signal)
        })
        .collect();
    let effective_signals: HashMap<Timeframe, i32> = signals
        .clone()
        .into_par_iter()
        .filter(|(tf, signal)| {
            *signal != 0
                && (conf.ntfy_always
                    || is_closing_timeframe(tf, Utc::now(), Some(Duration::from_secs(10)))
                        .unwrap_or(false))
        })
        .collect();
    let effective_tfs: Vec<Timeframe> = effective_signals
        .clone()
        .into_par_iter()
        .map(|(tf, _)| tf)
        .collect();
    let total_weight: usize = signals.par_iter().map(|(tf, _)| tf.weight()).sum();
    let effective_weight: usize = effective_signals
        .par_iter()
        .map(|(tf, _)| tf.weight())
        .sum();
    let need_notify = effective_weight > 0;

    dbg!(signals, effective_signals, need_notify, conf.ntfy_always);
    if conf.ntfy_always || need_notify {
        let records_serialized: HashMap<String, Value> = all_dfs
            .par_iter()
            .filter(|(tf, _df)| effective_tfs.contains(tf))
            .map(|(tf, df)| {
                let df = df
                    .clone()
                    .lazy()
                    .select([col("rssi"), col("atr_reversion_percent")])
                    .collect()
                    .expect("Failed to extract columns");
                let df_json: JsonDataframe = df
                    .slice(-2, 1)
                    .try_into()
                    .expect("Failed to serialize data frame to json");
                let df_json: Value = df_json.into();
                (tf.to_string(), df_json)
            })
            .collect();
        dbg!(&records_serialized);
        let records_json = serde_json::to_string(&records_serialized)?;

        let action_url = format!("https://{}.pages.dev/", conf.cloudflare_pages_project_name);
        ntfy::NtfyMessage::default()
            .topic(&conf.ntfy_topic)
            .title(&format!("{} notable movements", &conf.symbol))
            .message_template(
                r#"
Last stats:
{{ range $tf, $obj := . }}
{{$tf}}:{{range .}}{{range $k, $v := .}}
- {{$k}}: {{$v}}{{end}}{{end}}
{{ end }}
            "#
                .trim(),
            )
            .message(&records_json)
            .priority((effective_weight as f64 / total_weight as f64 * 4.).floor() as u8 + 1)
            .tags(vec![conf.symbol.to_string()])
            .actions(vec![vec![
                "view".to_string(),
                "Open chart".to_string(),
                action_url,
            ]])
            .send()
            .await?;
    }
    Ok(())
}

fn indicators(conf: &EnvConf) -> Vec<Expr> {
    let ohlc: ta::Ohlc = [col("open"), col("high"), col("low"), col("close")];

    // Axis conversion
    let time_to_date = col("time")
        .cast(DataType::Datetime(
            TimeUnit::Milliseconds,
            Some(TimeZone::UTC),
        ))
        .alias("Date");

    // Volume
    let vol_color = when(col("close").gt_eq(col("open")))
        .then(lit("rgba(76, 175, 80, 0.3)"))
        .otherwise(lit("rgba(242, 54, 69, 0.3)"))
        .alias("volume_color");
    let vol_sma = col("volume").ema(20).alias("volume_sma");

    // Moving thresholds
    let bias_rev = ohlc.bias_reversion_smoothed(9).alias("bias_reversion");
    let bias_rev_color = lit("rgba(178, 181, 190, 0.2)").alias("bias_reversion_color");
    let ema200 = col("close").ema(200).alias("ema200");
    let ema200_color = lit("rgba(156, 39, 176, 0.5)").alias("ema200_color");
    let neutral_revrsi = (col("open") + ohlc.bar_bias())
        .rev_rsi(14, 50.)
        .alias("neutral_revrsi");
    let neutral_revrsi_color = lit("rgba(178,181,190,0.2)").alias("neutral_revrsi_color");
    let bullish_revrsi = col("high").rev_rsi(14, 70.).alias("bullish_revrsi");
    let bullish_revrsi_color = lit("rgba(33,150,243,0.2)").alias("bullish_revrsi_color");
    let bearish_revrsi = col("low").rev_rsi(14, 30.).alias("bearish_revrsi");
    let bearish_revrsi_color = lit("rgba(255,152,0,0.2)").alias("bearish_revrsi_color");

    // Oscillation band
    let atr = ohlc.atr(42).alias("ATR");
    let atr_osc = (atr.clone() * lit(1.618)).alias("atr_oscillation");
    let atr_upperband = (col("open") + atr_osc.clone()).alias("atr_upperband");
    let atr_upperband_color = lit("rgba(76, 175, 80, 0.2)").alias("atr_upperband_color");
    let atr_lowerband = (col("open") - atr_osc.clone()).alias("atr_lowerband");
    let atr_lowerband_color = lit("rgba(242, 54, 69, 0.2)").alias("atr_lowerband_color");
    let atr_percent = (atr.clone() / col("open")).alias("atr_percent");

    // Relative structure power
    let structure_pwr = ohlc.bar_bias().rma(9).alias("structure_power");
    let structure_pwr_color = when(structure_pwr.clone().gt_eq(lit(0)))
        .then(lit("rgba(0, 137, 123, 1)"))
        .otherwise(lit("rgba(136, 14, 79, 1)"))
        .alias("structure_power_color");
    let structure_pwr_sma = structure_pwr.clone().sma(16).alias("structure_power_sma");
    let structure_pwr_dir = (lit(3) * structure_pwr.clone() - lit(2) * structure_pwr_sma.clone())
        .alias("structure_power_direction");

    // Relative structure strength index
    let rssi = ohlc.rssi(14).alias("rssi");
    let rssi_color = when(rssi.clone().gt(lit(59)))
        .then(lit("rgba(76, 175, 79, 1)"))
        .otherwise(
            when(rssi.clone().lt(lit(41)))
                .then(lit("rgba(242, 54, 70, 1)"))
                .otherwise(lit("rgba(191, 54, 207, 0.7)")),
        )
        .alias("rssi_color");
    let rssi_ma = rssi.clone().ema(9).alias("rssi_ma");
    let rssi_dir = (lit(3) * rssi.clone() - lit(2) * rssi_ma.clone()).alias("rssi_direction");

    // Stability indicator
    let atr_rev_percent = ohlc
        .band_reversion_percent(&atr_osc.clone(), &bias_rev.clone())
        .alias("atr_reversion_percent");
    let atr_rev_percent_color = when(atr_rev_percent.clone().gt(lit(50)))
        .then(lit("rgba(76, 175, 80, 0.5)"))
        .otherwise(
            when(atr_rev_percent.clone().lt(lit(-50)))
                .then(lit("rgba(242, 54, 69, 0.5)"))
                .otherwise(lit("rgba(41, 98, 255, 0.2)")),
        )
        .alias("atr_reversion_percent_color");

    // Signals
    let overbought = rssi
        .clone()
        .gt(lit(54))
        .logical_and(atr_rev_percent.clone().lt(lit(-50)))
        .alias("overbought");
    let oversold = rssi
        .clone()
        .lt(lit(46))
        .logical_and(atr_rev_percent.clone().gt(lit(50)))
        .alias("oversold");
    let climax_signal = when(overbought.clone().not().logical_and(oversold.clone().not()))
        .then(lit(0))
        .otherwise(when(overbought).then(lit(1)).otherwise(lit(-1)))
        .alias("climax_signal");
    let climax_signal_pos = when(climax_signal.clone().lt(lit(0)))
        .then(lit("belowBar"))
        .otherwise(lit("aboveBar"))
        .alias("climax_signal_pos");
    let climax_signal_color = when(climax_signal.clone().lt(lit(0)))
        .then(lit("rgba(33, 150, 243, 1)"))
        .otherwise(lit("rgba(233, 30, 99, 1)"))
        .alias("climax_signal_color");
    let climax_signal_shape = when(climax_signal.clone().lt(lit(0)))
        .then(lit("arrowUp"))
        .otherwise(lit("arrowDown"))
        .alias("climax_signal_shape");

    // Miscs
    let lvrg_adjust = conf.sl_percent / (1. + conf.tol_percent);
    let lvrg = (lit(lvrg_adjust) * ohlc[0].clone() / atr.clone()).alias("leverage");
    let sharpe_ratio = col("close").sharpe(200).alias("sharpe");
    let sharpe_ratio_color = when(sharpe_ratio.clone().gt(lit(0)))
        .then(lit("rgba(76, 175, 79, 0.5)"))
        .otherwise(lit("rgba(242, 54, 70, 0.5)"))
        .alias("sharpe_color");

    // Selected columns to export
    vec![
        time_to_date,
        vol_color,
        vol_sma,
        bias_rev,
        bias_rev_color,
        ema200,
        ema200_color,
        neutral_revrsi,
        neutral_revrsi_color,
        bullish_revrsi,
        bullish_revrsi_color,
        bearish_revrsi,
        bearish_revrsi_color,
        atr_upperband,
        atr_upperband_color,
        atr_lowerband,
        atr_lowerband_color,
        rssi,
        rssi_color,
        rssi_ma,
        rssi_dir,
        structure_pwr,
        structure_pwr_color,
        structure_pwr_sma,
        structure_pwr_dir,
        atr_percent,
        atr_rev_percent,
        atr_rev_percent_color,
        lvrg,
        climax_signal,
        climax_signal_pos,
        climax_signal_color,
        climax_signal_shape,
        sharpe_ratio,
        sharpe_ratio_color,
    ]
}

fn process_data(klines: &[Kline], conf: &EnvConf) -> Result<DataFrame, Box<dyn Error>> {
    let df = klines.iter().rev().cloned().to_dataframe().unwrap();
    let df_with_indicators = df.lazy().with_columns(indicators(conf)).collect().unwrap();
    Ok(df_with_indicators)
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
<html class="sl-theme-dark" style="font-size: 22px">
  <head>
    <meta charset="utf-8" />
    <title>{{ symbol }} (InNoobWeTrust™)</title>
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
        }

        #container {
            height: 100%;
        }

        #overlay {
            position: absolute;
            top: 2.5%;
            left: 2%;
            z-index: 9999;
        }
        #tf-btns {
            display: inline-block;
        }
        #fullscreen-btn {
            position: absolute;
            bottom: 15px;
            left: -6px;
            z-index: 9999;
            font-size: 10px;
        }
    </style>
  </head>
  <body>
    <div id="container" data-symbol="{{ symbol }}" data-tf="{{ default_tf }}"></div>
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
                background: { color: '#22222240' },
                textColor: '#DDD',
            },
            grid: {
                vertLines: { color: '#44444440' },
                horzLines: { color: '#44444440' },
            },
            timeScale: {
                timeVisible: true,
            },
        });
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
        const ema200Series = chart.addSeries(LightweightCharts.LineSeries, {});
        const biasRevSeries = chart.addSeries(LightweightCharts.LineSeries, {});
        const atrUpperBandSeries = chart.addSeries(LightweightCharts.LineSeries, {});
        const atrLowerBandSeries = chart.addSeries(LightweightCharts.LineSeries, {});
        const neutralRevRsiSeries = chart.addSeries(LightweightCharts.LineSeries, { lineWidth: 6, lineStyle: 2 });
        const bullishBandSeries = chart.addSeries(LightweightCharts.LineSeries, { lineWidth: 6 });
        const bearishBandSeries = chart.addSeries(LightweightCharts.LineSeries, { lineWidth: 6 });
        // Candlestick is added last in the panel to have higher z-order
        const candlestickSeries = chart.addSeries(LightweightCharts.CandlestickSeries);
        const structurePwrSeries = chart.addSeries(LightweightCharts.HistogramSeries, {}, 1);
        const structurePwrSmaSeries = chart.addSeries(LightweightCharts.BaselineSeries, {
            baseValue: { type: 'price', price: 0 },
            topLineColor: 'rgba(76, 175, 80, 0.3)',
            topFillColor1: 'rgba(76, 175, 80, 0.2)',
            topFillColor2: 'rgba(76, 175, 80, 0.5)',
            bottomLineColor: 'rgba(242, 54, 69, 0.3)',
            bottomFillColor1: 'rgba(242, 54, 69, 0.5)',
            bottomFillColor2: 'rgba(242, 54, 69, 0.2)',
        }, 1);
        const structurePwrDirSeries = chart.addSeries(LightweightCharts.BaselineSeries, {
            baseValue: { type: 'price', price: 0 },
            topLineColor: 'rgba(76, 175, 80, 0.5)',
            topFillColor1: 'rgba(76, 175, 80, 0.05)',
            topFillColor2: 'rgba(76, 175, 80, 0.1)',
            bottomLineColor: 'rgba(242, 54, 69, 0.5)',
            bottomFillColor1: 'rgba(242, 54, 69, 0.1)',
            bottomFillColor2: 'rgba(242, 54, 69, 0.05)',
        }, 1);
        const rssiSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 2);
        const rssiMaSeries = chart.addSeries(LightweightCharts.BaselineSeries, {
            baseValue: { type: 'price', price: 50 },
            topLineColor: 'rgba(76, 175, 80, 0.1)',
            topFillColor1: 'rgba(76, 175, 80, 0.2)',
            topFillColor2: 'rgba(76, 175, 80, 0.3)',
            bottomLineColor: 'rgba(242, 54, 69, 0.1)',
            bottomFillColor1: 'rgba(242, 54, 69, 0.3)',
            bottomFillColor2: 'rgba(242, 54, 69, 0.2)',
        }, 2);
        const rssiDirSeries = chart.addSeries(LightweightCharts.BaselineSeries, {
            baseValue: { type: 'price', price: 50 },
            topLineColor: 'rgba(76, 175, 80, 0.2)',
            topFillColor1: 'rgba(76, 175, 80, 0.05)',
            topFillColor2: 'rgba(76, 175, 80, 0.1)',
            bottomLineColor: 'rgba(242, 54, 69, 0.2)',
            bottomFillColor1: 'rgba(242, 54, 69, 0.1)',
            bottomFillColor2: 'rgba(242, 54, 69, 0.05)',
        }, 2);
        const atrRevSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 3);
        const sharpeSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 4);
        const markersSeries = LightweightCharts.createSeriesMarkers(candlestickSeries, []);
        const textWatermarks = [
            LightweightCharts.createTextWatermark(chart.panes()[0], {
                horzAlign: 'left',
                vertAlign: 'top',
            }),
            LightweightCharts.createTextWatermark(chart.panes()[1], {
                horzAlign: 'left',
                vertAlign: 'top',
            }),
            LightweightCharts.createTextWatermark(chart.panes()[2], {
                horzAlign: 'left',
                vertAlign: 'top',
            }),
            LightweightCharts.createTextWatermark(chart.panes()[3], {
                horzAlign: 'left',
                vertAlign: 'top',
            }),
            LightweightCharts.createTextWatermark(chart.panes()[4], {
                horzAlign: 'left',
                vertAlign: 'top',
            }),
        ];

        const watermarkUpdate = () => {
            const tf = tf_btns.value || container.dataset.tf || tfs[0];
            const atr = +(dataset[tf].slice(-1)[0].atr_percent * 100).toFixed(2);
            const lvrg = Math.floor(dataset[tf].slice(-1)[0]["leverage"]);
            atr_badge.innerHTML = `ATR: ${atr}%`;
            lvrg_badge.innerHTML = `x${lvrg}`;
            const watermarks = [
                {
                    lines: [
                        {
                            text: `${container.dataset.symbol} ${tf}`,
                            color: 'rgba(178, 181, 190, 0.5)',
                            fontSize: 24,
                        },
                    ],
                },
                {
                    lines: [
                        {
                            text: 'Structure Power (9, 16)',
                            color: 'rgba(178, 181, 190, 0.5)',
                            fontSize: 18,
                        },
                    ],
                },
                {
                    lines: [
                        {
                            text: 'RSSI (14, 9)',
                            color: 'rgba(178, 181, 190, 0.5)',
                            fontSize: 18,
                        },
                    ],
                },
                {
                    lines: [
                        {
                            text: 'ATR Reversion (42, 1.618)',
                            color: 'rgba(178, 181, 190, 0.5)',
                            fontSize: 18,
                        },
                    ],
                },
                {
                    lines: [
                        {
                            text: 'Sharpe (200)',
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
                color: d.volume_color,
            })));
            volumeSmaSeries.setData(data.map(d => ({
                time: d.time,
                value: d.volume_sma,
            })));
            ema200Series.setData(data.map(d => ({
                time: d.time,
                value: d.ema200,
                color: d.ema200_color,
            })));
            biasRevSeries.setData(data.map(d => ({
                time: d.time,
                value: d.bias_reversion,
                color: d.bias_reversion_color,
            })));
            atrUpperBandSeries.setData(data.map(d => ({
                time: d.time,
                value: d.atr_upperband,
                color: d.atr_upperband_color,
            })));
            atrLowerBandSeries.setData(data.map(d => ({
                time: d.time,
                value: d.atr_lowerband,
                color: d.atr_lowerband_color,
            })));
            neutralRevRsiSeries.setData(data.map(d => ({
                time: d.time,
                value: d.neutral_revrsi,
                color: d.neutral_revrsi_color,
            })));
            bullishBandSeries.setData(data.map(d => ({
                time: d.time,
                value: d.bullish_revrsi,
                color: d.bullish_revrsi_color,
            })));
            bearishBandSeries.setData(data.map(d => ({
                time: d.time,
                value: d.bearish_revrsi,
                color: d.bearish_revrsi_color,
            })));
            structurePwrSeries.setData(data.map(d => ({
                time: d.time,
                value: d.structure_power,
                color: d.structure_power_color,
            })));
            structurePwrSmaSeries.setData(data.map(d => ({
                time: d.time,
                value: d.structure_power_sma,
            })));
            structurePwrDirSeries.setData(data.map(d => ({
                time: d.time,
                value: d.structure_power_direction,
            })));
            rssiSeries.setData(data.map(d => ({
                time: d.time,
                value: d.rssi,
                color: d.rssi_color,
            })));
            rssiMaSeries.setData(data.map(d => ({
                time: d.time,
                value: d.rssi_ma,
            })));
            rssiDirSeries.setData(data.map(d => ({
                time: d.time,
                value: d.rssi_direction,
            })));
            atrRevSeries.setData(data.map(d => ({
                time: d.time,
                value: d.atr_reversion_percent,
                color: d.atr_reversion_percent_color,
            })));
            sharpeSeries.setData(data.map(d => ({
                time: d.time,
                value: d.sharpe,
                color: d.sharpe_color,
            })));
            const markers = data.filter(d => d.climax_signal != 0).map(d => ({
                time: d.time,
                position: d.climax_signal_pos,
                color: d.climax_signal_color,
                shape: d.climax_signal_shape,
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

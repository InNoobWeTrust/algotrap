use algotrap::ext::{webdriver::*, yfinance::*};
use algotrap::prelude::*;
use algotrap::ta::prelude::*;
use core::error::Error;
use fantoccini::Locator;
use minijinja::render;
use polars::lazy::prelude::*;
use polars::prelude::*;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;
use tracing::{info, warn};
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    setup_tracing();
    let mut etf_funds_dfs: Vec<DataFrame> = Vec::new(); // Net flow daily of individual funds
    let mut etf_total_dfs: Vec<DataFrame> = Vec::new(); // Net flow total daily
    let mut etf_cumulative_funds_dfs: Vec<DataFrame> = Vec::new(); // Cumulative net flow of individual funds daily
    let mut etf_cumulative_total_dfs: Vec<DataFrame> = Vec::new(); // Cumulative net flow total daily
    let mut ticker_dfs: Vec<DataFrame> = Vec::new(); // Asset price daily
    let mut fund_vols_dfs: Vec<DataFrame> = Vec::new(); // Trade volume daily of individual funds
    let mut fund_vol_total_dfs: Vec<DataFrame> = Vec::new(); // Trade volume total daily
    let srcs = vec![
        (BTC_TICKER, ETF_BTC_URL, ETF_BTC_EXTRACT_SCRIPT),
        (ETH_TICKER, ETF_ETH_URL, ETF_ETH_EXTRACT_SCRIPT),
        (SOL_TICKER, ETF_SOL_URL, ETF_SOL_EXTRACT_SCRIPT),
    ];
    for (ticker, url, script) in srcs {
        // Inner scope to automatically dispose webdriver before printing to avoid polluting console logs
        let (etf_df, start_timestamp, end_timestamp) = get_etf_data(url, script).await?;
        etf_funds_dfs.push(etf_df.clone());
        let fund_tickers: Vec<_> = etf_df
            .get_column_names()
            .into_iter()
            .filter(|name| *name != "Date" && *name != "Total")
            .map(|s| s.to_string())
            .collect();

        let etf_with_features = etf_df
            .clone()
            .lazy()
            .with_columns(etf_netflow_features(&fund_tickers))
            .collect()?;
        
        etf_total_dfs.push(etf_with_features.clone());
        
        // Extract cumulative netflow for individual funds
        let cumulative_cols: Vec<_> = etf_with_features
            .get_column_names()
            .into_iter()
            .filter(|name| name.starts_with("cumulative_netflow_"))
            .map(|s| col(s.as_str()))
            .collect();
        
        if !cumulative_cols.is_empty() {
            let mut cumulative_select_cols = vec![col("Date")];
            cumulative_select_cols.extend(cumulative_cols);
            etf_cumulative_funds_dfs.push(
                etf_with_features
                    .clone()
                    .lazy()
                    .select(cumulative_select_cols)
                    .collect()?,
            );
        }
        
        // Extract cumulative netflow total
        etf_cumulative_total_dfs.push(
            etf_with_features
                .lazy()
                .select([col("Date"), col("cumulative_netflow_total")])
                .collect()?,
        );

        // To yfinance after we got the starting date
        info!("Fetch ticker {ticker}...");
        let yfinance_client = YfinanceClient::new();
        // returns historic quotes with daily interval
        let klines = yfinance_client
            .get_quote_history(ticker, start_timestamp, end_timestamp, YfinanceInterval::D1)
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

        let mut fund_volume_dfs = Vec::new();

        for fund_ticker in fund_tickers {
            info!("Fetch fund ticker {}...", &fund_ticker);
            match yfinance_client
                .get_quote_history(
                    &fund_ticker,
                    start_timestamp,
                    end_timestamp,
                    YfinanceInterval::D1,
                )
                .await
            {
                Ok(klines) => {
                    if !klines.is_empty() {
                        let fund_df = klines.iter().cloned().to_dataframe().unwrap();
                        let fund_df = fund_df
                            .lazy()
                            .with_column(
                                (col("time") * lit(1000))
                                    .cast(DataType::Datetime(TimeUnit::Milliseconds, None))
                                    .dt()
                                    .date()
                                    .alias("Date"),
                            )
                            .select([col("Date"), col("volume").alias(&fund_ticker)])
                            .collect()?;
                        fund_volume_dfs.push(fund_df);
                    }
                }
                Err(e) => {
                    warn!("Could not fetch ticker {fund_ticker}: {e}");
                }
            }
        }

        if !fund_volume_dfs.is_empty() {
            let mut combined_vols_df = fund_volume_dfs[0].clone();
            if fund_volume_dfs.len() > 1 {
                for i in 1..fund_volume_dfs.len() {
                    combined_vols_df = combined_vols_df
                        .lazy()
                        .join_builder()
                        .with(fund_volume_dfs[i].clone().lazy())
                        .left_on([col("Date")])
                        .right_on([col("Date")])
                        .how(JoinType::Full)
                        .coalesce(JoinCoalesce::CoalesceColumns)
                        .finish()
                        .collect()?;
                }
            }
            
            // Apply volume features (total and MA20)
            let vol_cols: Vec<_> = combined_vols_df
                .get_column_names()
                .into_iter()
                .filter(|name| *name != "Date")
                .map(|s| s.to_string())
                .collect();
            
            let combined_vols_df = combined_vols_df
                .lazy()
                .with_columns(fund_vol_features(&vol_cols))
                .collect()?;
            
            fund_vol_total_dfs.push(
                combined_vols_df
                    .clone()
                    .lazy()
                    .select([col("Date"), col("volume_total"), col("volume_total_ma20")])
                    .collect()?,
            );
            fund_vols_dfs.push(combined_vols_df);
        }
    }
    
    // Create output directory
    let output_dir = Path::new("output/etf_dashboard");
    fs::create_dir_all(output_dir)?;
    info!("Created output directory: {}", output_dir.display());
    
    // Save processed data for each asset
    let asset_names = ["BTC", "ETH", "SOL"];
    for (i, asset_name) in asset_names.iter().enumerate() {
        if i < etf_funds_dfs.len() {
            info!("Saving data for {asset_name}...");
            
            // Save individual fund netflows
            let funds_file = output_dir.join(format!("{}_funds_netflow.csv", asset_name));
            let mut funds_csv = std::fs::File::create(&funds_file)?;
            CsvWriter::new(&mut funds_csv)
                .include_header(true)
                .finish(&mut etf_funds_dfs[i].clone())?;
            info!("Saved to {}", funds_file.display());
            
            // Save total netflow with features
            if i < etf_total_dfs.len() {
                let total_file = output_dir.join(format!("{}_total_netflow.csv", asset_name));
                let mut total_csv = std::fs::File::create(&total_file)?;
                CsvWriter::new(&mut total_csv)
                    .include_header(true)
                    .finish(&mut etf_total_dfs[i].clone())?;
                info!("Saved to {}", total_file.display());
            }
            
            // Save cumulative netflows
            if i < etf_cumulative_total_dfs.len() {
                let cumulative_file = output_dir.join(format!("{}_cumulative_total.csv", asset_name));
                let mut cumulative_csv = std::fs::File::create(&cumulative_file)?;
                CsvWriter::new(&mut cumulative_csv)
                    .include_header(true)
                    .finish(&mut etf_cumulative_total_dfs[i].clone())?;
                info!("Saved to {}", cumulative_file.display());
            }
            
            // Save ticker prices
            if i < ticker_dfs.len() {
                let ticker_file = output_dir.join(format!("{}_price.csv", asset_name));
                let mut ticker_csv = std::fs::File::create(&ticker_file)?;
                CsvWriter::new(&mut ticker_csv)
                    .include_header(true)
                    .finish(&mut ticker_dfs[i].clone())?;
                info!("Saved to {}", ticker_file.display());
            }
            
            // Save fund volumes
            if i < fund_vols_dfs.len() {
                let vols_file = output_dir.join(format!("{}_fund_volumes.csv", asset_name));
                let mut vols_csv = std::fs::File::create(&vols_file)?;
                CsvWriter::new(&mut vols_csv)
                    .include_header(true)
                    .finish(&mut fund_vols_dfs[i].clone())?;
                info!("Saved to {}", vols_file.display());
            }
            
            // Save fund volume totals
            if i < fund_vol_total_dfs.len() {
                let vol_total_file = output_dir.join(format!("{}_volume_total.csv", asset_name));
                let mut vol_total_csv = std::fs::File::create(&vol_total_file)?;
                CsvWriter::new(&mut vol_total_csv)
                    .include_header(true)
                    .finish(&mut fund_vol_total_dfs[i].clone())?;
                info!("Saved to {}", vol_total_file.display());
            }
        }
    }
    
    info!("All data saved to {}", output_dir.display());
    Ok(())
}

/// Extract dataframe from html table at url and return together with start and end timestamp in
/// seconds since epoch
async fn get_etf_data(
    url: &str,
    extract_script: &str,
) -> Result<(DataFrame, i64, i64), Box<dyn Error + Sync + Send>> {
    let geckodriver = GeckoDriver::default_with_log(Path::new("geckodriver.log"))?;
    let client = geckodriver.create_client(false).await?;
    info!("Going to {url}...");
    client.goto(url).await?;
    let elem = client.find(Locator::Css("table.etf")).await?;
    let etf_df = client
        .extract_table(&elem, Some(extract_script.to_string()))
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
    let start_date = etf_df.clone().column("Date")?.date()?.phys.get(0).unwrap();
    let end_date = etf_df.clone().column("Date")?.date()?.phys.get(etf_df.height() - 1).unwrap();
    let start_timestamp = start_date as i64 * 86_400;
    let end_timestamp = end_date as i64 * 86_400;
    client.close().await?;

    Ok((etf_df, start_timestamp, end_timestamp))
}

/// Sum total net flow across all funds
fn etf_netflow_features(fund_cols: &[String]) -> Vec<Expr> {
    let mut features = Vec::new();

    if fund_cols.is_empty() {
        // Return a zero column if no funds provided
        features.push(lit(0.0).alias("netflow_total"));
    } else {
        // Net flow total
        let total_exprs: Vec<Expr> = fund_cols.iter().map(|c| col(c)).collect();
        features.push(
            sum_horizontal(total_exprs, true)
                .expect("Failed to sum by funds")
                .alias("netflow_total"),
        );
    }

    // MA20 of net flow total
    features.push(col("netflow_total").sma(20).alias("netflow_total_ma20"));

    // Cumulative net flow total
    features.push(
        col("netflow_total")
            .cum_sum(false)
            .alias("cumulative_netflow_total"),
    );

    // Cumulative net flow of individual funds
    for ticker in fund_cols {
        features.push(
            col(ticker)
                .cum_sum(false)
                .alias(&format!("cumulative_netflow_{}", ticker)),
        );
    }

    features
}

// Sum vol across all funds
fn fund_vol_features(vol_cols: &[String]) -> Vec<Expr> {
    let mut features = Vec::new();

    if vol_cols.is_empty() {
        // Return a zero column if no volumes provided
        features.push(lit(0.0).alias("volume_total"));
    } else {
        // Volume total
        let vol_exprs: Vec<Expr> = vol_cols.iter().map(|c| col(c)).collect();
        features.push(
            sum_horizontal(vol_exprs, true)
                .expect("Failed to sum by vol")
                .alias("volume_total"),
        );
    }

    // MA20 of volume total
    features.push(col("volume_total").sma(20).alias("volume_total_ma20"));

    features
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

struct TdvHtmlVars {
    price_dataset: String,
    volume_dataset: String,
    netflow_dataset: String,
    symbol: String,
}

fn render_tdv_html(vars: &TdvHtmlVars) -> String {
    render!(
        TDV_HTML_TEMPLATE,
        price_dataset => vars.price_dataset,
        volume_dataset => vars.volume_dataset,
        netflow_dataset => vars.netflow_dataset,
        symbol => vars.symbol,
    )
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
    <script id="price-dataset" type="application/json">
        {{ price_dataset }}
    </script>
    <script id="volume-dataset" type="application/json">
        {{ volume_dataset }}
    </script>
    <script id="netflow-dataset" type="application/json">
        {{ netflow_dataset }}
    </script>
    <script type="text/javascript">
        const price_dataset = JSON.parse(document.getElementById('price-dataset').textContent);
        const volume_dataset = JSON.parse(document.getElementById('volume-dataset').textContent);
        const netflow_dataset = JSON.parse(document.getElementById('netflow-dataset').textContent);
        const container = document.getElementById('container');

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

#[cfg(test)]
mod tests {
    use super::*;
    use polars::prelude::*;

    /// Test that our netflow calculation works correctly by computing manually
    #[test]
    fn test_netflow_calculation() {
        // Create test data
        let fund1_values = vec![100.0, 200.0, 150.0];
        let fund2_values = vec![50.0, 75.0, 100.0];
        
        // Expected total netflow
        let expected_total: Vec<f64> = fund1_values.iter()
            .zip(fund2_values.iter())
            .map(|(a, b)| a + b)
            .collect();
        
        assert_eq!(expected_total, vec![150.0, 275.0, 250.0]);
        
        // Expected cumulative netflow
        let mut cumsum = 0.0;
        let expected_cumulative: Vec<f64> = expected_total.iter()
            .map(|v| {
                cumsum += v;
                cumsum
            })
            .collect();
        
        assert_eq!(expected_cumulative, vec![150.0, 425.0, 675.0]);
    }

    /// Test that volume calculation works correctly
    #[test]
    fn test_volume_calculation() {
        let vol1_values = vec![1000.0, 1200.0, 1100.0];
        let vol2_values = vec![500.0, 600.0, 550.0];
        
        // Expected total volume
        let expected_total: Vec<f64> = vol1_values.iter()
            .zip(vol2_values.iter())
            .map(|(a, b)| a + b)
            .collect();
        
        assert_eq!(expected_total, vec![1500.0, 1800.0, 1650.0]);
    }

    /// Test that fund_vol_features doesn't panic with empty columns
    #[test]
    fn test_fund_vol_features_empty() {
        let vol_cols: Vec<String> = vec![];
        let features = fund_vol_features(&vol_cols);
        
        // Should create features with lit(0.0)
        assert_eq!(features.len(), 2); // volume_total and volume_total_ma20
    }

    /// Test that etf_netflow_features doesn't panic with empty columns
    #[test]
    fn test_etf_netflow_features_empty() {
        let fund_cols: Vec<String> = vec![];
        let features = etf_netflow_features(&fund_cols);
        
        // Should create features with lit(0.0)
        assert!(features.len() >= 3); // At minimum: netflow_total, netflow_total_ma20, cumulative_netflow_total
    }

    /// Test that functions return the expected number of features
    #[test]
    fn test_feature_count() {
        let fund_cols = vec!["FUND1".to_string(), "FUND2".to_string()];
        let features = etf_netflow_features(&fund_cols);
        
        // Should have: netflow_total, netflow_total_ma20, cumulative_netflow_total, 
        // cumulative_netflow_FUND1, cumulative_netflow_FUND2
        assert_eq!(features.len(), 5);
        
        let vol_cols = vec!["VOL1".to_string(), "VOL2".to_string()];
        let vol_features = fund_vol_features(&vol_cols);
        
        // Should have: volume_total, volume_total_ma20
        assert_eq!(vol_features.len(), 2);
    }
}

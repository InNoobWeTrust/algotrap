use core::error::Error;
use core::time::Duration;
use dotenv::dotenv;
use futures::future::join_all;
use minijinja::render;
use rayon::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

use algotrap::engine::error::MarketError;
use algotrap::engine::traits::ComputedFrame;
use algotrap::engine::validation::ValidatedTicker;
use algotrap::ext::bingx::MAX_LIMIT;
use algotrap::prelude::*;
use algotrap::query::gap_zones::{GapZoneDirection, GapZoneRecord};
use algotrap::time_utils::next_close_across_tfs;

mod presentation;

// ─── Per-Ticker Config ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct TickerConf {
    symbol: String,
    sl_percent: f64,
    tol_percent: f64,
    default_tf: Timeframe,
    // Signal-only fields are ignored if present in existing TICKERS JSON.
    // Serde silently drops unknown fields by default.
}

// ─── Global Config ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct EnvConf {
    #[serde(deserialize_with = "deserialize_tickers")]
    tickers: Vec<TickerConf>,
    #[serde(deserialize_with = "deserialize_tfs")]
    chart_tfs: Vec<Timeframe>, // shared chart display TFs
    #[serde(default = "default_scan_interval")]
    scan_interval_secs: u64, // only used in --loop mode
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64, // per-request timeout
}

// ─── Serde helpers ───────────────────────────────────────────────────────────

/// Deserialize `TICKERS` env var: a JSON array of TickerConf objects.
fn deserialize_tickers<'de, D>(deserializer: D) -> Result<Vec<TickerConf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let s = s.trim();
    if s.is_empty() {
        return Err(serde::de::Error::custom(
            "TICKERS is required and must not be empty",
        ));
    }

    serde_json::from_str(s).map_err(|json_err| {
        serde::de::Error::custom(format!("failed to parse TICKERS as JSON: {json_err}"))
    })
}

/// Deserialize comma-separated timeframes (required field).
fn deserialize_tfs<'de, D>(deserializer: D) -> Result<Vec<Timeframe>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let s = s.trim();
    if s.is_empty() {
        return Err(serde::de::Error::custom(
            "CHART_TFS is required and must not be empty",
        ));
    }

    s.split(',')
        .map(|tf| {
            tf.trim()
                .parse::<Timeframe>()
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

fn require_non_empty_env(name: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(()),
        Ok(_) => Err(format!("{name} is required but is empty").into()),
        Err(std::env::VarError::NotPresent) => {
            Err(format!("{name} is required but was not set").into())
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(format!("{name} must be valid Unicode").into())
        }
    }
}

fn default_scan_interval() -> u64 {
    900 // 15 minutes (used only in --loop mode)
}

fn default_timeout_secs() -> u64 {
    10
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    dotenv().ok();
    require_non_empty_env("TICKERS")?;
    require_non_empty_env("CHART_TFS")?;
    let conf: EnvConf = envy::from_env()?;

    // --loop flag: re-enables the scheduled loop (e.g. for local/K8s use).
    // Default behavior is one-shot: run once and exit (used by GH Actions cron).
    let loop_mode = std::env::args().any(|a| a == "--loop");

    eprintln!(
        "Starting cryptobot — {} tickers, chart_tfs={:?}, mode={}",
        conf.tickers.len(),
        conf.chart_tfs,
        if loop_mode { "loop" } else { "one-shot" },
    );
    for tc in &conf.tickers {
        eprintln!("  ticker: {} (default_tf={})", tc.symbol, tc.default_tf);
    }

    eprintln!("─── Scan cycle start ───");
    match run_cycle(&conf).await {
        Ok(()) => eprintln!("─── Scan cycle complete ───"),
        Err(e) => eprintln!("─── Scan cycle failed: {e:#} ───"),
    }

    if !loop_mode {
        return Ok(());
    }

    // ─── Loop mode: re-run aligned to chart TF candle closes ─────────────────
    // Lead time: wake this many seconds BEFORE candle close so data is fresh.
    const LEAD_SECS: u64 = 5;
    loop {
        let now = chrono::Utc::now();
        let max_interval = Duration::from_secs(conf.scan_interval_secs);
        let sleep_dur = match next_close_across_tfs(&conf.chart_tfs, now) {
            Some((secs_until, tf)) => {
                let target = secs_until.saturating_sub(LEAD_SECS);
                let capped = target.min(max_interval.as_secs());
                let dur = Duration::from_secs(capped.max(10)); // floor: 10s minimum
                eprintln!(
                    "Next close: {} in {}s — sleeping {:.0}s (lead={}s)",
                    tf,
                    secs_until,
                    dur.as_secs_f64(),
                    LEAD_SECS,
                );
                dur
            }
            None => {
                eprintln!(
                    "No upcoming close found — sleeping {}s",
                    max_interval.as_secs()
                );
                max_interval
            }
        };
        tokio::time::sleep(sleep_dur).await;

        eprintln!("─── Scan cycle start ───");
        match run_cycle(&conf).await {
            Ok(()) => eprintln!("─── Scan cycle complete ───"),
            Err(e) => eprintln!("─── Scan cycle failed: {e:#} ───"),
        }
    }
}

// ─── Scan Cycle ──────────────────────────────────────────────────────────────

async fn run_cycle(conf: &EnvConf) -> Result<(), Box<dyn Error + Send + Sync>> {
    let client = ext::bingx::BingXClient::default();
    let timeout = Duration::from_secs(conf.timeout_secs);

    // Prepare output directory
    let output_dir = std::path::Path::new("output");
    let data_dir = output_dir.join("data");
    tokio::fs::create_dir_all(&data_dir).await?;

    // Collect ticker metadata for the HTML template
    let mut tickers_meta: Vec<TickerMeta> = Vec::new();

    // Process each ticker
    for ticker in &conf.tickers {
        eprintln!("Processing {}...", ticker.symbol);

        let result = tokio::time::timeout(
            timeout * conf.chart_tfs.len() as u32, // scale timeout by number of TFs
            process_ticker(ticker, &conf.chart_tfs, &client),
        )
        .await;

        match result {
            Ok(Ok(chart_json)) => {
                // Write chart data JSON
                let json_path = data_dir.join(format!("{}.json", ticker.symbol));
                tokio::fs::write(&json_path, &chart_json).await?;
                eprintln!(
                    "  Wrote {} ({} bytes)",
                    json_path.display(),
                    chart_json.len()
                );

                tickers_meta.push(TickerMeta {
                    symbol: ticker.symbol.clone(),
                    sl_percent: format!("{:.0}", ticker.sl_percent * 100.),
                    tol_percent: format!("{:.2}", ticker.tol_percent * 100.),
                    default_tf: ticker.default_tf.to_string(),
                });
            }
            Ok(Err(e)) => {
                eprintln!("  Error processing {}: {e:#}", ticker.symbol);
            }
            Err(_) => {
                eprintln!("  Timeout processing {}", ticker.symbol);
            }
        }
    }

    // Render index.html (no data embedded — data loaded via fetch from same origin)
    let tickers_json = serde_json::to_string(&tickers_meta)?;
    let chart_tfs_json = serde_json::to_string(&conf.chart_tfs)?;
    let html = render_tdv_html(&tickers_json, &chart_tfs_json);
    let html_path = output_dir.join("index.html");
    tokio::fs::write(&html_path, &html).await?;
    eprintln!("Wrote {}", html_path.display());

    // Upload to R2 is handled by the GH Actions workflow (wrangler r2 object put).
    // Nothing to do here — just write output/ and exit.

    Ok(())
}

// ─── Per-Ticker Processing ───────────────────────────────────────────────────

async fn process_ticker(
    ticker: &TickerConf,
    chart_tfs: &[Timeframe],
    client: &ext::bingx::BingXClient,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    // Fetch all chart TFs concurrently
    let mut chart_tfs_ordered = Vec::with_capacity(chart_tfs.len());
    for timeframe in chart_tfs {
        if !chart_tfs_ordered.contains(timeframe) {
            chart_tfs_ordered.push(*timeframe);
        }
    }

    let fetched = join_all(
        chart_tfs_ordered
            .iter()
            .map(|tf| {
                let symbol = ticker.symbol.clone();
                async move {
                    client
                        .get_futures_klines(&symbol, &tf.to_string(), MAX_LIMIT)
                        .await
                        .map(|k| (*tf, k))
                }
            })
            .collect::<Vec<_>>(),
    )
    .await;
    let fetched = fetched
        .into_iter()
        .filter_map(|res| match res {
            Ok(frame) => Some(frame),
            Err(err) => {
                eprintln!("  Error fetching {}: {err:#?}", ticker.symbol);
                None
            }
        })
        .collect();
    let all_dfs = compute_crypto_frames(fetched, ticker).await;

    // Serialize all fetched TFs to JSON for the chart
    let chart_dfs_serialized: HashMap<String, Value> = all_dfs
        .par_iter()
        .map(|(tf, (df, zones))| {
            let records = df.to_json_records()?;
            let candles_json = serde_json::Value::Array(
                records.into_iter().map(serde_json::Value::Object).collect(),
            );
            let gap_zones_json =
                serde_json::Value::Array(zones.iter().map(gap_zone_to_json).collect());
            let tf_json = serde_json::json!({
                "candles": candles_json,
                "gapZones": gap_zones_json,
            });
            Ok::<_, MarketError>((tf.to_string(), tf_json))
        })
        .collect::<Result<HashMap<_, _>, MarketError>>()?;
    let chart_json = serde_json::to_string(&chart_dfs_serialized)?;

    Ok(chart_json)
}

async fn compute_crypto_frames(
    fetched: Vec<(Timeframe, Vec<Kline>)>,
    ticker: &TickerConf,
) -> HashMap<Timeframe, (Box<dyn ComputedFrame>, Vec<GapZoneRecord>)> {
    let validated_ticker =
        match ValidatedTicker::new(&ticker.symbol, ticker.sl_percent, ticker.tol_percent) {
            Ok(validated_ticker) => validated_ticker,
            Err(err) => {
                for (timeframe, _) in fetched {
                    eprintln!(
                        "  Error computing indicators for {} {}: {err:#}",
                        ticker.symbol, timeframe
                    );
                }
                return HashMap::new();
            }
        };

    let computations = fetched.into_iter().map(|(timeframe, klines)| {
        let validated_ticker = validated_ticker.clone();
        async move {
            (
                timeframe,
                presentation::compute_crypto_frame(klines, validated_ticker).await,
            )
        }
    });

    join_all(computations)
        .await
        .into_iter()
        .filter_map(|(timeframe, result)| match result {
            Ok(frame) => Some((timeframe, frame)),
            Err(err) => {
                eprintln!(
                    "  Error computing indicators for {} {}: {err:#}",
                    ticker.symbol, timeframe
                );
                None
            }
        })
        .collect()
}

fn gap_zone_to_json(zone: &GapZoneRecord) -> Value {
    serde_json::json!({
        "time_ms": zone.time_ms,
        "open": zone.open,
        "high": zone.high,
        "low": zone.low,
        "close": zone.close,
        "volume": zone.volume,
        "body_bottom": zone.body_bottom,
        "body_top": zone.body_top,
        "body_ratio": zone.body_ratio,
        "direction": match zone.direction {
            GapZoneDirection::Bullish => "bullish",
            GapZoneDirection::Bearish => "bearish",
            GapZoneDirection::Flat => "flat",
        },
    })
}

// ─── Ticker metadata for HTML template ───────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct TickerMeta {
    symbol: String,
    sl_percent: String,
    tol_percent: String,
    default_tf: String,
}

fn render_tdv_html(tickers_json: &str, chart_tfs_json: &str) -> String {
    render!(
        TDV_HTML_TEMPLATE,
        tickers_json => tickers_json,
        chart_tfs => chart_tfs_json,
    )
    .trim()
    .to_string()
}

const TDV_HTML_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html class="sl-theme-dark" style="font-size: 22px">
  <head>
    <meta charset="utf-8" />
    <title>InNoobWeTrust™ CryptoBot</title>
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
        #ticker-select {
            min-width: 200px;
            margin-bottom: 0.25rem;
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
        #loading-overlay {
            position: fixed;
            inset: 0;
            display: flex;
            align-items: center;
            justify-content: center;
            background: rgba(0,0,0,0.6);
            z-index: 99999;
            pointer-events: none;
            opacity: 0;
            transition: opacity 0.2s;
        }
        #loading-overlay.active {
            opacity: 1;
            pointer-events: auto;
        }
    </style>
  </head>
  <body>
    <div id="container" data-symbol="" data-tf=""></div>
    <div id="loading-overlay"><sl-spinner style="font-size: 3rem; --track-width: 4px;"></sl-spinner></div>
    <div id="overlay">
        <div id="badges">
            <sl-badge id="sl-badge" variant="danger" pill>SL: -</sl-badge>
            <sl-badge id="tol-badge" variant="success" pill>Tol: -</sl-badge>
            <sl-badge id="atr-percent" variant="warning" pill>ATR: -</sl-badge>
            <sl-badge id="leverage" variant="primary" pill>Lvrg: -</sl-badge>
        </div>
        <sl-divider style="--spacing: 0.25rem;"></sl-divider>
        <sl-select id="ticker-select" size="small" placeholder="Select ticker"></sl-select>
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

      document.addEventListener('fullscreenchange', () => {
        if (document.fullscreenElement) {
          fullscreenButton.name = 'fullscreen-exit';
        } else {
          fullscreenButton.name = 'fullscreen';
        }
      });
    </script>
    <!-- Gap Zone Primitive classes (must be defined before onIntervalUpdate) -->
    <script type="text/javascript">
        class GapZoneBandRenderer {
            constructor(zones, chartData) { this._zones = zones; this._data = chartData; this._series = null; this._chart = null; }
            update(series, chart) { this._series = series; this._chart = chart; }
            draw(target) {
                const s = this._series; const c = this._chart;
                if (!s || !c || !this._data.length) return;
                target.useBitmapCoordinateSpace(scope => {
                    const ctx = scope.context;
                    const ts = c.timeScale();
                    const firstTime = this._data[0]?.time;
                    const lastTime = this._data[this._data.length - 1]?.time;
                    if (!firstTime || !lastTime) return;
                    const xLeft = ts.timeToCoordinate(firstTime);
                    const xRight = ts.timeToCoordinate(lastTime);
                    if (xLeft === null || xRight === null) return;
                    const ratio = scope.horizontalPixelRatio;
                    const vRatio = scope.verticalPixelRatio;
                    this._zones.forEach(z => {
                        const top = z.body_top ?? z.top;
                        const bottom = z.body_bottom ?? z.bottom;
                        const yTop = s.priceToCoordinate(top);
                        const yBot = s.priceToCoordinate(bottom);
                        if (yTop === null || yBot === null) return;
                        ctx.fillStyle = z.direction === 'bullish'
                            ? `rgba(33,150,243,0.12)`
                            : z.direction === 'bearish'
                                ? `rgba(255,152,0,0.12)`
                                : `rgba(158,158,158,0.12)`;
                        const x = Math.round(xLeft * ratio);
                        const w = Math.round((xRight - xLeft + 40) * ratio);
                        const y = Math.round(Math.min(yTop, yBot) * vRatio);
                        const h = Math.round(Math.abs(yBot - yTop) * vRatio);
                        ctx.fillRect(x, y, w, h);
                        ctx.strokeStyle = z.direction === 'bullish'
                            ? `rgba(33,150,243,0.4)`
                            : z.direction === 'bearish'
                                ? `rgba(255,152,0,0.4)`
                                : `rgba(158,158,158,0.4)`;
                        ctx.lineWidth = 1;
                        ctx.setLineDash([4 * ratio, 4 * ratio]);
                        ctx.beginPath();
                        ctx.moveTo(x, y); ctx.lineTo(x + w, y);
                        ctx.moveTo(x, y + h); ctx.lineTo(x + w, y + h);
                        ctx.stroke();
                        ctx.setLineDash([]);
                    });
                });
            }
        }
        class GapZonePaneView {
            constructor(renderer) { this._renderer = renderer; }
            renderer() { return this._renderer; }
        }
        class GapZonePrimitive {
            constructor(zones, chartData) {
                this._renderer = new GapZoneBandRenderer(zones, chartData);
                this._paneView = new GapZonePaneView(this._renderer);
            }
            attached({ series, chart }) { this._renderer.update(series, chart); }
            detached() { this._renderer.update(null, null); }
            paneViews() { return [this._paneView]; }
            updateAllViews() {}
        }
    </script>
    <script type="text/javascript">
        // ─── Config from template ─────────────────────────────────────
        const tickers = {{ tickers_json }};
        const chartTfs = {{ chart_tfs }};

        // ─── DOM refs ─────────────────────────────────────────────────
        const tickerSelect = document.getElementById('ticker-select');
        const tf_btns = document.getElementById('tf-btns');
        const container = document.getElementById('container');
        const slBadge = document.getElementById('sl-badge');
        const tolBadge = document.getElementById('tol-badge');
        const atr_badge = document.getElementById('atr-percent');
        const lvrg_badge = document.getElementById('leverage');
        const loadingOverlay = document.getElementById('loading-overlay');

        // ─── Global state ─────────────────────────────────────────────
        let dataset = {};
        let currentTicker = null;

        // ─── Populate ticker dropdown ─────────────────────────────────
        tickers.forEach(t => {
            const opt = document.createElement('sl-option');
            opt.value = t.symbol;
            opt.textContent = `BingX:${t.symbol}`;
            tickerSelect.appendChild(opt);
        });

        // ─── Create chart ─────────────────────────────────────────────
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
            scaleMargins: {
                top: 0.8,
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
            priceScaleId: '',
        });
        volumeSmaSeries.priceScale().applyOptions({
            scaleMargins: {
                top: 0.8,
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
        const atrRevSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 2);
        const ichingOriginalSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#4FC3F7', lineWidth: 2 }, 3);
        const ichingTransformedSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#FFB74D', lineWidth: 2 }, 3);
        const ichingNuclearSeries = chart.addSeries(LightweightCharts.LineSeries, { color: '#CE93D8', lineWidth: 2 }, 3);
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
        ];

        // ─── Watermark update ─────────────────────────────────────────
        const watermarkUpdate = () => {
            const tf = tf_btns.value || container.dataset.tf || chartTfs[0];
            const lastBar = dataset[tf]?.candles?.slice(-1)[0];
            if (lastBar) {
                const atr = +(lastBar.atr_percent * 100).toFixed(2);
                const lvrg = Math.floor(lastBar.leverage);
                atr_badge.innerHTML = `ATR: ${atr}%`;
                lvrg_badge.innerHTML = `x${lvrg}`;
            }
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
                            text: 'ATR Reversion (42, 1.618)',
                            color: 'rgba(178, 181, 190, 0.5)',
                            fontSize: 18,
                        },
                    ],
                },
                {
                    lines: [
                        {
                            text: 'I-Ching Original',
                            color: '#4FC3F7',
                            fontSize: 18,
                        },
                        {
                            text: 'I-Ching Transformed',
                            color: '#FFB74D',
                            fontSize: 18,
                        },
                        {
                            text: 'I-Ching Nuclear',
                            color: '#CE93D8',
                            fontSize: 18,
                        },
                    ],
                },
            ];
            Object.entries(textWatermarks).forEach(([k,v]) => {
                v.applyOptions(watermarks[k]);
            });
        }

        // ─── TF update ────────────────────────────────────────────────
        const onIntervalUpdate = (tf) => {
            const tfData = dataset[tf];
            if (!tfData) return;
            const data = tfData.candles.map(d => ({
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
            atrRevSeries.setData(data.map(d => ({
                time: d.time,
                value: d.atr_reversion_percent,
                color: d.atr_reversion_percent_color,
            })));
            ichingOriginalSeries.setData(data.map(d => ({
                time: d.time,
                value: d.iching_original_energy,
            })));
            ichingTransformedSeries.setData(data.map(d => ({
                time: d.time,
                value: d.iching_transformed_energy,
            })));
            ichingNuclearSeries.setData(data.map(d => ({
                time: d.time,
                value: d.iching_nuclear_energy,
            })));
            const markers = [];
            // ATR Climax circles
            data.forEach(d => {
                if (d.close >= d.atr_upperband) {
                    markers.push({
                        time: d.time,
                        position: 'aboveBar',
                        shape: 'circle',
                        color: 'rgba(157,225,159,0.7)',
                        size: 3,
                    });
                }
                if (d.close <= d.atr_lowerband) {
                    markers.push({
                        time: d.time,
                        position: 'belowBar',
                        shape: 'circle',
                        color: 'rgba(201,134,134,0.7)',
                        size: 3,
                    });
                }
            });
            markers.sort((a, b) => a.time - b.time);
            markersSeries.setMarkers(markers);

            // ─── Gap Zone Bands (precomputed) ──────────────────────────
            const suppliedGaps = Array.isArray(tfData.gapZones) ? tfData.gapZones : [];
            if (window._gapPrimitive) {
                try { candlestickSeries.detachPrimitive(window._gapPrimitive); } catch(_) {}
            }
            if (suppliedGaps.length > 0) {
                window._gapPrimitive = new GapZonePrimitive(suppliedGaps, data);
                candlestickSeries.attachPrimitive(window._gapPrimitive);
            }

            watermarkUpdate();
        }

        // ─── Layout ───────────────────────────────────────────────────
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

        // ─── TF buttons ───────────────────────────────────────────────
        chartTfs.forEach(tf => {
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

        // ─── Ticker loading ───────────────────────────────────────────
        async function loadTicker(symbol) {
            loadingOverlay.classList.add('active');
            try {
                const resp = await fetch(`data/${symbol}.json`);
                if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
                dataset = await resp.json();

                const ticker = tickers.find(t => t.symbol === symbol);
                currentTicker = ticker;
                container.dataset.symbol = `BingX:${symbol}`;
                container.dataset.tf = ticker.default_tf;

                // Update badges
                slBadge.innerHTML = `SL: ${ticker.sl_percent}%`;
                tolBadge.innerHTML = `Tol: ${ticker.tol_percent}%`;

                // Update page title
                document.title = `BingX:${symbol} (InNoobWeTrust™)`;

                // Click default TF
                requestAnimationFrame(() => {
                    const defaultBtn = [...tf_btns.children].find(b => b.textContent == ticker.default_tf);
                    if (defaultBtn) defaultBtn.click();
                    else if (tf_btns.children[0]) tf_btns.children[0].click();
                });
            } catch(e) {
                console.error(`Failed to load ticker data for ${symbol}:`, e);
            } finally {
                loadingOverlay.classList.remove('active');
            }
        }

        // ─── URL sync + init ──────────────────────────────────────────
        tickerSelect.addEventListener('sl-change', (e) => {
            const symbol = e.target.value;
            history.replaceState(null, '', `?ticker=${symbol}`);
            loadTicker(symbol);
        });

        // Initial load from URL params or first ticker
        const params = new URLSearchParams(location.search);
        const initialTicker = params.get('ticker') || tickers[0]?.symbol;
        if (initialTicker) {
            tickerSelect.value = initialTicker;
            loadTicker(initialTicker);
        }
    </script>
  </body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn ticker() -> TickerConf {
        TickerConf {
            symbol: "BTC-USDT".to_string(),
            sl_percent: 0.02,
            tol_percent: 0.01,
            default_tf: Timeframe::H1,
        }
    }

    fn klines(seed: f64) -> Vec<Kline> {
        (0..240)
            .map(|index| {
                let open = seed + index as f64;
                Kline {
                    open,
                    high: open + 4.0,
                    low: open - 2.0,
                    close: open + if index % 2 == 0 { 2.0 } else { -1.0 },
                    volume: 1_000.0 + index as f64,
                    time: 1_700_000_000_000 + index as i64 * 60_000,
                    adjclose: None,
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn crypto_batch_adapter_preserves_timeframes_and_isolates_invalid_siblings() {
        let ticker = ticker();
        let valid_5m = klines(100.0);
        let valid_1h = klines(200.0);
        let mut invalid = klines(300.0);
        invalid[0].open = f64::NAN;

        let frames = compute_crypto_frames(
            vec![
                (Timeframe::H1, valid_1h.clone()),
                (Timeframe::M1, invalid),
                (Timeframe::M5, valid_5m.clone()),
            ],
            &ticker,
        )
        .await;

        assert_eq!(frames.len(), 2);
        assert!(!frames.contains_key(&Timeframe::M1));
        let validated =
            ValidatedTicker::new(&ticker.symbol, ticker.sl_percent, ticker.tol_percent).unwrap();
        for (timeframe, klines) in [(Timeframe::H1, valid_1h), (Timeframe::M5, valid_5m)] {
            let expected = presentation::compute_crypto_frame(klines, validated.clone())
                .await
                .unwrap();
            assert_eq!(
                frames[&timeframe].0.to_json_records().unwrap(),
                expected.0.to_json_records().unwrap(),
                "timeframe {timeframe} must retain its matching batch result"
            );
            assert_eq!(
                frames[&timeframe].1, expected.1,
                "timeframe {timeframe} must retain its matching gap zones"
            );
            assert_eq!(
                frames[&timeframe]
                    .1
                    .iter()
                    .map(|zone| zone.time_ms)
                    .collect::<Vec<_>>(),
                expected
                    .1
                    .iter()
                    .map(|zone| zone.time_ms)
                    .collect::<Vec<_>>(),
                "timeframe {timeframe} must retain raw gap-zone ordering"
            );
        }
    }

    #[test]
    fn gap_zone_to_json_emits_exactly_raw_keys() {
        let zone = GapZoneRecord {
            time_ms: 1_700_000_000_000,
            open: 100.0,
            high: 115.0,
            low: 95.0,
            close: 110.0,
            volume: 1_000.0,
            body_bottom: 100.0,
            body_top: 110.0,
            direction: GapZoneDirection::Bullish,
            body_ratio: Some(0.8),
        };
        let value = gap_zone_to_json(&zone);
        let object = value.as_object().expect("gap zone JSON must be an object");
        assert_eq!(value["time_ms"], serde_json::json!(1_700_000_000_000i64));
        assert_eq!(value["open"], serde_json::json!(100.0));
        assert_eq!(value["high"], serde_json::json!(115.0));
        assert_eq!(value["low"], serde_json::json!(95.0));
        assert_eq!(value["close"], serde_json::json!(110.0));
        assert_eq!(value["volume"], serde_json::json!(1_000.0));
        assert_eq!(value["body_bottom"], serde_json::json!(100.0));
        assert_eq!(value["body_top"], serde_json::json!(110.0));
        assert_eq!(value["body_ratio"], serde_json::json!(0.8));
        assert_eq!(value["direction"], serde_json::json!("bullish"));
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "body_bottom",
                "body_ratio",
                "body_top",
                "close",
                "direction",
                "high",
                "low",
                "open",
                "time_ms",
                "volume",
            ],
            "gap zone JSON must emit exactly the raw flat keys"
        );
        for forbidden in [
            "rssi", "rssi_ma", "trust", "raw", "raw_zone", "candle", "candles", "ema", "ema9",
            "input",
        ] {
            assert!(
                object.get(forbidden).is_none(),
                "gap zone JSON must not contain {forbidden}"
            );
        }
    }

    #[tokio::test]
    async fn candle_records_and_gap_zones_omit_rssi_and_trust() {
        let validated =
            ValidatedTicker::new(&ticker().symbol, ticker().sl_percent, ticker().tol_percent)
                .unwrap();
        let (frame, zones) = presentation::compute_crypto_frame(klines(100.0), validated)
            .await
            .unwrap();
        for zone in zones {
            let value = gap_zone_to_json(&zone);
            assert!(
                value.get("trust").is_none(),
                "gap zone JSON must not carry trust"
            );
        }
        let records = frame.to_json_records().unwrap();
        assert!(
            !records.is_empty(),
            "projected frame must produce candle records"
        );
        for record in &records {
            for forbidden in ["rssi", "rssi_ma", "trust"] {
                assert!(
                    !record.contains_key(forbidden),
                    "candle record must not gain {forbidden}"
                );
            }
        }
        assert!(
            !frame.has_column("rssi") && !frame.has_column("rssi_ma") && !frame.has_column("trust"),
            "projected chart fields must not gain rssi/rssi_ma/trust"
        );
    }

    #[test]
    fn template_gap_bands_use_fixed_fill_and_stroke_opacity() {
        assert!(
            !TDV_HTML_TEMPLATE.contains("z.trust"),
            "renderer must not consume trust"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains("ctx.fillStyle"),
            "renderer must retain fill styling"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains("ctx.strokeStyle"),
            "renderer must retain border styling"
        );
        for color in ["33,150,243", "255,152,0", "158,158,158"] {
            for opacity in ["0.12", "0.4"] {
                let expected = format!("rgba({color},{opacity})");
                assert!(
                    TDV_HTML_TEMPLATE.contains(&expected),
                    "renderer must preserve fixed {opacity} opacity for {color}"
                );
            }
        }
        for preserved in [
            "z.body_top",
            "z.body_bottom",
            "priceToCoordinate",
            "ctx.fillRect",
            "ctx.stroke()",
            "GapZonePrimitive",
            "paneViews()",
            "attached({ series, chart })",
        ] {
            assert!(
                TDV_HTML_TEMPLATE.contains(preserved),
                "renderer must preserve geometry/lifecycle {preserved}"
            );
        }
        assert!(
            !TDV_HTML_TEMPLATE.to_lowercase().contains("rssi"),
            "template must not restore RSSI visuals"
        );
        assert!(
            !TDV_HTML_TEMPLATE.contains("d.trust") && !TDV_HTML_TEMPLATE.contains("d.rssi"),
            "template must not add trust or RSSI candle fields"
        );
    }

    #[test]
    fn template_renders_iching_energy_pane() {
        for field in [
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_nuclear_energy",
        ] {
            assert!(
                TDV_HTML_TEMPLATE.contains(field),
                "template must consume {field}"
            );
            assert!(
                TDV_HTML_TEMPLATE.contains(&format!("value: d.{field}")),
                "template must map {field} from normalized candle time"
            );
        }
        for (label, color) in [
            ("I-Ching Original", "#4FC3F7"),
            ("I-Ching Transformed", "#FFB74D"),
            ("I-Ching Nuclear", "#CE93D8"),
        ] {
            assert!(
                TDV_HTML_TEMPLATE.contains(label),
                "template must label {label}"
            );
            assert_eq!(
                TDV_HTML_TEMPLATE.matches(label).count(),
                1,
                "template must identify {label} in exactly one watermark"
            );
            assert!(
                TDV_HTML_TEMPLATE.contains(color),
                "template must use opaque color {color} for {label}"
            );
        }
        assert!(
            TDV_HTML_TEMPLATE.contains("time: Math.floor(d.time / 1000)"),
            "template must normalize candle time to seconds"
        );
        let pane3_add: Vec<&str> = TDV_HTML_TEMPLATE
            .lines()
            .filter(|line| line.contains("addSeries") && line.contains(", 3)"))
            .collect();
        assert_eq!(
            pane3_add.len(),
            3,
            "pane 3 must contain exactly three series"
        );
        assert!(
            pane3_add
                .iter()
                .all(|line| line.contains("LightweightCharts.LineSeries")),
            "pane 3 must contain only LineSeries"
        );
        assert!(
            !TDV_HTML_TEMPLATE.lines().any(|line| line.contains(", 3)")
                && (line.contains("BaselineSeries")
                    || line.contains("HistogramSeries")
                    || line.contains("AreaSeries"))),
            "pane 3 must not use gradient/baseline/histogram series"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains("chart.panes()[3]"),
            "template must create a watermark for pane 3"
        );
        assert!(
            !TDV_HTML_TEMPLATE.contains("chart.panes()[4]"),
            "template must reindex watermarks to four panes"
        );
    }

    #[test]
    fn template_removes_rssi_sharpe_and_retains_core_panes() {
        let lower = TDV_HTML_TEMPLATE.to_lowercase();
        assert!(
            !lower.contains("rssi"),
            "template must not retain RSSI artifacts"
        );
        assert!(
            !lower.contains("sharpe"),
            "template must not retain Sharpe artifacts"
        );
        assert!(
            !TDV_HTML_TEMPLATE.contains("rssi-bullish")
                && !TDV_HTML_TEMPLATE.contains("rssi-bearish"),
            "template must not retain RSSI tint classes"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains(
                "const neutralRevRsiSeries = chart.addSeries(LightweightCharts.LineSeries, { lineWidth: 6, lineStyle: 2 });"
            ),
            "Reverse RSI neutral overlay must remain in price pane 0"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains(
                "const bullishBandSeries = chart.addSeries(LightweightCharts.LineSeries, { lineWidth: 6 });"
            ),
            "Reverse RSI bullish overlay must remain in price pane 0"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains(
                "const bearishBandSeries = chart.addSeries(LightweightCharts.LineSeries, { lineWidth: 6 });"
            ),
            "Reverse RSI bearish overlay must remain in price pane 0"
        );
        for field in ["neutral_revrsi", "bullish_revrsi", "bearish_revrsi"] {
            assert!(
                TDV_HTML_TEMPLATE.contains(field),
                "template must retain Reverse RSI data {field}"
            );
        }
        assert!(
            TDV_HTML_TEMPLATE.contains(
                "const structurePwrSeries = chart.addSeries(LightweightCharts.HistogramSeries, {}, 1);"
            ),
            "Structure Power must remain in pane 1"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains(
                "const atrRevSeries = chart.addSeries(LightweightCharts.LineSeries, {}, 2);"
            ),
            "ATR Reversion must remain in pane 2"
        );
        let pane1_count = TDV_HTML_TEMPLATE.matches(", 1)").count();
        assert_eq!(
            pane1_count, 3,
            "pane 1 must retain its three Structure series"
        );
        let pane2_count = TDV_HTML_TEMPLATE
            .lines()
            .filter(|line| line.contains("addSeries") && line.contains(", 2)"))
            .count();
        assert_eq!(pane2_count, 1, "pane 2 must retain only ATR Reversion");
    }

    #[test]
    fn template_removes_climax_signal_markers_and_retains_atr_circles() {
        assert!(
            !TDV_HTML_TEMPLATE.contains("climax_signal"),
            "template must not retain orphaned climax_signal marker mapping"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains("const markers = [];"),
            "template must initialize an empty marker collection"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains("markersSeries.setMarkers(markers)"),
            "template must apply only valid markers"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains("shape: 'circle'"),
            "template must retain ATR climax circles"
        );
        assert!(
            TDV_HTML_TEMPLATE.contains("d.atr_upperband")
                && TDV_HTML_TEMPLATE.contains("d.atr_lowerband"),
            "template must retain ATR band climax conditions"
        );
    }
}

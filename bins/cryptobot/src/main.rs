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
use algotrap::ta::gap_zones::OhlcGapZones;
use algotrap::ta::prelude::*;
use algotrap::time_utils::{is_closing_timeframe, next_close_across_tfs};

// ─── Per-Ticker Config ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct TickerConf {
    symbol: String,
    sl_percent: f64,
    tol_percent: f64,
    #[serde(deserialize_with = "deserialize_tfs")]
    tfs: Vec<Timeframe>, // signal-checking TFs
    default_tf: Timeframe,
    #[serde(default, deserialize_with = "deserialize_tfs_opt")]
    ntfy_tf_exclusion: Vec<Timeframe>,
}

// ─── Global Config ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct EnvConf {
    #[serde(deserialize_with = "deserialize_tickers")]
    tickers: Vec<TickerConf>,
    #[serde(deserialize_with = "deserialize_tfs")]
    chart_tfs: Vec<Timeframe>, // shared chart display TFs
    cloudflare_pages_project_name: String,
    ntfy_topic: String,
    ntfy_always: bool,
    #[serde(default = "default_scan_interval")]
    scan_interval_secs: u64,
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
    serde_json::from_str(&s).map_err(serde::de::Error::custom)
}

/// Deserialize comma-separated timeframes (required field).
fn deserialize_tfs<'de, D>(deserializer: D) -> Result<Vec<Timeframe>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.split(',')
        .map(|tf| {
            tf.trim()
                .parse::<Timeframe>()
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

/// Deserialize comma-separated timeframes (optional / can be empty).
fn deserialize_tfs_opt<'de, D>(deserializer: D) -> Result<Vec<Timeframe>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.trim().is_empty() {
        return Ok(vec![]);
    }
    s.split(',')
        .map(|tf| {
            tf.trim()
                .parse::<Timeframe>()
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

fn default_scan_interval() -> u64 {
    900 // 15 minutes
}

fn default_timeout_secs() -> u64 {
    10
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    dotenv().ok();
    let conf: EnvConf = envy::from_env()?;

    // Collect all signal TFs across tickers for scheduling
    let all_signal_tfs: Vec<Timeframe> = conf
        .tickers
        .iter()
        .flat_map(|tc| tc.tfs.iter().copied())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    eprintln!(
        "Starting cryptobot — {} tickers, chart_tfs={:?}, signal_tfs={:?}, max_interval={}s",
        conf.tickers.len(),
        conf.chart_tfs,
        all_signal_tfs,
        conf.scan_interval_secs,
    );
    for tc in &conf.tickers {
        eprintln!(
            "  ticker: {} (signal_tfs={:?}, default_tf={}, ntfy_excl={:?})",
            tc.symbol, tc.tfs, tc.default_tf, tc.ntfy_tf_exclusion,
        );
    }

    // Lead time: run this many seconds BEFORE candle close so data is fresh
    const LEAD_SECS: u64 = 5;

    loop {
        eprintln!("─── Scan cycle start ───");

        match run_cycle(&conf).await {
            Ok(()) => eprintln!("─── Scan cycle complete ───"),
            Err(e) => eprintln!("─── Scan cycle failed: {e:#} ───"),
        }

        // Calculate sleep: align to next candle close across all signal TFs
        let now = chrono::Utc::now();
        let max_interval = Duration::from_secs(conf.scan_interval_secs);
        let sleep_dur = match next_close_across_tfs(&all_signal_tfs, now) {
            Some((secs_until, tf)) => {
                let target = secs_until.saturating_sub(LEAD_SECS);
                let capped = target.min(max_interval.as_secs());
                let dur = Duration::from_secs(capped.max(10)); // floor: 10s minimum
                eprintln!(
                    "Next close: {} in {}s — sleeping {:.0}s (lead={}s)",
                    tf, secs_until, dur.as_secs_f64(), LEAD_SECS,
                );
                dur
            }
            None => {
                eprintln!("No upcoming close found — sleeping {}s", max_interval.as_secs());
                max_interval
            }
        };
        tokio::time::sleep(sleep_dur).await;
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
            Ok(Ok((chart_json, signal_dfs))) => {
                // Write chart data JSON
                let json_path = data_dir.join(format!("{}.json", ticker.symbol));
                tokio::fs::write(&json_path, &chart_json).await?;
                eprintln!("  Wrote {} ({} bytes)", json_path.display(), chart_json.len());

                // Notify
                if let Err(e) = notify_ticker(&signal_dfs, ticker, conf).await {
                    eprintln!("  Notification failed for {}: {e:#}", ticker.symbol);
                }

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

    // Render index.html (no data embedded — data loaded via fetch)
    let tickers_json = serde_json::to_string(&tickers_meta)?;
    let chart_tfs_json = serde_json::to_string(&conf.chart_tfs)?;
    let html = render_tdv_html(&tickers_json, &chart_tfs_json);
    let html_path = output_dir.join("index.html");
    tokio::fs::write(&html_path, &html).await?;
    eprintln!("Wrote {}", html_path.display());

    // Deploy to Cloudflare Pages via wrangler
    deploy_to_cloudflare(output_dir, &conf.cloudflare_pages_project_name)?;

    Ok(())
}

// ─── Per-Ticker Processing ───────────────────────────────────────────────────

async fn process_ticker(
    ticker: &TickerConf,
    chart_tfs: &[Timeframe],
    client: &ext::bingx::BingXClient,
) -> Result<(String, HashMap<Timeframe, DataFrame>), Box<dyn Error + Send + Sync>> {
    // Merge chart_tfs + ticker.tfs (signal TFs) to avoid duplicate fetches
    let all_tfs: HashSet<Timeframe> = chart_tfs
        .iter()
        .chain(ticker.tfs.iter())
        .copied()
        .collect();

    let all_dfs = join_all(
        all_tfs
            .iter()
            .map(|tf| {
                let client = client;
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
    .await
    .into_par_iter()
    .filter_map(|res| match res {
        Ok((tf, klines)) => {
            let df = process_data(klines.as_slice(), ticker).expect("Failed to process data");
            Some((tf, df))
        }
        Err(err) => {
            eprintln!("  Error fetching {}: {err:#?}", ticker.symbol);
            None
        }
    })
    .collect::<HashMap<Timeframe, DataFrame>>();

    // Serialize only chart TFs for the JSON file
    let chart_dfs_serialized: HashMap<String, Value> = all_dfs
        .par_iter()
        .filter(|(tf, _)| chart_tfs.contains(tf))
        .map(|(tf, df)| {
            let df_json: JsonDataframe = df
                .try_into()
                .expect("Failed to serialize data frame to json");
            let df_json: Value = df_json.into();
            (tf.to_string(), df_json)
        })
        .collect();
    let chart_json = serde_json::to_string(&chart_dfs_serialized)?;

    Ok((chart_json, all_dfs))
}

// ─── Notification ────────────────────────────────────────────────────────────

async fn notify_ticker(
    all_dfs: &HashMap<Timeframe, DataFrame>,
    ticker: &TickerConf,
    conf: &EnvConf,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let excluded_tfs: HashSet<_> = ticker.ntfy_tf_exclusion.iter().cloned().collect();
    let signals: HashMap<Timeframe, i32> = all_dfs
        .par_iter()
        .filter(|(tf, _df)| ticker.tfs.contains(tf) && !excluded_tfs.contains(tf))
        .map(|(tf, df)| {
            let second_last_row = df.slice(-2, 1);
            let signal: i32 = second_last_row
                .column("climax_signal")
                .expect("Failed to get signal column")
                .get(0)
                .expect("Cannot get signal from last confirmed candle")
                .extract::<i32>()
                .unwrap();
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
                    || is_closing_timeframe(
                        tf,
                        Utc::now(),
                        Some(Duration::from_secs(conf.timeout_secs)),
                    )
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

    dbg!(&ticker.symbol, &signals, &effective_signals, need_notify, conf.ntfy_always);
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

        let action_url = format!(
            "https://{}.pages.dev/?ticker={}",
            conf.cloudflare_pages_project_name, ticker.symbol
        );
        ntfy::NtfyMessage::default()
            .topic(&conf.ntfy_topic)
            .title(&format!("{} notable movements", &ticker.symbol))
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
            .tags(vec![ticker.symbol.to_string()])
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

// ─── Cloudflare Pages Deploy ─────────────────────────────────────────────────

fn deploy_to_cloudflare(
    output_dir: &std::path::Path,
    project_name: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    const DEPLOY_TIMEOUT: Duration = Duration::from_secs(30);
    eprintln!("Deploying to Cloudflare Pages ({project_name})... (timeout: {}s)", DEPLOY_TIMEOUT.as_secs());

    let mut child = match std::process::Command::new("wrangler")
        .args([
            "pages",
            "deploy",
            output_dir
                .to_str()
                .expect("output_dir must be valid UTF-8"),
            "--project-name",
            project_name,
        ])
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to run wrangler: {e}");
            eprintln!("  Is wrangler installed? (npm install -g wrangler)");
            return Err(e.into());
        }
    };

    // Poll with timeout
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                eprintln!("Deployment successful!");
                return Ok(());
            }
            Ok(Some(status)) => {
                let msg = format!("wrangler exited with status: {status}");
                eprintln!("{msg}");
                return Err(msg.into());
            }
            Ok(None) => {
                if start.elapsed() > DEPLOY_TIMEOUT {
                    eprintln!("Deploy timed out after {}s — killing wrangler", DEPLOY_TIMEOUT.as_secs());
                    let _ = child.kill();
                    let _ = child.wait(); // reap zombie
                    return Err("wrangler deploy timed out".into());
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

// ─── Indicators ──────────────────────────────────────────────────────────────

fn indicators(ticker: &TickerConf) -> Vec<Expr> {
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
    let lvrg_adjust = ticker.sl_percent / (1. + ticker.tol_percent);
    let lvrg = (lit(lvrg_adjust) * ohlc[0].clone() / atr.clone()).alias("leverage");
    let sharpe_ratio = col("close").sharpe(200).alias("sharpe");
    let sharpe_ratio_color = when(sharpe_ratio.clone().gt(lit(0)))
        .then(lit("rgba(76, 175, 79, 0.5)"))
        .otherwise(lit("rgba(242, 54, 70, 0.5)"))
        .alias("sharpe_color");

    // Gap zone detection columns
    let is_atr_gap_col = ohlc.is_atr_gap(42).alias("is_atr_gap");
    let body_ratio_col = ohlc.body_ratio().alias("body_ratio");

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
        is_atr_gap_col,
        body_ratio_col,
    ]
}

fn process_data(klines: &[Kline], ticker: &TickerConf) -> Result<DataFrame, Box<dyn Error>> {
    let df = klines.iter().rev().cloned().to_dataframe().unwrap();
    let df_with_indicators = df.lazy().with_columns(indicators(ticker)).collect().unwrap();
    Ok(df_with_indicators)
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
        #container.rssi-bullish {
            background: rgba(76,175,80,0.05);
        }
        #container.rssi-bearish {
            background: rgba(242,54,69,0.05);
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
                        const yTop = s.priceToCoordinate(z.top);
                        const yBot = s.priceToCoordinate(z.bottom);
                        if (yTop === null || yBot === null) return;
                        const opacity = Math.min(0.25, 0.05 + z.trust * 0.2);
                        const borderOpacity = Math.min(0.5, 0.1 + z.trust * 0.4);
                        ctx.fillStyle = z.direction === 'bullish'
                            ? `rgba(33,150,243,${opacity})`
                            : `rgba(255,152,0,${opacity})`;
                        const x = Math.round(xLeft * ratio);
                        const w = Math.round((xRight - xLeft + 40) * ratio);
                        const y = Math.round(Math.min(yTop, yBot) * vRatio);
                        const h = Math.round(Math.abs(yBot - yTop) * vRatio);
                        ctx.fillRect(x, y, w, h);
                        ctx.strokeStyle = z.direction === 'bullish'
                            ? `rgba(33,150,243,${borderOpacity})`
                            : `rgba(255,152,0,${borderOpacity})`;
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

        // ─── Watermark update ─────────────────────────────────────────
        const watermarkUpdate = () => {
            const tf = tf_btns.value || container.dataset.tf || chartTfs[0];
            const lastBar = dataset[tf]?.slice(-1)[0];
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

        // ─── TF update ────────────────────────────────────────────────
        const onIntervalUpdate = (tf) => {
            if (!dataset[tf]) return;
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
            // ATR Reversion arrows
            data.forEach(d => {
                if (d.atr_reversion_percent > 50 && d.rssi < 46) {
                    markers.push({
                        time: d.time,
                        position: 'belowBar',
                        shape: 'arrowUp',
                        color: 'rgba(150,225,150,0.5)',
                    });
                }
                if (d.atr_reversion_percent < -50 && d.rssi > 54) {
                    markers.push({
                        time: d.time,
                        position: 'aboveBar',
                        shape: 'arrowDown',
                        color: 'rgba(220,80,80,0.5)',
                    });
                }
            });
            markers.sort((a, b) => a.time - b.time);
            markersSeries.setMarkers(markers);

            // ─── Gap Zone Bands ────────────────────────────────────────
            const gapZones = [];
            const MIN_TRUST = 0.3;
            data.forEach(d => {
                if (d.is_atr_gap && d.body_ratio >= MIN_TRUST) {
                    const bottom = Math.min(d.open, d.close);
                    const top = Math.max(d.open, d.close);
                    const bullish = d.close > d.open;
                    const rssiStrength = Math.abs((d.rssi || 50) - 50) / 50;
                    const trust = d.body_ratio * (0.5 + 0.5 * rssiStrength);
                    gapZones.push({ top, bottom, direction: bullish ? 'bullish' : 'bearish', trust });
                }
            });
            const recentGaps = gapZones.slice(-10);
            if (window._gapPrimitive) {
                try { candlestickSeries.detachPrimitive(window._gapPrimitive); } catch(_) {}
            }
            if (recentGaps.length > 0) {
                window._gapPrimitive = new GapZonePrimitive(recentGaps, data);
                candlestickSeries.attachPrimitive(window._gapPrimitive);
            }

            // ─── RSSI Tint ─────────────────────────────────────────────
            const lastRssi = data[data.length - 1]?.rssi || 50;
            container.classList.remove('rssi-bullish', 'rssi-bearish');
            if (lastRssi > 59) container.classList.add('rssi-bullish');
            else if (lastRssi < 41) container.classList.add('rssi-bearish');

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

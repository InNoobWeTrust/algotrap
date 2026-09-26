use core::error::Error;
use core::time::Duration;
use std::collections::{BTreeMap, HashMap};

use chartlib::{
    ChartRegistry, InteractiveDataset, REGISTRY_SCHEMA_VERSION, RegistryEntryMeta,
    render_interactive_html, validate_interactive_dataset,
};
use dotenv::dotenv;
use futures::future::join_all;
use serde::Deserialize;

use algotrap::engine::traits::ComputedFrame;
use algotrap::engine::validation::ValidatedTicker;
use algotrap::ext::bingx::MAX_LIMIT;
use algotrap::prelude::*;
use algotrap::query::gap_zones::GapZoneRecord;
use algotrap::time_utils::next_close_across_tfs;

mod presentation;

#[derive(Debug, Clone, Deserialize)]
struct TickerConf {
    symbol: String,
    sl_percent: f64,
    tol_percent: f64,
    default_tf: Timeframe,
}

#[derive(Debug, Clone, Deserialize)]
struct EnvConf {
    #[serde(deserialize_with = "deserialize_tickers")]
    tickers: Vec<TickerConf>,
    #[serde(deserialize_with = "deserialize_tfs")]
    chart_tfs: Vec<Timeframe>,
    #[serde(default = "default_scan_interval")]
    scan_interval_secs: u64,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

fn deserialize_tickers<'de, D>(deserializer: D) -> Result<Vec<TickerConf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "TICKERS is required and must not be empty",
        ));
    }
    serde_json::from_str(&value).map_err(|error| {
        serde::de::Error::custom(format!("failed to parse TICKERS as JSON: {error}"))
    })
}

fn deserialize_tfs<'de, D>(deserializer: D) -> Result<Vec<Timeframe>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "CHART_TFS is required and must not be empty",
        ));
    }
    value
        .split(',')
        .map(|timeframe| timeframe.trim().parse().map_err(serde::de::Error::custom))
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
    900
}

fn default_timeout_secs() -> u64 {
    10
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    dotenv().ok();
    require_non_empty_env("TICKERS")?;
    require_non_empty_env("CHART_TFS")?;
    let conf: EnvConf = envy::from_env()?;
    let loop_mode = std::env::args().any(|argument| argument == "--loop");

    loop {
        if let Err(error) = run_cycle(&conf).await {
            eprintln!("Cryptobot scan cycle failed: {error:#}");
        }
        if !loop_mode {
            return Ok(());
        }
        let maximum = Duration::from_secs(conf.scan_interval_secs);
        let delay = next_close_across_tfs(&conf.chart_tfs, chrono::Utc::now())
            .map(|(seconds, _)| Duration::from_secs(seconds.saturating_sub(5).max(10)))
            .unwrap_or(maximum)
            .min(maximum);
        tokio::time::sleep(delay).await;
    }
}

struct InteractivePublication {
    registry: ChartRegistry,
    datasets: BTreeMap<String, serde_json::Value>,
}

async fn run_cycle(conf: &EnvConf) -> Result<(), Box<dyn Error + Send + Sync>> {
    let client = ext::bingx::BingXClient::default();
    let timeout = Duration::from_secs(conf.timeout_secs);
    let mut candidates = Vec::new();

    for ticker in &conf.tickers {
        eprintln!("Processing {}...", ticker.symbol);
        match tokio::time::timeout(
            timeout * conf.chart_tfs.len() as u32,
            process_ticker(ticker, &conf.chart_tfs, &client),
        )
        .await
        {
            Ok(Ok(datasets)) if !datasets.is_empty() => {
                candidates.push((ticker_meta(ticker), datasets))
            }
            Ok(Ok(_)) => eprintln!("  No usable chart data for {}", ticker.symbol),
            Ok(Err(error)) => eprintln!("  Error processing {}: {error:#}", ticker.symbol),
            Err(_) => eprintln!("  Timeout processing {}", ticker.symbol),
        }
    }

    let chart_tfs = conf.chart_tfs.iter().map(ToString::to_string).collect();
    let publication = build_interactive_publication(candidates, chart_tfs)?;
    write_interactive_publication(std::path::Path::new("output"), &publication).await
}

fn ticker_meta(ticker: &TickerConf) -> RegistryEntryMeta {
    RegistryEntryMeta {
        symbol: ticker.symbol.clone(),
        sl_percent: format!("{:.0}", ticker.sl_percent * 100.0),
        tol_percent: format!("{:.2}", ticker.tol_percent * 100.0),
        default_tf: ticker.default_tf.to_string(),
    }
}

async fn process_ticker(
    ticker: &TickerConf,
    chart_tfs: &[Timeframe],
    client: &ext::bingx::BingXClient,
) -> Result<Vec<InteractiveDataset>, Box<dyn Error + Send + Sync>> {
    let mut ordered = Vec::with_capacity(chart_tfs.len());
    for timeframe in chart_tfs {
        if !ordered.contains(timeframe) {
            ordered.push(*timeframe);
        }
    }
    let fetched = join_all(ordered.iter().map(|timeframe| {
        let symbol = ticker.symbol.clone();
        async move {
            client
                .get_futures_klines(&symbol, &timeframe.to_string(), MAX_LIMIT)
                .await
                .map(|klines| (*timeframe, klines))
        }
    }))
    .await
    .into_iter()
    .filter_map(|result| match result {
        Ok(frame) => Some(frame),
        Err(error) => {
            eprintln!("  Error fetching {}: {error:#?}", ticker.symbol);
            None
        }
    })
    .collect();
    let mut frames = compute_crypto_frames(fetched, ticker).await;
    let display_symbol = format!("BingX:{}", ticker.symbol);

    Ok(ordered
        .into_iter()
        .filter_map(|timeframe| {
            let (frame, zones) = frames.remove(&timeframe)?;
            match presentation::adapt_chartlib_dataset(
                &ticker.symbol,
                &timeframe.to_string(),
                &display_symbol,
                frame.as_ref(),
                &zones,
            ) {
                Ok(dataset) => Some(dataset),
                Err(error) => {
                    eprintln!(
                        "  Error adapting {} {}: {error:#}",
                        ticker.symbol, timeframe
                    );
                    None
                }
            }
        })
        .collect())
}

async fn compute_crypto_frames(
    fetched: Vec<(Timeframe, Vec<Kline>)>,
    ticker: &TickerConf,
) -> HashMap<Timeframe, (Box<dyn ComputedFrame>, Vec<GapZoneRecord>)> {
    let validated =
        match ValidatedTicker::new(&ticker.symbol, ticker.sl_percent, ticker.tol_percent) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("  Invalid ticker {}: {error:#}", ticker.symbol);
                return HashMap::new();
            }
        };
    join_all(fetched.into_iter().map(|(timeframe, klines)| {
        let validated = validated.clone();
        async move {
            (
                timeframe,
                presentation::compute_crypto_frame(klines, validated).await,
            )
        }
    }))
    .await
    .into_iter()
    .filter_map(|(timeframe, result)| match result {
        Ok(frame) => Some((timeframe, frame)),
        Err(error) => {
            eprintln!(
                "  Error computing {} {}: {error:#}",
                ticker.symbol, timeframe
            );
            None
        }
    })
    .collect()
}

fn build_interactive_publication(
    candidates: Vec<(RegistryEntryMeta, Vec<InteractiveDataset>)>,
    chart_tfs: Vec<String>,
) -> Result<InteractivePublication, Box<dyn Error + Send + Sync>> {
    let mut tickers = Vec::new();
    let mut datasets = BTreeMap::new();
    for (ticker, datasets_for_ticker) in candidates {
        let mut timeframes = BTreeMap::new();
        for dataset in datasets_for_ticker {
            validate_interactive_dataset(&dataset)?;
            let payload = serde_json::json!({
                "candles": dataset.records,
                "gapZones": dataset.gap_zones.iter().map(chartlib::gap_zone_to_json).collect::<Vec<_>>(),
            });
            timeframes.insert(dataset.key.timeframe, payload);
        }
        if !timeframes.is_empty() {
            datasets.insert(ticker.symbol.clone(), serde_json::to_value(timeframes)?);
            tickers.push(ticker);
        }
    }
    if tickers.is_empty() {
        return Err("no valid chart datasets to publish".into());
    }
    Ok(InteractivePublication {
        registry: ChartRegistry {
            schema_version: REGISTRY_SCHEMA_VERSION,
            tickers: serde_json::to_value(tickers)?,
            chart_tfs: serde_json::to_value(chart_tfs)?,
        },
        datasets,
    })
}

async fn write_interactive_publication(
    output_dir: &std::path::Path,
    publication: &InteractivePublication,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let data_dir = output_dir.join("data");
    tokio::fs::create_dir_all(&data_dir).await?;
    for (symbol, dataset) in &publication.datasets {
        let path = data_dir.join(format!("{symbol}.json"));
        tokio::fs::write(&path, serde_json::to_string(dataset)?).await?;
        eprintln!("  Wrote {}", path.display());
    }
    let html = render_interactive_html(&publication.registry)?;
    let index = output_dir.join("index.html");
    tokio::fs::write(&index, html).await?;
    eprintln!("Wrote {}", index.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chartlib::{ChartRecord, DatasetKey, GapDirection, GapZone};
    use serde_json::Value;

    fn dataset(timeframe: &str) -> InteractiveDataset {
        InteractiveDataset {
            key: DatasetKey::new("BTC-USDT", timeframe),
            display_symbol: String::from("BingX:BTC-USDT"),
            records: vec![ChartRecord::from_iter([
                (String::from("time"), Value::from(1_700_000_000_000_i64)),
                (String::from("open"), Value::from(100.0)),
                (String::from("high"), Value::from(101.0)),
                (String::from("low"), Value::from(99.0)),
                (String::from("close"), Value::from(100.5)),
                (String::from("volume"), Value::from(1_000.0)),
            ])],
            gap_zones: vec![GapZone {
                time_ms: 1_700_000_000_000,
                open: 100.0,
                high: 101.0,
                low: 99.0,
                close: 100.5,
                volume: 1_000.0,
                body_bottom: 100.0,
                body_top: 100.5,
                body_ratio: Some(0.5),
                direction: GapDirection::Bullish,
            }],
        }
    }

    #[tokio::test]
    async fn publication_uses_canonical_grouped_data_layout_and_template() {
        let publication = build_interactive_publication(
            vec![(
                RegistryEntryMeta {
                    symbol: String::from("BTC-USDT"),
                    sl_percent: String::from("2"),
                    tol_percent: String::from("1.00"),
                    default_tf: String::from("1h"),
                },
                vec![dataset("1h"), dataset("4h")],
            )],
            vec![String::from("1h"), String::from("4h")],
        )
        .expect("publication");
        let root =
            std::env::temp_dir().join(format!("chartlib-publication-{}", std::process::id()));
        if root.exists() {
            tokio::fs::remove_dir_all(&root)
                .await
                .expect("clean prior output");
        }
        write_interactive_publication(&root, &publication)
            .await
            .expect("write publication");
        let html = tokio::fs::read_to_string(root.join("index.html"))
            .await
            .expect("read index");
        assert!(html.contains("LightweightCharts.createChart"));
        assert!(html.contains("chart.panes()[3]"));
        assert!(html.contains("GapZonePrimitive"));
        assert!(html.contains("#4FC3F7"));
        let json = tokio::fs::read_to_string(root.join("data/BTC-USDT.json"))
            .await
            .expect("read data");
        let data: serde_json::Value = serde_json::from_str(&json).expect("parse data");
        assert!(data["1h"]["candles"].is_array());
        assert!(data["1h"]["gapZones"].is_array());
        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove output");
    }
}

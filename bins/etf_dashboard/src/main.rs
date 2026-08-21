use algotrap::ext::{webdriver::*, yfinance::*};
use algotrap::prelude::Kline;
use chrono::{NaiveDate, TimeZone, Utc};
use core::error::Error;
use fantoccini::Locator;
use minijinja::render;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::IsTerminal;
use std::path::Path;
use tracing::{info, warn};
use tracing_subscriber::prelude::*;

type EtfRows = Vec<Map<String, Value>>;

#[derive(Debug, Clone, PartialEq)]
struct EtfFlowRow {
    date: NaiveDate,
    flows: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq)]
struct DashboardDatasets {
    price: Vec<Value>,
    volume: Vec<Value>,
    netflow: Vec<Value>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    setup_tracing();
    for (ticker, url, script) in [
        (BTC_TICKER, ETF_BTC_URL, ETF_BTC_EXTRACT_SCRIPT),
        (ETH_TICKER, ETF_ETH_URL, ETF_ETH_EXTRACT_SCRIPT),
        (SOL_TICKER, ETF_SOL_URL, ETF_SOL_EXTRACT_SCRIPT),
    ] {
        let (raw_rows, start_timestamp, end_timestamp) = get_etf_data(url, script).await?;
        let flows = normalize_etf_rows(raw_rows)?;
        let funds = fund_columns(&flows);
        let client = YfinanceClient::new();
        let asset_klines = client
            .get_quote_history(ticker, start_timestamp, end_timestamp, YfinanceInterval::D1)
            .await?;
        let mut fund_histories = HashMap::new();

        for fund in &funds {
            match client
                .get_quote_history(fund, start_timestamp, end_timestamp, YfinanceInterval::D1)
                .await
            {
                Ok(klines) if !klines.is_empty() => {
                    fund_histories.insert(fund.clone(), klines);
                }
                Ok(_) => warn!(ticker = %fund, "Fund history was empty"),
                Err(error) => warn!(ticker = %fund, "Could not fetch fund history: {error}"),
            }
        }

        let datasets = build_dashboard_datasets(&flows, &asset_klines, &fund_histories);
        write_dashboard_artifact(ticker, &datasets)?;
        info!(
            ticker,
            flow_rows = flows.len(),
            price_rows = datasets.price.len(),
            "Wrote ETF dashboard"
        );
    }
    Ok(())
}

/// Fetches one ETF flow table and derives its inclusive UTC date range.
async fn get_etf_data(
    url: &str,
    extract_script: &str,
) -> Result<(EtfRows, i64, i64), Box<dyn Error + Send + Sync>> {
    let geckodriver = GeckoDriver::default_with_log(Path::new("geckodriver.log"))?;
    let client = geckodriver.create_client(false).await?;
    client.goto(url).await?;
    let element = client.find(Locator::Css("table.etf")).await?;
    let rows = validate_etf_rows(
        client
            .extract_table(&element, Some(extract_script.to_owned()))
            .await?,
    )?;
    let normalized = normalize_etf_rows(rows.clone())?;
    let start = normalized
        .first()
        .ok_or("ETF table has no dated rows")?
        .date;
    let end = normalized.last().ok_or("ETF table has no dated rows")?.date;
    client.close().await?;
    Ok((rows, date_timestamp(start), date_timestamp(end)))
}

/// Validates browser-extracted ETF rows before normalization.
fn validate_etf_rows(rows: EtfRows) -> Result<EtfRows, Box<dyn Error + Send + Sync>> {
    if rows.is_empty() {
        return Err("ETF table has no rows".into());
    }
    for (index, row) in rows.iter().enumerate() {
        let date = row
            .get("Date")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("ETF row {index} has no Date string"))?;
        NaiveDate::parse_from_str(date.trim(), "%d %b %Y")
            .map_err(|error| format!("ETF row {index} has invalid Date {date:?}: {error}"))?;
    }
    Ok(rows)
}

/// Parses browser cells, sorts dates ascending, and combines duplicate dated observations.
fn normalize_etf_rows(rows: EtfRows) -> Result<Vec<EtfFlowRow>, Box<dyn Error + Send + Sync>> {
    let mut by_date = BTreeMap::<NaiveDate, BTreeMap<String, f64>>::new();
    for (index, row) in rows.into_iter().enumerate() {
        let date_cell = row
            .get("Date")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("ETF row {index} has no Date string"))?;
        let date = NaiveDate::parse_from_str(date_cell.trim(), "%d %b %Y")?;
        let daily = by_date.entry(date).or_default();
        for (fund, cell) in row {
            if fund == "Date" || fund == "Total" {
                continue;
            }
            if let Some(flow) = parse_flow_cell(&cell)? {
                *daily.entry(fund).or_default() += flow;
            }
        }
    }
    Ok(by_date
        .into_iter()
        .map(|(date, flows)| EtfFlowRow { date, flows })
        .collect())
}

fn parse_flow_cell(cell: &Value) -> Result<Option<f64>, Box<dyn Error + Send + Sync>> {
    let value = match cell {
        Value::Null => return Ok(None),
        Value::Number(value) => value
            .as_f64()
            .ok_or("ETF flow number is not representable as f64")?,
        Value::String(value) => {
            let cleaned = value.trim().replace(',', "");
            if cleaned.is_empty() || cleaned == "-" {
                return Ok(None);
            }
            cleaned.parse::<f64>()?
        }
        _ => return Err("ETF flow cell must be a number, string, or null".into()),
    };
    if value.is_finite() {
        Ok(Some(value))
    } else {
        Err("ETF flow cell must be finite".into())
    }
}

fn fund_columns(rows: &[EtfFlowRow]) -> Vec<String> {
    rows.iter()
        .flat_map(|row| row.flows.keys().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Builds the price, total-volume, and net-flow datasets expected by the dashboard artifact.
fn build_dashboard_datasets(
    flows: &[EtfFlowRow],
    asset_history: &[Kline],
    fund_histories: &HashMap<String, Vec<Kline>>,
) -> DashboardDatasets {
    let funds = fund_columns(flows);
    let prices = asset_history
        .iter()
        .map(|kline| (kline_date(kline), kline.close))
        .collect::<BTreeMap<_, _>>();
    let volumes = fund_histories
        .iter()
        .flat_map(|(fund, klines)| {
            klines
                .iter()
                .map(|kline| ((kline_date(kline), fund.clone()), kline.volume))
        })
        .collect::<HashMap<_, _>>();
    let mut cumulative_total = 0.0;
    let mut cumulative_by_fund = BTreeMap::<String, f64>::new();
    let mut netflow_total_history = Vec::new();
    let mut volume_total_history = Vec::new();
    let mut price = Vec::new();
    let mut volume = Vec::new();
    let mut netflow = Vec::new();

    for row in flows {
        let daily_total = funds
            .iter()
            .map(|fund| row.flows.get(fund).copied().unwrap_or(0.0))
            .sum::<f64>();
        cumulative_total += daily_total;
        netflow_total_history.push(daily_total);
        let volume_total = funds
            .iter()
            .map(|fund| {
                volumes
                    .get(&(row.date, fund.clone()))
                    .copied()
                    .unwrap_or(0.0)
            })
            .sum::<f64>();
        volume_total_history.push(volume_total);
        let mut record = Map::new();
        record.insert("time".into(), json!(date_timestamp(row.date)));
        record.insert("netflow_total".into(), json!(daily_total));
        record.insert(
            "netflow_total_ma20".into(),
            optional_json(trailing_average(&netflow_total_history, 20)),
        );
        record.insert("cumulative_netflow_total".into(), json!(cumulative_total));
        for fund in &funds {
            let flow = row.flows.get(fund).copied().unwrap_or(0.0);
            let cumulative = cumulative_by_fund.entry(fund.clone()).or_default();
            *cumulative += flow;
            record.insert(fund.clone(), json!(flow));
            record.insert(format!("cumulative_netflow_{fund}"), json!(*cumulative));
        }
        netflow.push(Value::Object(record));
        volume.push(json!({"time": date_timestamp(row.date), "value": volume_total, "ma20": trailing_average(&volume_total_history, 20)}));
        if let Some(close) = prices.get(&row.date) {
            price.push(json!({"time": date_timestamp(row.date), "value": close}));
        }
    }
    DashboardDatasets {
        price,
        volume,
        netflow,
    }
}

fn trailing_average(values: &[f64], period: usize) -> Option<f64> {
    (values.len() >= period)
        .then(|| values[values.len() - period..].iter().sum::<f64>() / period as f64)
}

fn optional_json(value: Option<f64>) -> Value {
    value.map_or(Value::Null, |value| json!(value))
}

fn kline_date(kline: &Kline) -> NaiveDate {
    Utc.timestamp_opt(kline.time, 0)
        .single()
        .expect("valid yfinance timestamp")
        .date_naive()
}

fn date_timestamp(date: NaiveDate) -> i64 {
    date.and_hms_opt(0, 0, 0)
        .expect("midnight is valid")
        .and_utc()
        .timestamp()
}

fn write_dashboard_artifact(
    symbol: &str,
    datasets: &DashboardDatasets,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    std::fs::create_dir_all("output")?;
    std::fs::write(
        format!("output/{symbol}.html"),
        render_tdv_html(symbol, datasets)?,
    )?;
    Ok(())
}

fn render_tdv_html(
    symbol: &str,
    datasets: &DashboardDatasets,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let datasets = json!({
        "price": datasets.price,
        "volume": datasets.volume,
        "netflow": datasets.netflow,
    });
    Ok(
        render!(TDV_HTML_TEMPLATE, symbol => symbol, datasets => serde_json::to_string(&datasets)?)
            .trim()
            .to_string(),
    )
}

fn setup_tracing() {
    let subscriber = tracing_subscriber::Registry::default()
        .with(
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
    tracing::subscriber::set_global_default(subscriber)
        .expect("tracing subscriber must only be initialized once");
}

const BTC_TICKER: &str = "BTC-USD";
const ETH_TICKER: &str = "ETH-USD";
const SOL_TICKER: &str = "SOL-USD";
const ETF_BTC_URL: &str = "https://farside.co.uk/bitcoin-etf-flow-all-data/";
const ETF_ETH_URL: &str = "https://farside.co.uk/ethereum-etf-flow-all-data/";
const ETF_SOL_URL: &str = "https://farside.co.uk/sol/";
const ETF_BTC_EXTRACT_SCRIPT: &str = r#"const table = arguments[0]; const rows = [...table.rows]; const headerIndex = rows.findIndex(row => [...row.cells].some(cell => cell.innerText.trim() === 'Date')); if (headerIndex < 0) throw new Error('ETF table has no Date header'); const headers = [...rows[headerIndex].cells].map(cell => cell.innerText.trim()).filter(Boolean); return rows.slice(headerIndex + 1).map(row => [...row.cells].map(cell => cell.innerText.trim())).filter(cells => cells.length >= headers.length && /^\d{1,2}\s+[A-Za-z]{3}\s+\d{4}$/.test(cells[0])).map(cells => Object.fromEntries(headers.map((header, index) => [header, cells[index] === '-' ? null : cells[index]))));"#;
const ETF_ETH_EXTRACT_SCRIPT: &str = ETF_BTC_EXTRACT_SCRIPT;
const ETF_SOL_EXTRACT_SCRIPT: &str = ETF_BTC_EXTRACT_SCRIPT;

const TDV_HTML_TEMPLATE: &str = r#"<!doctype html><html><head><meta charset=\"utf-8\"><title>{{ symbol }} ETF flows</title></head><body><h1>{{ symbol }} ETF flows</h1><script id=\"dashboard-datasets\" type=\"application/json\">{{ datasets }}</script><script>const datasets=JSON.parse(document.getElementById('dashboard-datasets').textContent);</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn kline(time: i64, close: f64, volume: f64) -> Kline {
        Kline {
            open: close,
            high: close,
            low: close,
            close,
            volume,
            time,
            adjclose: None,
        }
    }
    #[test]
    fn normalization_sorts_combines_duplicates_and_preserves_missing_as_zero_later() {
        let rows = serde_json::from_value(json!([{"Date":"03 Jan 2026","IBIT":"1,200.5","FBTC":null},{"Date":"02 Jan 2026","IBIT":"-2","FBTC":"3"},{"Date":"03 Jan 2026","IBIT":"4.5"}])).unwrap();
        let normalized = normalize_etf_rows(rows).unwrap();
        assert_eq!(
            normalized.iter().map(|row| row.date).collect::<Vec<_>>(),
            vec![
                NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
                NaiveDate::from_ymd_opt(2026, 1, 3).unwrap()
            ]
        );
        assert_eq!(normalized[1].flows["IBIT"], 1205.0);
        assert!(!normalized[1].flows.contains_key("FBTC"));
    }
    #[test]
    fn dashboard_datasets_join_history_and_compute_features() {
        let rows = normalize_etf_rows(serde_json::from_value(json!([{"Date":"02 Jan 2026","IBIT":"10","FBTC":"-2"},{"Date":"03 Jan 2026","IBIT":null,"FBTC":"4"}])).unwrap()).unwrap();
        let jan_2 = date_timestamp(NaiveDate::from_ymd_opt(2026, 1, 2).unwrap());
        let jan_3 = date_timestamp(NaiveDate::from_ymd_opt(2026, 1, 3).unwrap());
        let datasets = build_dashboard_datasets(
            &rows,
            &[kline(jan_2, 100.0, 0.0)],
            &HashMap::from([
                (
                    "IBIT".into(),
                    vec![kline(jan_2, 0.0, 50.0), kline(jan_3, 0.0, 60.0)],
                ),
                ("FBTC".into(), vec![kline(jan_2, 0.0, 30.0)]),
            ]),
        );
        assert_eq!(datasets.price, vec![json!({"time":jan_2,"value":100.0})]);
        assert_eq!(datasets.volume[0]["value"], 80.0);
        assert_eq!(datasets.volume[1]["value"], 60.0);
        assert_eq!(datasets.netflow[1]["netflow_total"], 4.0);
        assert_eq!(datasets.netflow[1]["cumulative_netflow_total"], 12.0);
        assert!(datasets.netflow[1]["netflow_total_ma20"].is_null());
    }
    #[test]
    fn rendered_artifact_contains_constructed_datasets() {
        let datasets = DashboardDatasets {
            price: vec![json!({"time": 1, "value": 2})],
            volume: vec![],
            netflow: vec![json!({"netflow_total": 3})],
        };
        let html = render_tdv_html("BTC-USD", &datasets).unwrap();
        assert!(html.contains("BTC-USD ETF flows"));
        assert!(html.contains("netflow_total"));
        assert!(html.contains("dashboard-datasets"));
    }
}

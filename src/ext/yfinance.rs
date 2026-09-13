use crate::model::Kline;
use core::error::Error;
use core::fmt::Display;
use reqwest::Url;
use serde_json::Value;
use serde_json::json;

const YFINANCE_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const YFINANCE_COOKIE_URL: &str = "https://fc.yahoo.com";
const YFINANCE_CRUMB_URL: &str = "https://query1.finance.yahoo.com/v1/test/getcrumb";
/// Yahoo Finance chart-history API base URL.
pub const YFINANCE_API_HISTORY: &str = "https://query2.finance.yahoo.com/v8/finance/chart/";

/// Supported Yahoo Finance history intervals.
#[derive(Debug, Clone, Copy)]
pub enum YfinanceInterval {
    //"1d","5d","1mo","3mo","6mo","1y","2y","5y","10y","ytd","max"
    /// One day.
    D1,
    /// Five days.
    D5,
    /// One month.
    Mo1,
    /// Three months.
    Mo3,
    /// Six months.
    Mo6,
    /// One year.
    Y1,
    /// Two years.
    Y2,
    /// Five years.
    Y5,
    /// Ten years.
    Y10,
    /// Year to date.
    Ytd,
    /// All available history.
    Max,
}

impl Display for YfinanceInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        let s = match self {
            YfinanceInterval::D1 => "1d",
            YfinanceInterval::D5 => "5d",
            YfinanceInterval::Mo1 => "1mo",
            YfinanceInterval::Mo3 => "3mo",
            YfinanceInterval::Mo6 => "6mo",
            YfinanceInterval::Y1 => "1y",
            YfinanceInterval::Y2 => "2y",
            YfinanceInterval::Y5 => "5y",
            YfinanceInterval::Y10 => "10y",
            YfinanceInterval::Ytd => "ytd",
            YfinanceInterval::Max => "max",
        };
        write!(f, "{s}")
    }
}

/// Warning: For research purpose, only use this for backtesting on historical data
#[derive(Clone)]
pub struct YfinanceClient {
    client: reqwest::Client,
}

impl Default for YfinanceClient {
    fn default() -> Self {
        let client = match reqwest::ClientBuilder::new()
            .cookie_store(true)
            .user_agent(YFINANCE_USER_AGENT)
            .build()
        {
            Ok(client) => client,
            Err(_) => reqwest::Client::new(),
        };
        Self { client }
    }
}

fn require_array<'a>(
    value: &'a Value,
    name: &str,
) -> Result<&'a Vec<Value>, Box<dyn Error + Send + Sync>> {
    match value.as_array() {
        Some(array) => Ok(array),
        None => Err(format!("yfinance: missing or non-array {name}").into()),
    }
}

fn finite_f64(value: &Value) -> Option<f64> {
    match value.as_f64() {
        Some(v) if v.is_finite() => Some(v),
        _ => None,
    }
}

fn timestamp_i64(value: &Value) -> Option<i64> {
    if let Some(v) = value.as_i64() {
        Some(v)
    } else if let Some(u) = value.as_u64() {
        i64::try_from(u).ok()
    } else {
        None
    }
}

fn chart_data_to_klines(chart_data: &Value) -> Result<Vec<Kline>, Box<dyn Error + Send + Sync>> {
    let timestamps = require_array(&chart_data["timestamp"], "timestamp")?;
    let quote = &chart_data["indicators"]["quote"][0];
    match quote {
        Value::Object(_) => {}
        _ => return Err("yfinance: missing indicators.quote[0]".into()),
    }
    let open = require_array(&quote["open"], "quote.open")?;
    let high = require_array(&quote["high"], "quote.high")?;
    let low = require_array(&quote["low"], "quote.low")?;
    let close = require_array(&quote["close"], "quote.close")?;
    let volume = require_array(&quote["volume"], "quote.volume")?;
    let adjclose_values: Option<&Vec<Value>> = match &chart_data["indicators"]["adjclose"][0]["adjclose"]
    {
        Value::Null => None,
        Value::Array(values) => Some(values),
        _ => return Err("yfinance: adjclose is non-array".into()),
    };

    let len = timestamps.len();
    if open.len() != len
        || high.len() != len
        || low.len() != len
        || close.len() != len
        || volume.len() != len
    {
        return Err(format!(
            "yfinance: length mismatch timestamp={} open={} high={} low={} close={} volume={}",
            len,
            open.len(),
            high.len(),
            low.len(),
            close.len(),
            volume.len()
        )
        .into());
    }
    if let Some(adjclose) = adjclose_values
        && adjclose.len() != len
    {
        return Err(format!(
            "yfinance: length mismatch timestamp={} adjclose={}",
            len,
            adjclose.len()
        )
        .into());
    }

    let mut klines = Vec::new();
    for (i, timestamp_value) in timestamps.iter().enumerate() {
        let time = match timestamp_i64(timestamp_value) {
            Some(time) => time,
            None => continue,
        };
        let open_value = match finite_f64(&open[i]) {
            Some(value) => value,
            None => continue,
        };
        let high_value = match finite_f64(&high[i]) {
            Some(value) => value,
            None => continue,
        };
        let low_value = match finite_f64(&low[i]) {
            Some(value) => value,
            None => continue,
        };
        let close_value = match finite_f64(&close[i]) {
            Some(value) => value,
            None => continue,
        };
        let volume_value = match finite_f64(&volume[i]) {
            Some(value) => value,
            None => continue,
        };
        let adjclose = match adjclose_values {
            Some(values) => finite_f64(&values[i]),
            None => None,
        };
        klines.push(Kline {
            open: open_value,
            high: high_value,
            low: low_value,
            close: close_value,
            volume: volume_value,
            time,
            adjclose,
        });
    }

    Ok(klines)
}

fn parse_chart_response(json_resp: &Value) -> Result<Vec<Kline>, Box<dyn Error + Send + Sync>> {
    if json_resp["chart"]["error"] != json!(null) {
        return Err(format!("yfinance: chart error {}", json_resp["chart"]["error"]).into());
    }
    let result = &json_resp["chart"]["result"];
    let chart_data = match result.as_array() {
        Some(results) => match results.first() {
            Some(chart_data) => chart_data,
            None => return Err("yfinance: empty chart.result".into()),
        },
        None => return Err("yfinance: missing or non-array chart.result".into()),
    };
    chart_data_to_klines(chart_data)
}

impl YfinanceClient {
    /// Creates a Yahoo Finance history client.
    pub fn new() -> Self {
        Self::default()
    }

    // Fetch history
    /// Fetches historical klines for a ticker and time range.
    pub async fn get_quote_history(
        &self,
        ticker: &str,
        period1: i64,
        period2: i64,
        interval: YfinanceInterval,
    ) -> Result<Vec<Kline>, Box<dyn Error + Send + Sync>> {
        // Get cookie first
        self.client.get(YFINANCE_COOKIE_URL).send().await?;
        // Get crumb
        self.client.get(YFINANCE_CRUMB_URL).send().await?;

        let url_str = YFINANCE_API_HISTORY.to_string() + ticker;

        let params_vec = vec![
            ("period1", period1.to_string()),
            ("period2", period2.to_string()),
            ("interval", interval.to_string()),
        ];

        let url = Url::parse_with_params(&url_str, params_vec)?;

        let response = self.client.get(url).send().await?;
        if response.status() != 200 {
            return Err(format!("{response:#?}").into());
        }
        let json_resp = response.json::<serde_json::Value>().await?;

        parse_chart_response(&json_resp)
    }
}

#[cfg(test)]
#[allow(clippy::manual_unwrap_or_default, clippy::assertions_on_constants)]
mod tests {
    use super::chart_data_to_klines;
    use super::parse_chart_response;
    use serde_json::Value;
    use serde_json::json;

    fn valid_chart_data() -> Value {
        json!({
            "timestamp": [1_700_000_000, 1_700_008_600, 1_700_017_200],
            "indicators": {
                "quote": [{
                    "open": [10.0, 11.0, 12.0],
                    "high": [10.5, 11.5, 12.5],
                    "low": [9.5, 10.5, 11.5],
                    "close": [10.2, 11.2, 12.2],
                    "volume": [100.0, 200.0, 300.0]
                }],
                "adjclose": [{
                    "adjclose": [10.1, 11.1, 12.1]
                }]
            }
        })
    }

    fn response_with(chart_data: Value) -> Value {
        json!({
            "chart": {
                "result": [chart_data],
                "error": null
            }
        })
    }

    #[test]
    fn complete_response_parses_all_rows_in_order() {
        let json_resp = response_with(valid_chart_data());
        let result = parse_chart_response(&json_resp);
        assert!(result.is_ok());
        let klines = match result {
            Ok(klines) => klines,
            Err(_) => Vec::new(),
        };
        assert_eq!(klines.len(), 3);
        assert_eq!(klines[0].time, 1_700_000_000);
        assert_eq!(klines[1].time, 1_700_008_600);
        assert_eq!(klines[2].time, 1_700_017_200);
        assert_eq!(klines[0].open, 10.0);
        assert_eq!(klines[1].high, 11.5);
        assert_eq!(klines[2].low, 11.5);
        assert_eq!(klines[2].close, 12.2);
        assert_eq!(klines[2].volume, 300.0);
        assert_eq!(klines[0].adjclose, Some(10.1));
        assert_eq!(klines[1].adjclose, Some(11.1));
        assert_eq!(klines[2].adjclose, Some(12.1));
    }

    #[test]
    fn each_required_null_skips_same_row_without_shifting() {
        let required_paths: [(&str, &str); 6] = [
            ("timestamp", "timestamp"),
            ("open", "quote.open"),
            ("high", "quote.high"),
            ("low", "quote.low"),
            ("close", "quote.close"),
            ("volume", "quote.volume"),
        ];
        for (field, _) in required_paths {
            let mut chart_data = valid_chart_data();
            if field == "timestamp" {
                chart_data["timestamp"][1] = Value::Null;
            } else {
                chart_data["indicators"]["quote"][0][field][1] = Value::Null;
            }
            let result = chart_data_to_klines(&chart_data);
            assert!(result.is_ok(), "field {field} should Ok with skipped row");
            let klines = match result {
                Ok(klines) => klines,
                Err(_) => Vec::new(),
            };
            assert_eq!(klines.len(), 2, "field {field} null must skip one row");
            assert_eq!(klines[0].time, 1_700_000_000, "field {field}");
            assert_eq!(
                klines[1].time, 1_700_017_200,
                "field {field} must not shift indices"
            );
        }
    }

    #[test]
    fn each_required_non_numeric_skips_same_row() {
        let mut chart_data = valid_chart_data();
        chart_data["indicators"]["quote"][0]["close"][1] = json!("bad");
        let klines = match chart_data_to_klines(&chart_data) {
            Ok(klines) => klines,
            Err(_) => Vec::new(),
        };
        assert_eq!(klines.len(), 2);
        assert_eq!(klines[0].time, 1_700_000_000);
        assert_eq!(klines[1].time, 1_700_017_200);
    }

    #[test]
    fn adjclose_null_retained_with_none() {
        let mut chart_data = valid_chart_data();
        chart_data["indicators"]["adjclose"][0]["adjclose"][1] = Value::Null;
        let result = chart_data_to_klines(&chart_data);
        assert!(result.is_ok());
        let klines = match result {
            Ok(klines) => klines,
            Err(_) => Vec::new(),
        };
        assert_eq!(klines.len(), 3);
        assert_eq!(klines[0].adjclose, Some(10.1));
        assert_eq!(klines[1].adjclose, None);
        assert_eq!(klines[2].adjclose, Some(12.1));
        assert_eq!(klines[1].time, 1_700_008_600);
        assert_eq!(klines[1].close, 11.2);
    }

    #[test]
    fn missing_whole_adjclose_maps_all_none() {
        let mut chart_data = valid_chart_data();
        if let Some(obj) = chart_data
            .get_mut("indicators")
            .and_then(|v| v.as_object_mut())
        {
            obj.remove("adjclose");
        }
        let result = chart_data_to_klines(&chart_data);
        assert!(result.is_ok());
        let klines = match result {
            Ok(klines) => klines,
            Err(_) => Vec::new(),
        };
        assert_eq!(klines.len(), 3);
        for kline in &klines {
            assert_eq!(kline.adjclose, None);
        }
    }

    #[test]
    fn missing_arrays_error() {
        let mut no_timestamp = valid_chart_data();
        if let Some(obj) = no_timestamp.as_object_mut() {
            obj.remove("timestamp");
        }
        assert!(chart_data_to_klines(&no_timestamp).is_err());

        let mut no_quote = valid_chart_data();
        if let Some(obj) = no_quote
            .get_mut("indicators")
            .and_then(|v| v.as_object_mut())
        {
            obj.remove("quote");
        }
        assert!(chart_data_to_klines(&no_quote).is_err());

        let mut no_open = valid_chart_data();
        if let Some(obj) = no_open["indicators"]["quote"][0].as_object_mut() {
            obj.remove("open");
        }
        assert!(chart_data_to_klines(&no_open).is_err());

        let mut non_array_close = valid_chart_data();
        non_array_close["indicators"]["quote"][0]["close"] = json!("bad");
        assert!(chart_data_to_klines(&non_array_close).is_err());

        let missing_result = json!({"chart": {"result": null, "error": null}});
        assert!(parse_chart_response(&missing_result).is_err());

        let empty_result = json!({"chart": {"result": [], "error": null}});
        assert!(parse_chart_response(&empty_result).is_err());

        let chart_error = json!({"chart": {"result": null, "error": {"code": "Not Found"}}});
        assert!(parse_chart_response(&chart_error).is_err());
    }

    #[test]
    fn length_mismatch_errors() {
        let mut mismatch = valid_chart_data();
        mismatch["indicators"]["quote"][0]["open"] = json!([10.0, 11.0]);
        assert!(chart_data_to_klines(&mismatch).is_err());

        let mut adj_mismatch = valid_chart_data();
        adj_mismatch["indicators"]["adjclose"][0]["adjclose"] = json!([10.1, 11.1]);
        assert!(chart_data_to_klines(&adj_mismatch).is_err());
    }

    #[test]
    fn all_invalid_required_returns_ok_empty() {
        let mut chart_data = valid_chart_data();
        chart_data["indicators"]["quote"][0]["open"] = json!([null, null, null]);
        let result = chart_data_to_klines(&chart_data);
        assert!(result.is_ok());
        match result {
            Ok(klines) => assert!(klines.is_empty()),
            Err(_) => assert!(false, "expected Ok empty"),
        }
    }
}

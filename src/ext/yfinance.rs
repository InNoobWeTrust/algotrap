use crate::model::Kline;
use core::error::Error;
use core::fmt::Display;
use reqwest::Url;
use serde_json::json;

const YFINANCE_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const YFINANCE_COOKIE_URL: &str = "https://fc.yahoo.com";
const YFINANCE_CRUMB_URL: &str = "https://query1.finance.yahoo.com/v1/test/getcrumb";
pub const YFINANCE_API_HISTORY: &str = "https://query2.finance.yahoo.com/v8/finance/chart/";

#[derive(Debug, Clone, Copy)]
pub enum YfinanceInterval {
    //"1d","5d","1mo","3mo","6mo","1y","2y","5y","10y","ytd","max"
    D1,
    D5,
    Mo1,
    Mo3,
    Mo6,
    Y1,
    Y2,
    Y5,
    Y10,
    Ytd,
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
        Self {
            client: reqwest::ClientBuilder::new()
                .cookie_store(true)
                .user_agent(YFINANCE_USER_AGENT)
                .build()
                .unwrap(),
        }
    }
}

impl YfinanceClient {
    pub fn new() -> Self {
        Self::default()
    }

    // Fetch history
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

        if json_resp["chart"]["error"] != json!(null) {
            return Err(format!("{json_resp:#?}").into());
        }

        let chart_data = &json_resp["chart"]["result"][0];
        //dbg!(&chart_data);
        let timestamps: Vec<_> = chart_data["timestamp"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        let quotes = &chart_data["indicators"]["quote"][0];
        let open: Vec<_> = quotes["open"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let high: Vec<_> = quotes["high"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let low: Vec<_> = quotes["low"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let close: Vec<_> = quotes["close"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let adjclose: Vec<_> = chart_data["indicators"]["adjclose"][0]["adjclose"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let volume: Vec<_> = quotes["volume"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();

        let klines = (0..timestamps.len())
            .map(|i| Kline {
                open: open[i],
                high: high[i],
                low: low[i],
                close: close[i],
                volume: volume[i],
                time: timestamps[i],
                adjclose: Some(adjclose[i]),
            })
            .collect();

        Ok(klines)
    }
}

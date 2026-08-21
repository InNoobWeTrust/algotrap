use crate::model::Kline;
use core::error::Error;
use core::fmt::Display;
use core::time::Duration;
use hex;
use hmac::{Hmac, Mac};
use reqwest::Url;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use tap::Pipe;

type HmacSha256 = Hmac<Sha256>;
pub const MAX_LIMIT: u32 = 1440;
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
pub const BINGX_API_KLINES: &str = "https://open-api.bingx.com/openApi/swap/v3/quote/klines";

#[derive(Clone)]
pub struct BingXClient {
    api_key: String,
    secret_key: String,
    anonymous: bool,
    client: reqwest::Client,
}

impl Default for BingXClient {
    fn default() -> Self {
        Self::with_timeout(DEFAULT_TIMEOUT_SECS)
    }
}

impl BingXClient {
    pub fn new(api_key: &str, secret_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            secret_key: secret_key.to_string(),
            anonymous: false,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .build()
                .expect("Failed to build reqwest client with timeout"),
        }
    }

    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self {
            api_key: "".to_string(),
            secret_key: "".to_string(),
            anonymous: true,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout_secs))
                .build()
                .expect("Failed to build reqwest client with timeout"),
        }
    }

    // Generate BingX-compliant signature
    fn generate_signature(&self, params: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
            .expect("HMAC initialization failed");
        mac.update(params.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    // Fetch perpetual futures candles
    pub async fn get_futures_klines(
        &self,
        symbol: &str,
        interval: &impl Display,
        limit: u32,
    ) -> Result<Vec<Kline>, Box<dyn Error + Send + Sync>> {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();
        let limit_str = limit.to_string();

        let mut params_vec = vec![
            ("symbol", symbol.to_string()),
            ("interval", interval.to_string()),
            ("limit", limit_str),
            ("time", time),
        ];
        params_vec.sort_by_key(|k| k.0); // BingX requires sorted params for signing
        let query_string = params_vec
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");

        if self.anonymous {
            let signature = self.generate_signature(&query_string);
            params_vec.push(("signature", signature));
        }

        let url = Url::parse_with_params(BINGX_API_KLINES, params_vec)?;

        let response = self
            .client
            .get(url)
            .pipe(|req| {
                if self.anonymous {
                    req
                } else {
                    req.header("X-BX-APIKEY", &self.api_key)
                }
            })
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        if response["code"] != 0 {
            let msg = response["msg"].as_str().unwrap_or("unknown BingX error");
            let code = &response["code"];
            return Err(format!("BingX API error code {code}: {msg}").into());
        }

        deserialize_futures_klines(response["data"].clone()).map_err(Into::into)
    }
}

/// Deserializes futures klines in the chronological order required by TA.
pub(crate) fn deserialize_futures_klines(
    payload: serde_json::Value,
) -> Result<Vec<Kline>, serde_json::Error> {
    let mut klines = serde_json::from_value::<Vec<Kline>>(payload)?;
    // WHY: BingX futures-klines payloads are documented newest-first; TA requires oldest-first.
    klines.reverse();
    Ok(klines)
}

#[cfg(test)]
mod tests {
    use super::deserialize_futures_klines;
    use serde_json::json;

    #[test]
    fn futures_klines_normalize_bingx_newest_first_payload_without_changing_candles() {
        let payload = json!([
            {
                "open": "102.0",
                "high": "103.0",
                "low": "101.0",
                "close": "102.5",
                "volume": "12.0",
                "time": 1_700_000_120_000_i64,
                "adjclose": "102.25"
            },
            {
                "open": "101.0",
                "high": "102.0",
                "low": "100.0",
                "close": "101.5",
                "volume": "11.0",
                "time": 1_700_000_060_000_i64,
                "adjclose": null
            },
            {
                "open": "100.0",
                "high": "101.0",
                "low": "99.0",
                "close": "100.5",
                "volume": "10.0",
                "time": 1_700_000_000_000_i64,
                "adjclose": "100.25"
            }
        ]);

        let klines = deserialize_futures_klines(payload).unwrap();

        assert_eq!(klines.len(), 3);
        assert!(klines.windows(2).all(|pair| pair[0].time < pair[1].time));
        assert_eq!(klines[0].time, 1_700_000_000_000);
        assert_eq!(klines[0].open, 100.0);
        assert_eq!(klines[0].adjclose, Some(100.25));
        assert_eq!(klines[2].time, 1_700_000_120_000);
        assert_eq!(klines[2].close, 102.5);
        assert_eq!(klines[2].adjclose, Some(102.25));
    }
}

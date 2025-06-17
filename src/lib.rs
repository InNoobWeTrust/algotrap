use core::error::Error;
use hex;
use hmac::{Hmac, Mac};
use reqwest::Url;
use serde::Deserialize;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use tap::Pipe;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Deserialize)]
pub struct Kline {
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub time: i64,
}

#[derive(Clone)]
pub struct BingXClient {
    api_key: String,
    secret_key: String,
    anonymous: bool,
    client: reqwest::Client,
}

impl Default for BingXClient {
    fn default() -> Self {
        Self {
            api_key: "".to_string(),
            secret_key: "".to_string(),
            anonymous: true,
            client: reqwest::Client::new(),
        }
    }
}

impl BingXClient {
    pub fn new(api_key: &str, secret_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            secret_key: secret_key.to_string(),
            anonymous: false,
            client: reqwest::Client::new(),
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
        interval: &str,
        limit: u32,
    ) -> Result<Vec<Kline>, Box<dyn Error>> {
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
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        if self.anonymous {
            let signature = self.generate_signature(&query_string);
            params_vec.push(("signature", signature));
        }

        let url = Url::parse_with_params(
            "https://open-api.bingx.com/openApi/swap/v3/quote/klines",
            params_vec,
        )?;

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

        Ok(serde_json::from_value(response["data"].clone()).unwrap())
    }
}

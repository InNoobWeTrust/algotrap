use serde::Deserialize;

use algotrap::prelude::*;

// ─── Per-Ticker Config ───────────────────────────────────────────────────────

/// Trading parameters specific to a single ticker.
#[derive(Debug, Clone, Deserialize)]
pub struct TickerConf {
    pub symbol: String,
    pub sl_percent: f64,
    pub tol_percent: f64,
    #[serde(deserialize_with = "deserialize_tfs")]
    pub tfs: Vec<Timeframe>,
    pub default_tf: Timeframe,
}

/// Deserialize comma-separated timeframes from a JSON string field.
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

// ─── Global Config ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct EnvConf {
    // Multi-ticker (JSON array)
    #[serde(deserialize_with = "deserialize_tickers")]
    pub tickers: Vec<TickerConf>,

    // Telegram
    pub telegram_bot_token: String,
    pub telegram_chat_id: i64,

    // LLM
    pub llm_api_base: String,
    pub llm_api_key: String,
    pub llm_model: String,
    #[serde(default)]
    pub llm_debug: bool,

    // Browserless
    pub browserless_url: String,

    // Prompt config directory (system.txt, user.txt, tools.json)
    #[serde(default = "default_prompts_dir")]
    pub prompts_dir: String,

    // Alert scanning
    #[serde(default = "default_scan_interval")]
    pub scan_interval_secs: u64,

    // Adaptive scoring — weight guardrails
    #[serde(default = "default_weight_rate_limit")]
    pub weight_rate_limit: f64,
    #[serde(default = "default_weight_min")]
    pub weight_min: f64,
    #[serde(default = "default_weight_max")]
    pub weight_max: f64,

    // Memory
    #[serde(default = "default_memory_dir")]
    pub memory_dir: String,
    #[serde(default = "default_max_predictions")]
    pub max_predictions: usize,
    #[serde(default = "default_keep_recent_messages")]
    pub keep_recent_messages: usize,

    // Tier boundaries
    #[serde(default = "default_tier_alert_threshold")]
    pub tier_alert_threshold: f64,
    #[serde(default = "default_tier_watch_threshold")]
    pub tier_watch_threshold: f64,

    // Change detection
    #[serde(default = "default_change_detection_indicators")]
    pub change_detection_indicators: String,

    // Notification cooldown — minimum seconds between notifications per ticker
    #[serde(default = "default_notification_cooldown_secs")]
    pub notification_cooldown_secs: u64,

    // HTTP request timeout
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    /// Whether the LLM model supports native reasoning (e.g., reasoning_effort param).
    /// When true, CoT prompting is suppressed (prevents double-reasoning waste).
    /// When false (default), a chain-of-thought trigger is appended to the system prompt.
    #[serde(default)]
    pub supports_reasoning: bool,
}

/// Deserialize `TICKERS` env var: a JSON array of TickerConf objects.
fn deserialize_tickers<'de, D>(deserializer: D) -> Result<Vec<TickerConf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    serde_json::from_str(&s).map_err(serde::de::Error::custom)
}

fn default_scan_interval() -> u64 {
    900 // 15 minutes
}

fn default_weight_rate_limit() -> f64 {
    0.05
}

fn default_weight_min() -> f64 {
    0.01
}

fn default_weight_max() -> f64 {
    0.50
}

fn default_memory_dir() -> String {
    "/data/memory".to_string()
}

fn default_max_predictions() -> usize {
    8
}

fn default_keep_recent_messages() -> usize {
    10
}

fn default_tier_alert_threshold() -> f64 {
    70.0
}

fn default_tier_watch_threshold() -> f64 {
    55.0
}

fn default_change_detection_indicators() -> String {
    "rssi,structure_power".to_string()
}

fn default_timeout_secs() -> u64 {
    30
}

fn default_notification_cooldown_secs() -> u64 {
    3600 // 1 hour
}

fn default_prompts_dir() -> String {
    "config/prompts".to_string()
}

impl EnvConf {
    /// Find a ticker config by symbol (case-insensitive).
    pub fn find_ticker(&self, symbol: &str) -> Option<&TickerConf> {
        self.tickers
            .iter()
            .find(|tc| tc.symbol.eq_ignore_ascii_case(symbol))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Helper: build a minimal env HashMap for EnvConf deserialization.
    fn base_env() -> HashMap<String, String> {
        let tickers_json = r#"[{"symbol":"BTC-USDT","sl_percent":0.1,"tol_percent":0.618,"tfs":"1m,5m,15m,1h,4h,1d,1w,1M","default_tf":"15m"},{"symbol":"ETH-USDT","sl_percent":0.08,"tol_percent":0.5,"tfs":"15m,1h,4h","default_tf":"4h"}]"#;
        let mut env = HashMap::new();
        env.insert("TICKERS".into(), tickers_json.into());
        env.insert("TELEGRAM_BOT_TOKEN".into(), "test-token".into());
        env.insert("TELEGRAM_CHAT_ID".into(), "-100123".into());
        env.insert("LLM_API_BASE".into(), "http://localhost:4000/v1".into());
        env.insert("LLM_API_KEY".into(), "sk-test".into());
        env.insert("LLM_MODEL".into(), "test-model".into());
        env.insert("BROWSERLESS_URL".into(), "http://localhost:3000".into());
        env
    }

    #[test]
    fn test_tickers_json_deserialization() {
        let env = base_env();
        let conf: EnvConf = envy::from_iter(env.into_iter()).unwrap();

        assert_eq!(conf.tickers.len(), 2);
        assert_eq!(conf.tickers[0].symbol, "BTC-USDT");
        assert_eq!(conf.tickers[0].tfs.len(), 8);
        assert_eq!(conf.tickers[1].symbol, "ETH-USDT");
        assert_eq!(conf.tickers[1].tfs.len(), 3);
    }

    #[test]
    fn test_ticker_conf_fields() {
        let env = base_env();
        let conf: EnvConf = envy::from_iter(env.into_iter()).unwrap();

        let btc = &conf.tickers[0];
        assert!((btc.sl_percent - 0.1).abs() < f64::EPSILON);
        assert!((btc.tol_percent - 0.618).abs() < f64::EPSILON);
        assert_eq!(btc.default_tf, Timeframe::M15);

        let eth = &conf.tickers[1];
        assert!((eth.sl_percent - 0.08).abs() < f64::EPSILON);
        assert_eq!(eth.default_tf, Timeframe::H4);
    }

    #[test]
    fn test_default_values() {
        let env = base_env();
        let conf: EnvConf = envy::from_iter(env.into_iter()).unwrap();

        assert_eq!(conf.scan_interval_secs, 900);
        assert!((conf.tier_alert_threshold - 70.0).abs() < f64::EPSILON);
        assert!((conf.tier_watch_threshold - 55.0).abs() < f64::EPSILON);
        assert!((conf.weight_rate_limit - 0.05).abs() < f64::EPSILON);
        assert!((conf.weight_min - 0.01).abs() < f64::EPSILON);
        assert!((conf.weight_max - 0.50).abs() < f64::EPSILON);
        assert_eq!(conf.max_predictions, 8);
        assert_eq!(conf.keep_recent_messages, 10);
        assert_eq!(conf.memory_dir, "/data/memory");
        assert_eq!(conf.timeout_secs, 30);
        assert_eq!(conf.prompts_dir, "config/prompts");
        assert_eq!(conf.change_detection_indicators, "rssi,structure_power");
    }

    #[test]
    fn test_custom_scan_interval() {
        let mut env = base_env();
        env.insert("SCAN_INTERVAL_SECS".into(), "300".into());
        env.insert("TIER_ALERT_THRESHOLD".into(), "85".into());
        let conf: EnvConf = envy::from_iter(env.into_iter()).unwrap();

        assert_eq!(conf.scan_interval_secs, 300);
        assert!((conf.tier_alert_threshold - 85.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_find_ticker_case_insensitive() {
        let env = base_env();
        let conf: EnvConf = envy::from_iter(env.into_iter()).unwrap();

        assert!(conf.find_ticker("BTC-USDT").is_some());
        assert!(conf.find_ticker("btc-usdt").is_some());
        assert!(conf.find_ticker("Btc-Usdt").is_some());
        assert!(conf.find_ticker("ETH-USDT").is_some());
        assert!(conf.find_ticker("XRP-USDT").is_none());
    }

    #[test]
    fn test_invalid_tickers_json() {
        let mut env = base_env();
        env.insert("TICKERS".into(), "not-valid-json".into());
        let result: Result<EnvConf, _> = envy::from_iter(env.into_iter());
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_tickers_array() {
        let mut env = base_env();
        env.insert("TICKERS".into(), "[]".into());
        let conf: EnvConf = envy::from_iter(env.into_iter()).unwrap();
        assert!(conf.tickers.is_empty());
    }

    #[test]
    fn test_supports_reasoning_default() {
        let env = base_env();
        let conf: EnvConf = envy::from_iter(env.into_iter()).unwrap();
        assert!(!conf.supports_reasoning); // defaults to false
    }

    #[test]
    fn test_supports_reasoning_enabled() {
        let mut env = base_env();
        env.insert("SUPPORTS_REASONING".into(), "true".into());
        let conf: EnvConf = envy::from_iter(env.into_iter()).unwrap();
        assert!(conf.supports_reasoning);
    }
}

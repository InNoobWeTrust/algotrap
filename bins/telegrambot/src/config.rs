use serde::Deserialize;

use algotrap::prelude::*;

#[derive(Debug, Clone, Deserialize)]
pub struct EnvConf {
    // Trading
    pub symbol: String,
    pub sl_percent: f64,
    pub tol_percent: f64,
    pub tfs: Vec<Timeframe>,
    pub default_tf: Timeframe,

    // Telegram
    pub telegram_bot_token: String,
    pub telegram_chat_id: i64,

    // LLM
    pub llm_api_base: String,
    pub llm_api_key: String,
    pub llm_model: String,

    // Browserless
    pub browserless_url: String,

    // Scheduling
    #[serde(default = "default_analysis_interval")]
    pub analysis_interval_secs: u64,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_analysis_interval() -> u64 {
    3600
}

fn default_timeout_secs() -> u64 {
    30
}

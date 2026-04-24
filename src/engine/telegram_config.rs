//! Telegram bot indicator configuration for the engine boundary.
//!
//! This is a simplified, engine-neutral version of the telegrambot's IndicatorConfig.
//! It provides the period() and smooth() methods needed by the indicator pipeline
//! without depending on the telegrambot's memory module.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Per-indicator parameter specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorParamSpec {
    pub period: Option<usize>,
    pub smooth: Option<usize>,
}

/// Telegram bot indicator configuration.
///
/// Keys are indicator group names (e.g., "rssi", "atr", "structure_power").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramIndicatorConfig {
    pub indicators: HashMap<String, IndicatorParamSpec>,
}

impl Default for TelegramIndicatorConfig {
    fn default() -> Self {
        let mut indicators = HashMap::new();

        indicators.insert(
            "rssi".into(),
            IndicatorParamSpec {
                period: Some(14),
                smooth: Some(9),
            },
        );
        indicators.insert(
            "structure_power".into(),
            IndicatorParamSpec {
                period: None,
                smooth: Some(9),
            },
        );
        indicators.insert(
            "atr".into(),
            IndicatorParamSpec {
                period: Some(42),
                smooth: None,
            },
        );
        indicators.insert(
            "ema200".into(),
            IndicatorParamSpec {
                period: Some(200),
                smooth: None,
            },
        );
        indicators.insert(
            "sharpe".into(),
            IndicatorParamSpec {
                period: Some(200),
                smooth: None,
            },
        );
        indicators.insert(
            "bias_reversion".into(),
            IndicatorParamSpec {
                period: None,
                smooth: Some(9),
            },
        );
        indicators.insert(
            "revrsi".into(),
            IndicatorParamSpec {
                period: Some(14),
                smooth: None,
            },
        );
        indicators.insert(
            "gap_zones".into(),
            IndicatorParamSpec {
                period: Some(42),
                smooth: None,
            },
        );

        Self { indicators }
    }
}

impl TelegramIndicatorConfig {
    /// Get a tunable period value for an indicator, falling back to default.
    pub fn period(&self, name: &str, default: usize) -> usize {
        self.indicators
            .get(name)
            .and_then(|p| p.period)
            .unwrap_or(default)
    }

    /// Get a tunable smooth value for an indicator, falling back to default.
    pub fn smooth(&self, name: &str, default: usize) -> usize {
        self.indicators
            .get(name)
            .and_then(|p| p.smooth)
            .unwrap_or(default)
    }

    /// Returns the keys that are expected but missing from the config.
    /// This helps detect when deserialization silently falls back to defaults.
    pub fn missing_keys(&self, required: &[&str]) -> Vec<String> {
        required
            .iter()
            .filter(|&&key| !self.indicators.contains_key(key))
            .map(|&key| key.to_string())
            .collect()
    }

    /// Returns warnings for missing or invalid indicator configurations.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        let required_keys = [
            "rssi",
            "structure_power",
            "atr",
            "ema200",
            "sharpe",
            "bias_reversion",
            "revrsi",
            "gap_zones",
        ];

        let missing = self.missing_keys(&required_keys);
        for key in &missing {
            warnings.push(format!("Missing indicator config: {}", key));
        }

        // This engine-neutral config does not track per-indicator active flags,
        // so every present config entry is treated as active.
        let active_count = self.indicators.len();
        if active_count < 2 {
            warnings.push(format!(
                "Only {} active indicators - minimum 2 required",
                active_count
            ));
        }

        warnings
    }
}

#[cfg(test)]
mod tests {
    use super::{IndicatorParamSpec, TelegramIndicatorConfig};
    use std::collections::HashMap;

    #[test]
    fn missing_keys_reports_absent_required_entries() {
        let mut indicators = HashMap::new();
        indicators.insert(
            "rssi".to_string(),
            IndicatorParamSpec {
                period: Some(14),
                smooth: Some(9),
            },
        );

        let config = TelegramIndicatorConfig { indicators };

        assert_eq!(
            config.missing_keys(&["rssi", "atr", "ema200"]),
            vec!["atr".to_string(), "ema200".to_string()]
        );
    }

    #[test]
    fn validate_warns_for_missing_keys_and_too_few_entries() {
        let mut indicators = HashMap::new();
        indicators.insert(
            "rssi".to_string(),
            IndicatorParamSpec {
                period: Some(14),
                smooth: Some(9),
            },
        );

        let config = TelegramIndicatorConfig { indicators };
        let warnings = config.validate();

        assert!(
            warnings.contains(&"Missing indicator config: atr".to_string()),
            "expected missing atr warning, got {warnings:?}"
        );
        assert!(
            warnings.contains(&"Only 1 active indicators - minimum 2 required".to_string()),
            "expected minimum active warning, got {warnings:?}"
        );
    }
}

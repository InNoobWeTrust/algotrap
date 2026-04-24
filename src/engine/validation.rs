//! Input validation types for the engine boundary.

use crate::engine::error::MarketError;

/// Canonical ticker symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticker(pub String);

impl Ticker {
    /// Returns the ticker symbol as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Ticker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Validated ticker that has been canonicalized.
///
/// Created from raw user input via [`parse_validated_ticker`].
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedTicker {
    value: String,
    sl_percent: f64,
    tol_percent: f64,
}

impl ValidatedTicker {
    /// Creates a new validated ticker.
    pub fn new(raw: &str, sl_percent: f64, tol_percent: f64) -> Result<Self, MarketError> {
        let validated = parse_validated_ticker(raw)?;

        if !(0.0..=1.0).contains(&sl_percent) {
            return Err(MarketError::validation(
                "sl_percent must be between 0.0 and 1.0",
            ));
        }
        if !(0.0..=1.0).contains(&tol_percent) {
            return Err(MarketError::validation(
                "tol_percent must be between 0.0 and 1.0",
            ));
        }

        Ok(Self {
            value: validated.into_inner(),
            sl_percent,
            tol_percent,
        })
    }

    /// Returns the validated ticker as a string slice.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    fn into_inner(self) -> String {
        self.value
    }
}

impl std::fmt::Display for ValidatedTicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

/// Indicator specification for the engine.
///
/// These are the allowed indicators that can be computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatedIndicator {
    SMA,
    EMA,
    RSI,
    RevRsi,
    ATR,
    ATRRevPercent,
    BandReversion,
    BiasReversion,
    Sharpe,
    StructurePower,
    IsAtrGap,
    BodyRatio,
    BiasedCandle,
    Leverage,
    Date,
}

impl ValidatedIndicator {
    /// Returns the output column names for this indicator.
    pub fn output_columns(&self) -> Vec<String> {
        match self {
            ValidatedIndicator::SMA => vec!["volume_sma".to_string()],
            ValidatedIndicator::EMA => vec!["ema200".to_string()],
            ValidatedIndicator::RSI => vec!["rssi".to_string(), "rssi_ma".to_string()],
            ValidatedIndicator::RevRsi => vec![
                "neutral_revrsi".to_string(),
                "bullish_revrsi".to_string(),
                "bearish_revrsi".to_string(),
            ],
            ValidatedIndicator::ATR => vec![
                "atr_upperband".to_string(),
                "atr_lowerband".to_string(),
                "atr_percent".to_string(),
            ],
            ValidatedIndicator::ATRRevPercent => vec!["atr_reversion_percent".to_string()],
            ValidatedIndicator::BandReversion => vec!["band_reversion".to_string()],
            ValidatedIndicator::BiasReversion => vec!["bias_reversion".to_string()],
            ValidatedIndicator::Sharpe => vec!["sharpe".to_string()],
            ValidatedIndicator::StructurePower => vec![
                "structure_power".to_string(),
                "structure_power_sma".to_string(),
            ],
            ValidatedIndicator::IsAtrGap => vec!["is_atr_gap".to_string()],
            ValidatedIndicator::BodyRatio => vec!["body_ratio".to_string()],
            ValidatedIndicator::BiasedCandle => vec!["biased_candle".to_string()],
            ValidatedIndicator::Leverage => vec!["leverage".to_string()],
            ValidatedIndicator::Date => vec!["Date".to_string()],
        }
    }
}

/// Parses and validates a ticker symbol.
///
/// # Rules
/// - Must be ASCII-only
/// - Alphanumeric, dots, underscores, hyphens allowed
/// - Converted to uppercase
/// - Whitespace trimmed
/// - Maximum 32 characters
///
/// # Errors
/// Returns [`MarketError::validation`] if the ticker is invalid.
pub fn parse_validated_ticker(raw: &str) -> Result<ValidatedTicker, MarketError> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Err(MarketError::validation("Ticker cannot be empty"));
    }

    if trimmed.len() > 32 {
        return Err(MarketError::validation(
            "Ticker exceeds maximum length of 32 characters",
        ));
    }

    if trimmed.contains("..") {
        return Err(MarketError::validation(
            "Ticker contains invalid path traversal pattern",
        ));
    }

    // Check for ASCII-only and allowed characters
    for (i, c) in trimmed.chars().enumerate() {
        if !c.is_ascii() {
            return Err(MarketError::validation(format!(
                "Ticker contains non-ASCII character at position {}",
                i
            )));
        }
        if !c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != '-' {
            return Err(MarketError::validation(format!(
                "Ticker contains invalid character '{}' at position {}",
                c, i
            )));
        }
    }

    Ok(ValidatedTicker {
        value: trimmed.to_ascii_uppercase(),
        sl_percent: 0.0,
        tol_percent: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_tickers() {
        assert!(parse_validated_ticker("BTCUSDT").is_ok());
        assert!(parse_validated_ticker("btcusdt").is_ok()); // converted to uppercase
        assert!(parse_validated_ticker("BTC-USDT").is_ok());
        assert!(parse_validated_ticker("BTC_USDT").is_ok());
        assert!(parse_validated_ticker("BTC.USDT").is_ok());
    }

    #[test]
    fn test_invalid_tickers() {
        assert!(parse_validated_ticker("").is_err());
        assert!(parse_validated_ticker("BTC/USDT").is_err()); // slash not allowed
        assert!(parse_validated_ticker("BTC USDT").is_err()); // space not allowed
        assert!(parse_validated_ticker("../BTCUSDT").is_err());
    }

    #[test]
    fn test_validated_ticker_new_validates_percentages() {
        assert!(ValidatedTicker::new("BTCUSDT", 0.02, 0.01).is_ok());
        assert!(ValidatedTicker::new("BTCUSDT", -0.01, 0.01).is_err());
        assert!(ValidatedTicker::new("BTCUSDT", 0.02, 1.01).is_err());
    }

    #[test]
    fn test_output_columns_match_engine_aliases() {
        assert_eq!(
            ValidatedIndicator::EMA.output_columns(),
            vec!["ema200".to_string()]
        );
        assert_eq!(
            ValidatedIndicator::RSI.output_columns(),
            vec!["rssi".to_string(), "rssi_ma".to_string()]
        );
    }
}

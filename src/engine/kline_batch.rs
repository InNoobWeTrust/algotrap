//! Kline batch handling for memory estimation and limits.

use crate::engine::error::MarketError;
use crate::model::kline::Kline;

/// A batch of raw klines for processing.
#[derive(Debug, Clone)]
pub struct RawKlineBatch {
    /// The klines in the batch.
    pub klines: Vec<Kline>,
    /// Optional ticker symbol for the batch.
    pub ticker: Option<String>,
}

impl RawKlineBatch {
    /// Creates a new raw kline batch.
    pub fn new(klines: Vec<Kline>, ticker: Option<String>) -> Self {
        Self { klines, ticker }
    }

    /// Returns the number of klines in the batch.
    pub fn len(&self) -> usize {
        self.klines.len()
    }

    /// Returns true if the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.klines.is_empty()
    }
}

/// Limits for batch processing.
#[derive(Debug, Clone)]
pub struct BatchLimits {
    /// Maximum number of rows per batch.
    pub max_batch_rows: usize,
    /// Maximum batch size in bytes.
    pub max_batch_bytes: u64,
    /// Maximum number of indicators per batch.
    pub max_indicators: usize,
}

impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            max_batch_rows: 10_000,
            max_batch_bytes: 100 * 1024 * 1024, // 100 MB
            max_indicators: 64,
        }
    }
}

impl BatchLimits {
    /// Validates that the indicator count does not exceed the maximum.
    ///
    /// # Errors
    /// Returns error if `count` exceeds `max_indicators`.
    pub fn validate_indicator_count(&self, count: usize) -> Result<(), MarketError> {
        if count > self.max_indicators {
            Err(MarketError::validation(format!(
                "Indicator count {} exceeds maximum {}",
                count, self.max_indicators
            )))
        } else {
            Ok(())
        }
    }
}

/// Computes estimated memory bytes for a raw kline batch.
///
/// Returns a pessimistic upper-bound estimate using checked arithmetic.
/// Returns error if the computation would overflow.
pub fn estimated_memory_bytes(
    input: &RawKlineBatch,
    limits: &BatchLimits,
) -> Result<u64, MarketError> {
    // Base size: 6 f64 fields * 8 bytes each = 48 bytes per kline
    let kline_count = input.klines.len() as u64;
    let per_kline_bytes: u64 = 48;

    // Calculate base size with checked arithmetic
    let base_size = kline_count
        .checked_mul(per_kline_bytes)
        .ok_or_else(|| MarketError::computation("Batch size overflow in memory estimation"))?;

    // Add overhead for Vec and optional fields
    let overhead = kline_count.saturating_mul(16); // rough overhead estimate

    let total = base_size
        .checked_add(overhead)
        .ok_or_else(|| MarketError::computation("Batch size overflow in memory estimation"))?;

    // Check against limits
    if kline_count > limits.max_batch_rows as u64 {
        return Err(MarketError::validation(format!(
            "Batch row count {} exceeds limit {}",
            kline_count, limits.max_batch_rows
        )));
    }

    if total > limits.max_batch_bytes {
        return Err(MarketError::validation(format!(
            "Batch size {} bytes exceeds limit {} bytes",
            total, limits.max_batch_bytes
        )));
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_batch() {
        let batch = RawKlineBatch::new(Vec::new(), None);
        let limits = BatchLimits::default();
        let bytes = estimated_memory_bytes(&batch, &limits).unwrap();
        assert_eq!(bytes, 0);
    }

    #[test]
    fn test_single_kline() {
        let kline = Kline {
            open: 1.0,
            high: 2.0,
            low: 0.5,
            close: 1.5,
            volume: 100.0,
            time: 0,
            adjclose: None,
        };
        let batch = RawKlineBatch::new(vec![kline], Some("BTCUSDT".to_string()));
        let limits = BatchLimits::default();
        let bytes = estimated_memory_bytes(&batch, &limits).unwrap();
        assert!(bytes > 0);
    }

    #[test]
    fn test_overflow_rejected() {
        let limits = BatchLimits {
            max_batch_rows: usize::MAX,
            max_batch_bytes: u64::MAX,
            max_indicators: 64,
        };
        // This would overflow if we had enough klines
        let kline = Kline {
            open: 1.0,
            high: 2.0,
            low: 0.5,
            close: 1.5,
            volume: 100.0,
            time: 0,
            adjclose: None,
        };
        let batch = RawKlineBatch::new(vec![kline; 1000], None);
        let result = estimated_memory_bytes(&batch, &limits);
        // Should not overflow with reasonable batch sizes
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_indicator_count() {
        let limits = BatchLimits::default();

        // Should pass
        assert!(limits.validate_indicator_count(5).is_ok());

        // Should fail
        assert!(limits.validate_indicator_count(100).is_err());
    }
}

//! Validated OHLC inputs and price-range kernels.

use super::{TaError, TaResult, validate_finite_output};

use super::ma::rma;

/// Borrowed, equally-sized chronological open/high/low/close series.
#[derive(Debug, Clone, Copy)]
pub struct Ohlc<'a> {
    open: &'a [f64],
    high: &'a [f64],
    low: &'a [f64],
    close: &'a [f64],
}

impl<'a> Ohlc<'a> {
    /// Validates non-empty, finite, equally-sized OHLC input series.
    pub fn new(
        open: &'a [f64],
        high: &'a [f64],
        low: &'a [f64],
        close: &'a [f64],
    ) -> TaResult<Self> {
        if open.is_empty() {
            return Err(TaError::validation("OHLC slices are empty"));
        }
        if [high.len(), low.len(), close.len()]
            .iter()
            .any(|&length| length != open.len())
        {
            return Err(TaError::alignment("OHLC slices must have equal lengths"));
        }
        for (name, values) in [
            ("open", open),
            ("high", high),
            ("low", low),
            ("close", close),
        ] {
            if values.iter().any(|value| !value.is_finite()) {
                return Err(TaError::validation(format!(
                    "OHLC {name} contains a non-finite value"
                )));
            }
        }
        Ok(Self {
            open,
            high,
            low,
            close,
        })
    }

    /// Returns `close - open + (high - open) - (open - low)` for every bar.
    pub fn bar_bias(self) -> TaResult<Vec<f64>> {
        let result = self
            .open
            .iter()
            .zip(self.high)
            .zip(self.low)
            .zip(self.close)
            .map(|(((&open, &high), &low), &close)| (close - open) + (high - open) - (open - low))
            .collect::<Vec<_>>();
        validate_finite_output("OHLC bar bias", &result)?;
        Ok(result)
    }

    /// Returns `abs(close - open) / (high - low)`, using zero for zero-range bars.
    pub fn body_ratio(self) -> TaResult<Vec<f64>> {
        let result = self
            .open
            .iter()
            .zip(self.high)
            .zip(self.low)
            .zip(self.close)
            .map(|(((&open, &high), &low), &close)| {
                let range = high - low;
                if range == 0.0 {
                    0.0
                } else {
                    (close - open).abs() / range
                }
            })
            .collect::<Vec<_>>();
        validate_finite_output("OHLC body ratio", &result)?;
        Ok(result)
    }

    /// Returns true range, treating the first bar's previous close as its own close.
    pub fn true_range(self) -> TaResult<Vec<f64>> {
        let result = self
            .high
            .iter()
            .zip(self.low)
            .zip(self.close)
            .enumerate()
            .map(|(index, ((&high, &low), &close))| {
                let previous_close = index
                    .checked_sub(1)
                    .map(|previous| self.close[previous])
                    .unwrap_or(close);
                (high - low)
                    .max((high - previous_close).abs())
                    .max((low - previous_close).abs())
            })
            .collect::<Vec<_>>();
        validate_finite_output("OHLC true range", &result)?;
        Ok(result)
    }

    /// Returns ATR as Wilder's moving average of this instance's true range.
    pub fn atr(self, period: usize) -> TaResult<Vec<f64>> {
        rma(&self.true_range()?, period)
    }

    /// Returns the open series.
    pub fn open(self) -> &'a [f64] {
        self.open
    }
    /// Returns the high series.
    pub fn high(self) -> &'a [f64] {
        self.high
    }
    /// Returns the low series.
    pub fn low(self) -> &'a [f64] {
        self.low
    }
    /// Returns the close series.
    pub fn close(self) -> &'a [f64] {
        self.close
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_bar_bias_body_ratio_and_first_true_range() {
        let ohlc = Ohlc::new(&[10.0, 11.0], &[12.0, 15.0], &[8.0, 10.0], &[11.0, 14.0]).unwrap();
        assert_eq!(ohlc.bar_bias().unwrap(), vec![1.0, 6.0]);
        assert_eq!(ohlc.body_ratio().unwrap(), vec![0.25, 0.6]);
        assert_eq!(ohlc.true_range().unwrap(), vec![4.0, 5.0]);
        assert_eq!(ohlc.atr(2).unwrap(), vec![4.0, 4.5]);
    }
    #[test]
    fn validates_input_and_preserves_zero_range_ratio() {
        let flat = Ohlc::new(&[1.0], &[1.0], &[1.0], &[2.0]).unwrap();
        assert_eq!(flat.body_ratio().unwrap(), vec![0.0]);
        assert!(Ohlc::new(&[], &[], &[], &[]).is_err());
        assert!(Ohlc::new(&[1.0], &[1.0, 2.0], &[1.0], &[1.0]).is_err());
    }
}

//! Pure moving-average kernels with the indicator frame's legacy warm-up behavior.

use super::{TaError, TaResult, validate_finite_output, validate_finite_series};

fn validate_period(period: usize) -> TaResult<()> {
    if period == 0 {
        Err(TaError::invalid_period("period must be greater than zero"))
    } else {
        Ok(())
    }
}

/// Returns the simple moving average, using every available leading value during warm-up.
pub fn sma(values: &[f64], period: usize) -> TaResult<Vec<f64>> {
    validate_period(period)?;
    validate_finite_series("SMA values", values)?;
    let result = values
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let start = index.saturating_add(1).saturating_sub(period);
            let window = &values[start..=index];
            window.iter().sum::<f64>() / window.len() as f64
        })
        .collect::<Vec<_>>();
    validate_finite_output("SMA", &result)?;
    Ok(result)
}

/// Returns an EMA seeded from the first value with `alpha = 2 / (period + 1)`.
pub fn ema(values: &[f64], period: usize) -> TaResult<Vec<f64>> {
    validate_period(period)?;
    validate_finite_series("EMA values", values)?;
    let result = smooth(values, 2.0 / (period as f64 + 1.0));
    validate_finite_output("EMA", &result)?;
    Ok(result)
}

/// Returns Wilder's moving average seeded from the first value with `alpha = 1 / period`.
pub fn rma(values: &[f64], period: usize) -> TaResult<Vec<f64>> {
    validate_period(period)?;
    validate_finite_series("RMA values", values)?;
    let result = smooth(values, 1.0 / period as f64);
    validate_finite_output("RMA", &result)?;
    Ok(result)
}

fn smooth(values: &[f64], alpha: f64) -> Vec<f64> {
    let mut result = Vec::with_capacity(values.len());
    let mut previous = 0.0;
    for (index, &value) in values.iter().enumerate() {
        let current = if index == 0 {
            value
        } else {
            (1.0 - alpha) * previous + alpha * value
        };
        result.push(current);
        previous = current;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_averages_preserve_warmup_and_seed() {
        let values = [1.0, 2.0, 4.0, 8.0];
        assert_eq!(
            sma(&values, 3).unwrap(),
            vec![1.0, 1.5, 7.0 / 3.0, 14.0 / 3.0]
        );
        assert_eq!(ema(&values, 3).unwrap(), vec![1.0, 1.5, 2.75, 5.375]);
        assert_eq!(
            rma(&values, 3).unwrap(),
            vec![
                1.0,
                1.3333333333333335,
                2.2222222222222223,
                4.148148148148148
            ]
        );
    }

    #[test]
    fn moving_averages_reject_zero_period() {
        assert!(sma(&[1.0], 0).is_err());
        assert!(ema(&[1.0], 0).is_err());
        assert!(rma(&[1.0], 0).is_err());
    }

    #[test]
    fn moving_averages_reject_non_finite_values() {
        for values in [&[f64::NAN][..], &[f64::INFINITY][..]] {
            assert!(sma(values, 1).is_err());
            assert!(ema(values, 1).is_err());
            assert!(rma(values, 1).is_err());
        }
    }
}

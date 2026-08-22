//! Rolling risk metric kernels.
use super::ma::sma;
use super::{TaResult, validate_finite_output, validate_finite_series};

/// Returns legacy rolling Sharpe values, yielding zero for short or zero-stdev windows.
pub fn sharpe(close: &[f64], period: usize) -> TaResult<Vec<f64>> {
    validate_finite_series("Sharpe close", close)?;
    let close_sma = sma(close, period)?;
    let deviations = close
        .iter()
        .zip(close_sma)
        .map(|(&value, mean)| value - mean)
        .collect::<Vec<_>>();
    let result = close
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let start = index.saturating_add(1).saturating_sub(period);
            let window = &close[start..=index];
            if window.len() < 2 {
                return 0.0;
            }
            let mean = window.iter().sum::<f64>() / window.len() as f64;
            let stdev = (window
                .iter()
                .map(|value| (value - mean).powi(2))
                .sum::<f64>()
                / (window.len() - 1) as f64)
                .sqrt();
            if stdev == 0.0 {
                0.0
            } else {
                deviations[start..=index].iter().sum::<f64>() / period as f64 / stdev
            }
        })
        .collect::<Vec<_>>();
    validate_finite_output("Sharpe", &result)?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_short_and_zero_stdev_behavior() {
        assert_eq!(sharpe(&[3.0, 3.0, 3.0], 2).unwrap(), vec![0.0; 3]);
        assert_eq!(sharpe(&[1.0], 3).unwrap(), vec![0.0]);
    }

    #[test]
    fn rejects_non_finite_close_values() {
        assert!(sharpe(&[1.0, f64::NAN], 2).is_err());
    }
}

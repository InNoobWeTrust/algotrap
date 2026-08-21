//! Relative-strength-index kernels.

use super::ma::rma;
use super::{
    TaError, TaResult, validate_finite_output, validate_finite_series, validate_finite_value,
};

/// Returns RSI with zero change seeded as neutral 50 and zero losses as 100.
pub fn rsi(values: &[f64], period: usize) -> TaResult<Vec<f64>> {
    validate_finite_series("RSI values", values)?;
    let (gains, losses) = changes(values);
    let gains = rma(&gains, period)?;
    let losses = rma(&losses, period)?;
    let result = gains
        .into_iter()
        .zip(losses)
        .map(|(gain, loss)| {
            if loss == 0.0 && gain == 0.0 {
                50.0
            } else if loss == 0.0 {
                100.0
            } else {
                100.0 - 100.0 / (1.0 + gain / loss)
            }
        })
        .collect::<Vec<_>>();
    validate_finite_output("RSI", &result)?;
    Ok(result)
}

/// Returns the next value required to reach the requested RSI target.
pub fn reverse_rsi(values: &[f64], period: usize, target: f64) -> TaResult<Vec<f64>> {
    validate_finite_series("reverse RSI values", values)?;
    validate_finite_value("reverse RSI target", target)?;
    if !(0.0 < target && target < 100.0) {
        return Err(TaError::validation(
            "reverse RSI target must be strictly between 0 and 100",
        ));
    }
    let (gains, losses) = changes(values);
    let gains = rma(&gains, period)?;
    let losses = rma(&losses, period)?;
    let result = values
        .iter()
        .zip(gains)
        .zip(losses)
        .map(|((&source, gain), loss)| {
            let target_ratio = target / (100.0 - target);
            let reverse_ratio = (100.0 - target) / target;
            let x = (period - 1) as f64 * (loss * target_ratio - gain);
            if x >= 0.0 {
                source + x
            } else {
                source + x * reverse_ratio
            }
        })
        .collect::<Vec<_>>();
    validate_finite_output("reverse RSI", &result)?;
    Ok(result)
}

fn changes(values: &[f64]) -> (Vec<f64>, Vec<f64>) {
    values
        .iter()
        .enumerate()
        .map(|(index, &value)| {
            let difference = index
                .checked_sub(1)
                .map(|previous| value - values[previous])
                .unwrap_or(0.0);
            (difference.max(0.0), (-difference).max(0.0))
        })
        .unzip()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_seed_and_zero_gain_loss_semantics() {
        assert_eq!(
            rsi(&[5.0, 5.0, 6.0, 5.0], 2).unwrap(),
            vec![50.0, 50.0, 100.0, 33.33333333333333]
        );
        assert_eq!(reverse_rsi(&[5.0, 5.0], 2, 50.0).unwrap(), vec![5.0, 5.0]);
    }

    #[test]
    fn rejects_invalid_inputs() {
        for target in [0.0, 100.0, f64::NAN, f64::INFINITY] {
            assert!(reverse_rsi(&[5.0, 5.0], 2, target).is_err());
        }
        assert!(rsi(&[1.0, f64::NAN], 2).is_err());
        assert!(reverse_rsi(&[1.0, f64::INFINITY], 2, 50.0).is_err());
    }
}

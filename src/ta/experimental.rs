//! Composite experimental technical-analysis kernels.
use super::{TaError, TaResult, validate_finite_output, validate_finite_series};
use super::{
    ma::{ema, rma, sma},
    ohlc::Ohlc,
    rsi::rsi,
};

/// Returns RSSI from `open + bar_bias`.
pub fn rssi(ohlc: Ohlc<'_>, period: usize) -> TaResult<Vec<f64>> {
    let result = rsi(
        &ohlc
            .open()
            .iter()
            .zip(ohlc.bar_bias()?)
            .map(|(&open, bias)| open + bias)
            .collect::<Vec<_>>(),
        period,
    )?;
    validate_finite_output("RSSI", &result)?;
    Ok(result)
}

/// Returns the unsmoothed open-minus-bias-RMA reversion primitive.
pub fn bias_reversion(ohlc: Ohlc<'_>, period: usize) -> TaResult<Vec<f64>> {
    let result = ohlc
        .open()
        .iter()
        .zip(rma(&ohlc.bar_bias()?, period)?)
        .map(|(&open, bias)| open - bias)
        .collect::<Vec<_>>();
    validate_finite_output("bias reversion", &result)?;
    Ok(result)
}
/// Returns bias reversion smoothed with an SMA of the same period.
pub fn bias_reversion_smoothed(ohlc: Ohlc<'_>, period: usize) -> TaResult<Vec<f64>> {
    sma(&bias_reversion(ohlc, period)?, period)
}
/// Returns signed distance of a signal from open-centered oscillation bands.
pub fn band_reversion(ohlc: Ohlc<'_>, oscillation: &[f64], signal: &[f64]) -> TaResult<Vec<f64>> {
    if oscillation.len() != ohlc.open().len() || signal.len() != ohlc.open().len() {
        return Err(TaError::validation("band inputs must match OHLC length"));
    }
    validate_finite_series("band oscillation", oscillation)?;
    validate_finite_series("band signal", signal)?;
    let result = ohlc
        .open()
        .iter()
        .zip(oscillation)
        .zip(signal)
        .map(|((&open, &oscillation), &signal)| {
            let upper = open + oscillation;
            let lower = open - oscillation;
            if lower <= signal && upper >= signal {
                0.0
            } else if signal - upper > 0.0 {
                signal - upper
            } else {
                (signal - lower).min(0.0)
            }
        })
        .collect::<Vec<_>>();
    validate_finite_output("band reversion", &result)?;
    Ok(result)
}
/// Returns band reversion as a percentage of the oscillation.
pub fn band_reversion_percent(
    ohlc: Ohlc<'_>,
    oscillation: &[f64],
    signal: &[f64],
) -> TaResult<Vec<f64>> {
    validate_finite_series("band percentage oscillation", oscillation)?;
    validate_finite_series("band percentage signal", signal)?;
    let result = band_reversion(ohlc, oscillation, signal)?
        .into_iter()
        .zip(oscillation)
        .map(|(value, &oscillation)| {
            if oscillation == 0.0 {
                0.0
            } else {
                100.0 * value / oscillation
            }
        })
        .collect::<Vec<_>>();
    validate_finite_output("band reversion percent", &result)?;
    Ok(result)
}
/// Smooths RSSI with the frame's EMA convention.
pub fn smooth_rssi(values: &[f64], period: usize) -> TaResult<Vec<f64>> {
    ema(values, period)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_kernels_preserve_validation_and_band_behavior() {
        let ohlc = Ohlc::new(&[10., 10., 10.], &[10.; 3], &[10.; 3], &[10.; 3]).unwrap();
        assert_eq!(
            band_reversion(ohlc, &[2., 2., 2.], &[11., 13., 7.]).unwrap(),
            vec![0., 1., -1.]
        );
        assert!(band_reversion(ohlc, &[f64::NAN; 3], &[10.; 3]).is_err());
        assert!(smooth_rssi(&[f64::NAN], 2).is_err());
    }

    #[test]
    fn bias_primitives_are_independent() {
        let ohlc = Ohlc::new(
            &[10., 11., 12.],
            &[12., 14., 15.],
            &[9., 10., 11.],
            &[11., 13., 12.],
        )
        .unwrap();
        assert_eq!(bias_reversion(ohlc, 2).unwrap(), vec![8., 8., 9.5]);
        assert_eq!(
            bias_reversion_smoothed(ohlc, 2).unwrap(),
            vec![8., 8., 8.75]
        );
    }
}

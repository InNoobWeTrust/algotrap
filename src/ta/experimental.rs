use polars::prelude::*;
use tap::Pipe;

use super::prelude::*;

/// Relative Structure Strength Index
/// Experimental index of relative strength based on `open + bar_bias`
pub fn rssi(ohlc: &Ohlc, len: usize) -> Expr {
    let bias = bar_bias(ohlc);
    let bar_pwr = ohlc[0].clone() + bias;

    bar_pwr.rsi(len)
}

/// Bias reversion
/// Calculate the reversion value based on moving average of bias and open price
pub fn bias_reversion(ohlc: &Ohlc, len: usize) -> Expr {
    let bias = bar_bias(ohlc);
    let bias_rma = rma(&bias, len);
    // Reversion value
    ohlc[0].clone() - bias_rma
}

/// Bias reversion smoothed
/// Calculate the reversion value based on moving average of bias and open price
/// The value is then smoothed using simple moving average of the same length
pub fn bias_reversion_smoothed(ohlc: &Ohlc, len: usize) -> Expr {
    bias_reversion(ohlc, len).pipe(|val| sma(&val, len))
}

/// Band Reversion
/// Bands that tend to keep aligned with the signal line inside
/// @returns the reversion distance from nearest band to signal line
pub fn band_reversion(ohlc: &Ohlc, osc: &Expr, signal: &Expr) -> Expr {
    let upper = ohlc[0].clone() + osc.clone();
    let lower = ohlc[0].clone() - osc.clone();
    let normal_lower = lower.clone().lt_eq(signal.clone());
    let normal_upper = upper.clone().gt_eq(signal.clone());
    // Dip value, lower cap at 0
    let dip_val = signal.clone() - upper.clone();
    let dip_val_clipped = dip_val.clip(lit(0), lit(f64::MAX));
    // Surge value, upper cap at 0
    let surge_val = signal.clone() - lower.clone();
    let surge_val_clipped = surge_val.clip(lit(f64::MIN), lit(0));

    // Reversion value, positive suggests reversing upward when negative suggests downward
    when(normal_lower.logical_and(normal_upper))
        .then(lit(0))
        .otherwise(
            when(dip_val_clipped.clone().gt(lit(0)))
                .then(dip_val_clipped)
                .otherwise(surge_val_clipped),
        )
}

/// Band Reversion as percentage of band's oscillation
/// @returns the reversion distance as percentage of oscillation
pub fn band_reversion_percent(ohlc: &Ohlc, osc: &Expr, signal: &Expr) -> Expr {
    let rev_val = band_reversion(ohlc, osc, signal);
    lit(100) * rev_val / osc.clone()
}

pub trait OhlcExperimental {
    fn rssi(&self, len: usize) -> Expr;
    fn bias_reversion(&self, len: usize) -> Expr;
    fn bias_reversion_smoothed(&self, len: usize) -> Expr;
    fn band_reversion(&self, osc: &Expr, signal: &Expr) -> Expr;
    fn band_reversion_percent(&self, osc: &Expr, signal: &Expr) -> Expr;
}
impl OhlcExperimental for Ohlc {
    fn rssi(&self, len: usize) -> Expr {
        rssi(self, len)
    }
    fn bias_reversion(&self, len: usize) -> Expr {
        bias_reversion(self, len)
    }
    fn bias_reversion_smoothed(&self, len: usize) -> Expr {
        bias_reversion_smoothed(self, len)
    }
    fn band_reversion(&self, osc: &Expr, signal: &Expr) -> Expr {
        band_reversion(self, osc, signal)
    }
    fn band_reversion_percent(&self, osc: &Expr, signal: &Expr) -> Expr {
        band_reversion_percent(self, osc, signal)
    }
}

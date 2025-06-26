use polars::prelude::*;
use tap::Pipe;

use crate::ta::{bar_bias, common, rma, rsi, sma, volatility::atr};

/// Relative Structure Strength Index
/// Experimental index of relative strength based on `open + bar_bias`
pub fn rssi(ohlc: &[Expr; 4], len: usize) -> Expr {
    let bias = common::bar_bias(ohlc);
    let bar_pwr = ohlc[0].clone() + bias;

    rsi(&bar_pwr, len)
}

/// Bias reversion
/// Calculate the reversion value based on moving average of bias and open price
pub fn bias_reversion(ohlc: &[Expr; 4], len: usize) -> Expr {
    let bias = bar_bias(ohlc);
    let bias_rma = rma(&bias, len);
    // Reversion value
    ohlc[0].clone() - bias_rma
}

/// Bias reversion smoothed
/// Calculate the reversion value based on moving average of bias and open price
/// The value is then smoothed using simple moving average of the same length
pub fn bias_reversion_smoothed(ohlc: &[Expr; 4], len: usize) -> Expr {
    bias_reversion(ohlc, len).pipe(|val| sma(&val, len))
}

/// ATR bands
pub fn atr_band(ohlc: &[Expr; 4], len: usize, mult: f64) -> [Expr; 2] {
    let atr_raw = atr(ohlc, len);
    let atr_osc = atr_raw * lit(mult);
    let prev_high = ohlc[1].clone().shift(lit(1));
    let prev_low = ohlc[1].clone().shift(lit(1));
    let upper_band = prev_high + atr_osc.clone();
    let lower_band = prev_low - atr_osc.clone();

    [upper_band, lower_band]
}

/// ATR Reversion
/// ATR bands tend to keep aligned with the bias reversion inside
/// @returns the reversion distance from nearest band to bias reversion value
pub fn atr_reversion(ohlc: &[Expr; 4], bias_len: usize, atr_len: usize, atr_mult: f64) -> Expr {
    let bias_val = bias_reversion_smoothed(ohlc, bias_len);
    let [upper_band, lower_band] = atr_band(ohlc, atr_len, atr_mult);
    let normal_lower = lower_band.clone().lt_eq(bias_val.clone());
    let normal_upper = upper_band.clone().gt_eq(bias_val.clone());
    // Dip value, lower cap at 0
    let dip_val = bias_val.clone() - upper_band.clone();
    let dip_val_clipped = dip_val.clip(lit(0), lit(f64::MAX));
    // Surge value, upper cap at 0
    let surge_val = bias_val.clone() - lower_band.clone();
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

/// ATR Reversion as percentage of ATR
/// @returns the ATR reversion distance as percentage of ATR
pub fn atr_reversion_percent(
    ohlc: &[Expr; 4],
    bias_len: usize,
    atr_len: usize,
    atr_mult: f64,
) -> Expr {
    let atr_val = atr(ohlc, atr_len) * lit(atr_mult);
    let atr_rev_val = atr_reversion(ohlc, bias_len, atr_len, atr_mult);
    lit(100) * atr_rev_val / atr_val
}

use polars::prelude::*;
use tap::Pipe;

use crate::ta::{bar_bias, common, rma, rsi, sma};

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

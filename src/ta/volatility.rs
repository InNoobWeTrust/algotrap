use polars::lazy::dsl::max_horizontal;
use polars::prelude::*;

use super::prelude::*;

/// True Range
pub fn true_range(ohlc: &Ohlc) -> Expr {
    let prev_close = ohlc[3].clone().shift(lit(1));
    let hl_range = ohlc[1].clone() - ohlc[2].clone();
    let hc_range = ohlc[1].clone() - prev_close.clone();
    let lc_range = ohlc[2].clone() - prev_close.clone();
    let max_range = max_horizontal([hl_range.clone(), hc_range.clone(), lc_range.clone()]).unwrap();

    when(prev_close.clone().is_nan())
        .then(hl_range)
        .otherwise(max_range)
}

/// ATR
pub fn atr(ohlc: &Ohlc, len: usize) -> Expr {
    let tr = true_range(ohlc);
    rma(&tr, len)
}

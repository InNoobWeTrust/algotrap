use polars::prelude::*;

use super::Ohlc;

pub fn bar_bias(ohlc: &Ohlc) -> Expr {
    let [open, high, low, close] = ohlc;

    let bull_pwr = high.clone() - open.clone();
    let bear_pwr = open.clone() - low.clone();
    let balance = close.clone() - open.clone();

    balance + bull_pwr - bear_pwr
}

use polars::prelude::*;

use crate::ta::{common, rsi};

/// Relative Structure Strength Index
/// Experimental index of relative strength based on `open + bar_bias`
pub fn rssi(ohlc: &[Expr; 4], len: usize) -> Expr {
    let bias = common::bar_bias(ohlc);

    rsi(&bias, len)
}

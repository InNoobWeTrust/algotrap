use super::*;
use polars::prelude::*;

/// Sharpe ratio
pub fn sharpe(src: &Expr, len: usize) -> Expr {
    let stdev = src.clone().rolling_std(RollingOptionsFixedWindow {
        window_size: len,
        min_periods: 0,
        weights: None,
        center: false,
        fn_params: None,
    });
    let stdev = when(stdev.clone().is_nan())
        .then(lit(f64::MAX))
        .otherwise(stdev);
    let avg_ret = (src.clone() - src.clone().sma(len)).rolling_sum(RollingOptionsFixedWindow {
        window_size: len,
        min_periods: 0,
        weights: None,
        center: false,
        fn_params: None,
    }) / lit(len as u64);
    let avg_ret = when(avg_ret.clone().is_nan())
        .then(lit(0))
        .otherwise(avg_ret);
    avg_ret / stdev
}

pub trait ExprMetric {
    fn sharpe(&self, len: usize) -> Expr;
}

impl ExprMetric for Expr {
    fn sharpe(&self, len: usize) -> Expr {
        sharpe(self, len)
    }
}

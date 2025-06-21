use polars::prelude::*;

/// Simple moving average
pub fn sma(src: &Expr, len: usize) -> Expr {
    src.clone().rolling_mean(RollingOptionsFixedWindow {
        window_size: len,
        min_periods: 0,
        weights: None,
        center: false,
        fn_params: None,
    })
}

/// Exponential moving average with alpha = 1 / length
pub fn rma(src: &Expr, len: usize) -> Expr {
    let alpha = 1. / (len as f64);
    src.clone().ewm_mean(EWMOptions {
        alpha,
        adjust: false,
        ..Default::default()
    })
}

/// Exponential moving average with alpha = 2 / (length + 1)
pub fn ema(src: &Expr, len: usize) -> Expr {
    let alpha = 2. / (len as f64 + 1.);
    src.clone().ewm_mean(EWMOptions {
        alpha,
        adjust: false,
        ..Default::default()
    })
}

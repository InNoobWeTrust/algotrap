use polars::prelude::*;

/// Calculate RSI value from a source column in dataframe
pub fn rsi(col_name: &str, len: usize) -> Expr {
    // Source values
    let src = col(col_name);

    // Diff
    let diff = src.diff(1, polars::series::ops::NullBehavior::Ignore);

    // Gains: positive changes
    let gains = when(diff.clone().gt(lit(0)))
        .then(diff.clone())
        .otherwise(lit(0));
    // Losses: negative changes (as positive values)
    let losses = when(diff.clone().lt(lit(0)))
        .then(-diff.clone())
        .otherwise(lit(0));

    // Rolling mean for gains and losses
    let avg_gain = gains.rolling_mean(RollingOptionsFixedWindow {
        window_size: len,
        min_periods: len,
        weights: None,
        center: false,
        fn_params: None,
    });
    let avg_loss = losses.rolling_mean(RollingOptionsFixedWindow {
        window_size: len,
        min_periods: len,
        weights: None,
        center: false,
        fn_params: None,
    });

    // RS calculation
    let rs = avg_gain / avg_loss;
    // RSI
    lit(100) - (lit(100) / (lit(1) + rs))
}

use polars::prelude::*;

use super::prelude::*;

/// Calculate RSI value from a source column in dataframe
pub fn rsi(src: &Expr, len: usize) -> Expr {
    // Diff
    let diff = src
        .clone()
        .diff(lit(1), polars::series::ops::NullBehavior::Ignore);

    // Gains: positive changes
    let gains = when(diff.clone().gt(lit(0)))
        .then(diff.clone())
        .otherwise(lit(0));
    // Losses: negative changes (as positive values)
    let losses = when(diff.clone().lt(lit(0)))
        .then(-diff.clone())
        .otherwise(lit(0));

    // Rolling mean for gains and losses
    let avg_gain = gains.rma(len);
    let avg_loss = losses.rma(len);

    // RS calculation
    let rs = avg_gain / avg_loss;
    // RSI
    let rsi = lit(100) - (lit(100) / (lit(1) + rs));

    // Normalize nan
    when(rsi.clone().is_nan()).then(lit(50)).otherwise(rsi)
}

/// Calculate target value by providing target rsi and reverse engineer the formula.
/// https://c.mql5.com/forextsd/forum/138/reverse_engineering_rsi.pdf
pub fn rev_rsi(src: &Expr, len: usize, rsi: f64) -> Expr {
    let exp_per = 2 * len - 1;
    let diff = src
        .clone()
        .diff(lit(1), polars::series::ops::NullBehavior::Ignore);
    // Gains: positive changes
    let gains = when(diff.clone().gt(lit(0)))
        .then(diff.clone())
        .otherwise(lit(0));
    // Losses: negative changes (as positive values)
    let losses = when(diff.clone().lt(lit(0)))
        .then(-diff.clone())
        .otherwise(lit(0));

    // Average gains and losses
    let avg_gain = gains.ema(exp_per);
    let avg_loss = losses.ema(exp_per);
    // x factor
    let x = lit((len - 1) as i64) * (avg_loss * lit(rsi / (100. - rsi)) - avg_gain);
    // RevEngRSI
    when(x.clone().gt_eq(lit(0)))
        .then(src.clone() + x.clone())
        .otherwise(src.clone() + x.clone() * lit((100. - rsi) / rsi))
}

pub trait ExprRsi {
    fn rsi(&self, len: usize) -> Self;
    fn rev_rsi(&self, len: usize, rsi: f64) -> Self;
}

/// Chaining
impl ExprRsi for Expr {
    fn rsi(&self, len: usize) -> Self {
        rsi(self, len)
    }
    fn rev_rsi(&self, len: usize, rsi: f64) -> Self {
        rev_rsi(self, len, rsi)
    }
}

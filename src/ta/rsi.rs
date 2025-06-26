use polars::prelude::*;

use crate::ta::rma;

/// Calculate RSI value from a source column in dataframe
pub fn rsi(src: &Expr, len: usize) -> Expr {
    // Diff
    let diff = src
        .clone()
        .diff(1, polars::series::ops::NullBehavior::Ignore);

    // Gains: positive changes
    let gains = when(diff.clone().gt(lit(0)))
        .then(diff.clone())
        .otherwise(lit(0));
    // Losses: negative changes (as positive values)
    let losses = when(diff.clone().lt(lit(0)))
        .then(-diff.clone())
        .otherwise(lit(0));

    // Rolling mean for gains and losses
    let avg_gain = rma(&gains, len);
    let avg_loss = rma(&losses, len);

    // RS calculation
    let rs = avg_gain / avg_loss;
    // RSI
    let rsi = lit(100) - (lit(100) / (lit(1) + rs));

    // Normalize nan
    when(rsi.clone().is_nan()).then(lit(50)).otherwise(rsi)
}

pub trait ExprRsi {
    fn rsi(&self, len: usize) -> Self;
}

/// Chaining
impl ExprRsi for Expr {
    fn rsi(&self, len: usize) -> Self {
        rsi(self, len)
    }
}

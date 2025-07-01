use polars::prelude::*;

use super::prelude::*;

pub type Ohlc = [Expr; 4];

pub trait OhlcBias {
    fn bar_bias(&self) -> Expr;
}
impl OhlcBias for Ohlc {
    fn bar_bias(&self) -> Expr {
        bar_bias(self)
    }
}

pub trait OhlcAtr {
    fn true_range(&self) -> Expr;
    fn atr(&self, len: usize) -> Expr;
}
impl OhlcAtr for Ohlc {
    fn true_range(&self) -> Expr {
        true_range(self)
    }
    fn atr(&self, len: usize) -> Expr {
        atr(self, len)
    }
}

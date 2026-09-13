//! Typed Kernel and indicator exports for application aggregates.

pub use super::error::{TaError, TaErrorKind, TaResult};
pub use super::kernel::{Kernel, KernelStep, PriorState};
pub use super::ops::{
    Atr, AtrState, BandPoint, BandReversion, BandReversionPercent, BandReversionPercentState,
    BandReversionState, BarBias, BarBiasState, BiasReversion, BiasReversionInput,
    BiasReversionState, BodyRatio, BodyRatioState, Ema, EmaState, IsAtrGap, IsAtrGapState,
    ReverseRsi, ReverseRsiState, Rma, RmaState, Rsi, RsiState, Sharpe, SharpeState, Sma, SmaState,
    atr, atr_percent, band_reversion, band_reversion_percent, bar_bias, bias_reversion, body_ratio,
    ema, is_atr_gap, option_map2, require_output, reverse_rsi, rma, rsi, sharpe, sma,
};
pub use super::processor::Processor;

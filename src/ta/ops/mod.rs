//! Stateful technical-analysis transforms.

mod averages;
mod bands;
mod bias_reversion;
mod candle;
mod gap_candidate;
mod rsi;
mod sharpe;
mod support;

pub use averages::{Ema, EmaState, Rma, RmaState, Sma, SmaState, ema, rma, sma};
pub use bands::{
    BandPoint, BandReversion, BandReversionPercent, BandReversionPercentState, BandReversionState,
    IsAtrGap, IsAtrGapState, band_reversion, band_reversion_percent, is_atr_gap,
};
pub use bias_reversion::{BiasReversion, BiasReversionInput, BiasReversionState, bias_reversion};
pub use candle::{
    Atr, AtrState, BarBias, BarBiasState, BodyRatio, BodyRatioState, atr, bar_bias, body_ratio,
};
pub use gap_candidate::{
    GapCandidateDirection, GapCandidateFacts, GapCandidateInput, gap_candidate_facts,
};
pub use rsi::{ReverseRsi, ReverseRsiState, Rsi, RsiState, reverse_rsi, rsi};
pub use sharpe::{Sharpe, SharpeState, sharpe};
pub use support::{atr_percent, option_map2, require_output};

//! Cryptobot-specific indicator projection and presentation contract.

use algotrap::adapter::{Stamped, StreamPipelineError, kernel_stream_pipeline};
use algotrap::engine::error::MarketError;
use algotrap::engine::frame::{SourceColumnData, SourceFrame};
use algotrap::engine::traits::ComputedFrame;
use algotrap::engine::validation::ValidatedTicker;
use algotrap::prelude::Kline;
use algotrap::query::RawQuery;
use algotrap::query::duckdb::DuckDBQuery;
use algotrap::query::gap_zones::{GapZoneDirection, GapZoneRecord, recent_gap_zones};
use algotrap::ta::ops::{GapCandidateDirection, GapCandidateInput, gap_candidate_facts};
use algotrap::ta::prelude::{
    Atr, AtrState, BandPoint, BandReversion, BandReversionPercent, BandReversionPercentState,
    BandReversionState, BarBias, BarBiasState, BiasReversion, BiasReversionInput,
    BiasReversionState, BodyRatio, BodyRatioState, Ema, EmaState, IsAtrGap, IsAtrGapState, Kernel,
    KernelStep, PriorState, ReverseRsi, ReverseRsiState, Rma, RmaState, Sma, SmaState, TaError,
    TaResult, atr, atr_percent, band_reversion, band_reversion_percent, bar_bias, bias_reversion,
    body_ratio, ema, is_atr_gap, option_map2, require_output, reverse_rsi, rma, sma,
};
use algotrap::ta::{LeapMonthPolicy, iching_bar_trajectory, plum_blossom_signal_with_policy};
use chartlib::{DatasetKey, GapDirection, GapZone, InteractiveDataset};
use futures::TryStreamExt;
use std::convert::Infallible;

const VOLUME_EMA_PERIOD: usize = 20;
const EMA_PERIOD: usize = 200;
const REVERSE_RSI_PERIOD: usize = 14;
const ATR_PERIOD: usize = 42;
const BIAS_PERIOD: usize = 9;
const STRUCTURE_PERIOD: usize = 9;
const STRUCTURE_SMA_PERIOD: usize = 16;

/// App-level default body-ratio threshold for gap-candidate qualification.
///
/// Pure TA (`gap_candidate_facts`) owns validation and the
/// `is_atr_gap && body_ratio >= threshold` rule over the inclusive domain
/// `[0.0, 1.0]`; Cryptobot owns this caller-supplied default value.
const GAP_CANDIDATE_BODY_RATIO_THRESHOLD: f64 = 0.618;
const ATR_BAND_MULTIPLIER: f64 = 1.618;
const ATR_GAP_MULTIPLIER: f64 = 1.0;

pub(crate) struct CryptoIndicators {
    bar_bias: BarBias,
    atr: Atr,
    volume_ema: Ema,
    ema200: Ema,
    bias_reversion: BiasReversion,
    neutral_revrsi: ReverseRsi,
    bullish_revrsi: ReverseRsi,
    bearish_revrsi: ReverseRsi,
    structure_power: Rma,
    structure_power_sma: Sma,
    body_ratio: BodyRatio,
    band_reversion: BandReversion,
    band_reversion_percent: BandReversionPercent,
    is_atr_gap: IsAtrGap,
    atr_multiplier: f64,
    body_ratio_threshold: f64,
}

pub(crate) struct CryptoIndicatorState {
    bar_bias: BarBiasState,
    atr: AtrState,
    volume_ema: EmaState,
    ema200: EmaState,
    bias_reversion: BiasReversionState,
    neutral_revrsi: ReverseRsiState,
    bullish_revrsi: ReverseRsiState,
    bearish_revrsi: ReverseRsiState,
    structure_power: RmaState,
    structure_power_sma: SmaState,
    body_ratio: BodyRatioState,
    band_reversion: BandReversionState,
    band_reversion_percent: BandReversionPercentState,
    is_atr_gap: IsAtrGapState,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CryptoIndicatorRow {
    pub atr: Option<f64>,
    pub volume_sma: Option<f64>,
    pub ema200: Option<f64>,
    pub bias_reversion: Option<f64>,
    pub neutral_revrsi: Option<f64>,
    pub bullish_revrsi: Option<f64>,
    pub bearish_revrsi: Option<f64>,
    pub atr_upperband: Option<f64>,
    pub atr_lowerband: Option<f64>,
    pub iching_original_energy: Option<f64>,
    pub iching_transformed_energy: Option<f64>,
    pub iching_nuclear_energy: Option<f64>,
    pub structure_power: Option<f64>,
    pub structure_power_sma: Option<f64>,
    pub atr_percent: Option<f64>,
    pub atr_reversion_percent: Option<f64>,
    pub band_reversion: Option<f64>,
    pub body_ratio: Option<f64>,
    pub is_atr_gap: Option<bool>,
    pub gap_candidate_qualifies: bool,
    pub gap_candidate_body_bottom: Option<f64>,
    pub gap_candidate_body_top: Option<f64>,
    pub gap_candidate_direction: Option<GapCandidateDirection>,
}

impl CryptoIndicators {
    fn new() -> Self {
        Self {
            bar_bias: bar_bias(),
            atr: atr(ATR_PERIOD),
            volume_ema: ema(VOLUME_EMA_PERIOD),
            ema200: ema(EMA_PERIOD),
            bias_reversion: bias_reversion(BIAS_PERIOD),
            neutral_revrsi: reverse_rsi(REVERSE_RSI_PERIOD, 50.0),
            bullish_revrsi: reverse_rsi(REVERSE_RSI_PERIOD, 70.0),
            bearish_revrsi: reverse_rsi(REVERSE_RSI_PERIOD, 30.0),
            structure_power: rma(STRUCTURE_PERIOD),
            structure_power_sma: sma(STRUCTURE_SMA_PERIOD),
            body_ratio: body_ratio(),
            band_reversion: band_reversion(ATR_BAND_MULTIPLIER),
            band_reversion_percent: band_reversion_percent(ATR_BAND_MULTIPLIER),
            is_atr_gap: is_atr_gap(ATR_GAP_MULTIPLIER),
            atr_multiplier: ATR_BAND_MULTIPLIER,
            body_ratio_threshold: GAP_CANDIDATE_BODY_RATIO_THRESHOLD,
        }
    }
}

impl Kernel for CryptoIndicators {
    type Input = Kline;
    type Output = CryptoIndicatorRow;
    type State = CryptoIndicatorState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        kline: &Kline,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        let datetime = chrono::DateTime::from_timestamp_millis(kline.time).ok_or_else(|| {
            TaError::validation(format!("Kline time {} is out of range", kline.time))
        })?;
        let signal = plum_blossom_signal_with_policy(datetime, LeapMonthPolicy::Allow)?;
        let transformed = signal
            .transformed
            .ok_or_else(|| TaError::computation("Plum Blossom transformed channel is missing"))?;
        let bias_step = self
            .bar_bias
            .transition(child_prior(&prior, |state| &state.bar_bias), kline)?;
        let atr_step = self
            .atr
            .transition(child_prior(&prior, |state| &state.atr), kline)?;
        let volume_step = self.volume_ema.transition(
            child_prior(&prior, |state| &state.volume_ema),
            &kline.volume,
        )?;
        let ema_step = self
            .ema200
            .transition(child_prior(&prior, |state| &state.ema200), &kline.close)?;
        let bias = require_output("bar bias", bias_step.output)?;
        let atr = require_output("ATR", atr_step.output)?;
        let bias_reversion_step = self.bias_reversion.transition(
            child_prior(&prior, |state| &state.bias_reversion),
            &BiasReversionInput {
                open: kline.open,
                bias,
            },
        )?;
        let neutral_revrsi_step = self.neutral_revrsi.transition(
            child_prior(&prior, |state| &state.neutral_revrsi),
            &(kline.open + bias),
        )?;
        let bullish_revrsi_step = self.bullish_revrsi.transition(
            child_prior(&prior, |state| &state.bullish_revrsi),
            &kline.high,
        )?;
        let bearish_revrsi_step = self.bearish_revrsi.transition(
            child_prior(&prior, |state| &state.bearish_revrsi),
            &kline.low,
        )?;
        let structure_step = self
            .structure_power
            .transition(child_prior(&prior, |state| &state.structure_power), &bias)?;
        let structure_value = require_output("structure power", structure_step.output)?;
        let structure_sma_step = self.structure_power_sma.transition(
            child_prior(&prior, |state| &state.structure_power_sma),
            &structure_value,
        )?;
        let body_step = self
            .body_ratio
            .transition(child_prior(&prior, |state| &state.body_ratio), kline)?;
        let bias_reversion_value = require_output("bias reversion", bias_reversion_step.output)?;
        let band_point = BandPoint {
            open: kline.open,
            atr,
            signal: bias_reversion_value,
        };
        let band_step = self.band_reversion.transition(
            child_prior(&prior, |state| &state.band_reversion),
            &band_point,
        )?;
        let band_percent_step = self.band_reversion_percent.transition(
            child_prior(&prior, |state| &state.band_reversion_percent),
            &band_point,
        )?;
        let gap_step = self.is_atr_gap.transition(
            child_prior(&prior, |state| &state.is_atr_gap),
            &BandPoint {
                open: kline.open,
                atr,
                signal: kline.close,
            },
        )?;
        let body_ratio = require_output("body ratio", body_step.output)?;
        let is_atr_gap = gap_step
            .output
            .ok_or_else(|| TaError::computation("ATR gap output is missing"))?;
        let candidate = gap_candidate_facts(GapCandidateInput {
            open: kline.open,
            close: kline.close,
            is_atr_gap,
            body_ratio,
            body_ratio_threshold: self.body_ratio_threshold,
        })?;
        let output = CryptoIndicatorRow {
            atr: Some(atr),
            volume_sma: volume_step.output,
            ema200: ema_step.output,
            bias_reversion: bias_reversion_step.output,
            neutral_revrsi: neutral_revrsi_step.output,
            bullish_revrsi: bullish_revrsi_step.output,
            bearish_revrsi: bearish_revrsi_step.output,
            atr_upperband: option_map2(Some(kline.open), Some(atr), |open, atr| {
                open + atr * self.atr_multiplier
            })?,
            atr_lowerband: option_map2(Some(kline.open), Some(atr), |open, atr| {
                open - atr * self.atr_multiplier
            })?,
            iching_original_energy: Some(signal.original.energy),
            iching_transformed_energy: Some(transformed.energy),
            iching_nuclear_energy: Some(signal.nuclear.energy),
            structure_power: Some(structure_value),
            structure_power_sma: structure_sma_step.output,
            atr_percent: option_map2(Some(atr), Some(kline.open), atr_percent)?,
            atr_reversion_percent: band_percent_step.output,
            band_reversion: band_step.output,
            body_ratio: Some(body_ratio),
            is_atr_gap: Some(is_atr_gap),
            gap_candidate_qualifies: candidate.qualifies,
            gap_candidate_body_bottom: candidate.body_bottom,
            gap_candidate_body_top: candidate.body_top,
            gap_candidate_direction: candidate.direction,
        };
        let next_state = CryptoIndicatorState {
            bar_bias: bias_step.next_state,
            atr: atr_step.next_state,
            volume_ema: volume_step.next_state,
            ema200: ema_step.next_state,
            bias_reversion: bias_reversion_step.next_state,
            neutral_revrsi: neutral_revrsi_step.next_state,
            bullish_revrsi: bullish_revrsi_step.next_state,
            bearish_revrsi: bearish_revrsi_step.next_state,
            structure_power: structure_step.next_state,
            structure_power_sma: structure_sma_step.next_state,
            body_ratio: body_step.next_state,
            band_reversion: band_step.next_state,
            band_reversion_percent: band_percent_step.next_state,
            is_atr_gap: gap_step.next_state,
        };
        Ok(KernelStep { output, next_state })
    }
}

fn child_prior<'a, Parent, Child>(
    prior: &'a PriorState<'_, Parent>,
    select: impl FnOnce(&Parent) -> &Child,
) -> PriorState<'a, Child> {
    match prior {
        PriorState::Initial => PriorState::Initial,
        PriorState::Existing(state) => PriorState::Existing(select(state)),
    }
}

async fn collect_crypto_rows(klines: &[Kline]) -> Result<Vec<CryptoIndicatorRow>, MarketError> {
    let source =
        futures::stream::iter(klines.iter().cloned().enumerate().map(|(id, value)| {
            Ok::<_, StreamPipelineError<Infallible, usize>>(Stamped { id, value })
        }));
    let stamped_rows: Vec<Stamped<usize, CryptoIndicatorRow>> =
        kernel_stream_pipeline(source, CryptoIndicators::new())
            .try_collect()
            .await
            .map_err(map_crypto_stream_error)?;

    if stamped_rows.len() != klines.len()
        || stamped_rows
            .iter()
            .enumerate()
            .any(|(position, stamped)| stamped.id != position)
    {
        return Err(MarketError::computation(
            "stamped row cardinality or source-position mismatch",
        ));
    }

    Ok(stamped_rows
        .into_iter()
        .map(|stamped| stamped.value)
        .collect())
}

fn map_crypto_stream_error(error: StreamPipelineError<Infallible, usize>) -> MarketError {
    match error {
        StreamPipelineError::Source(source) => match source {},
        StreamPipelineError::Ta(error) => MarketError::from(error),
        StreamPipelineError::Lagged { skipped } => {
            MarketError::computation(format!("indicator source lagged by {skipped} rows"))
        }
        StreamPipelineError::Alignment { left, right } => MarketError::computation(format!(
            "indicator source alignment mismatch: left {left}, right {right}",
        )),
        StreamPipelineError::OutputClosed => {
            MarketError::computation("indicator output closed before completion")
        }
        StreamPipelineError::TaskJoin { message } => {
            MarketError::computation(format!("indicator task failed: {message}"))
        }
    }
}

fn crypto_output_frame(
    rows: Vec<CryptoIndicatorRow>,
    klines: &[Kline],
) -> Result<SourceFrame, MarketError> {
    if rows.len() != klines.len() {
        return Err(MarketError::computation(
            "indicator row count does not match market row count",
        ));
    }

    let trajectories = klines
        .iter()
        .enumerate()
        .map(|(index, kline)| {
            let bar_close_time = klines.get(index + 1).map_or(kline.time, |next| next.time);
            iching_bar_trajectory(kline.time, bar_close_time).map_err(MarketError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let numbers = |select: fn(&CryptoIndicatorRow) -> Option<f64>| {
        SourceColumnData::Number(rows.iter().map(select).collect())
    };
    let columns = vec![
        (
            "open".into(),
            SourceColumnData::Number(klines.iter().map(|row| Some(row.open)).collect()),
        ),
        (
            "high".into(),
            SourceColumnData::Number(klines.iter().map(|row| Some(row.high)).collect()),
        ),
        (
            "low".into(),
            SourceColumnData::Number(klines.iter().map(|row| Some(row.low)).collect()),
        ),
        (
            "close".into(),
            SourceColumnData::Number(klines.iter().map(|row| Some(row.close)).collect()),
        ),
        (
            "volume".into(),
            SourceColumnData::Number(klines.iter().map(|row| Some(row.volume)).collect()),
        ),
        (
            "time".into(),
            SourceColumnData::Number(klines.iter().map(|row| Some(row.time as f64)).collect()),
        ),
        (
            "adj_close".into(),
            SourceColumnData::Number(klines.iter().map(|row| row.adjclose).collect()),
        ),
        ("atr".into(), numbers(|row| row.atr)),
        ("volume_sma".into(), numbers(|row| row.volume_sma)),
        ("ema200".into(), numbers(|row| row.ema200)),
        ("bias_reversion".into(), numbers(|row| row.bias_reversion)),
        ("neutral_revrsi".into(), numbers(|row| row.neutral_revrsi)),
        ("bullish_revrsi".into(), numbers(|row| row.bullish_revrsi)),
        ("bearish_revrsi".into(), numbers(|row| row.bearish_revrsi)),
        ("atr_upperband".into(), numbers(|row| row.atr_upperband)),
        ("atr_lowerband".into(), numbers(|row| row.atr_lowerband)),
        (
            "iching_original_energy".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.energy_open))
                    .collect(),
            ),
        ),
        (
            "iching_transformed_energy".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.transformed_open))
                    .collect(),
            ),
        ),
        (
            "iching_nuclear_energy".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.nuclear_open))
                    .collect(),
            ),
        ),
        (
            "iching_open".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.energy_open))
                    .collect(),
            ),
        ),
        (
            "iching_high".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.energy_high))
                    .collect(),
            ),
        ),
        (
            "iching_low".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.energy_low))
                    .collect(),
            ),
        ),
        (
            "iching_close".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.energy_close))
                    .collect(),
            ),
        ),
        (
            "iching_moving_line".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| trajectory.moving_line.map(f64::from))
                    .collect(),
            ),
        ),
        (
            "iching_transformed_close".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.transformed_close))
                    .collect(),
            ),
        ),
        (
            "iching_nuclear_close".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.nuclear_close))
                    .collect(),
            ),
        ),
        ("structure_power".into(), numbers(|row| row.structure_power)),
        (
            "structure_power_sma".into(),
            numbers(|row| row.structure_power_sma),
        ),
        ("atr_percent".into(), numbers(|row| row.atr_percent)),
        (
            "atr_reversion_percent".into(),
            numbers(|row| row.atr_reversion_percent),
        ),
        ("band_reversion".into(), numbers(|row| row.band_reversion)),
        ("body_ratio".into(), numbers(|row| row.body_ratio)),
        (
            "is_atr_gap".into(),
            SourceColumnData::Boolean(rows.iter().map(|row| row.is_atr_gap).collect()),
        ),
        (
            "gap_candidate_qualifies".into(),
            SourceColumnData::Boolean(
                rows.iter()
                    .map(|row| Some(row.gap_candidate_qualifies))
                    .collect(),
            ),
        ),
        (
            "gap_candidate_body_bottom".into(),
            numbers(|row| row.gap_candidate_body_bottom),
        ),
        (
            "gap_candidate_body_top".into(),
            numbers(|row| row.gap_candidate_body_top),
        ),
        (
            "gap_candidate_direction".into(),
            SourceColumnData::Text(
                rows.iter()
                    .map(|row| match row.gap_candidate_direction {
                        Some(GapCandidateDirection::Bullish) => Some("bullish".to_string()),
                        Some(GapCandidateDirection::Bearish) => Some("bearish".to_string()),
                        Some(GapCandidateDirection::Flat) => Some("flat".to_string()),
                        None => None,
                    })
                    .collect(),
            ),
        ),
    ];

    SourceFrame::from_columns(columns)
}

/// Computes and projects one cryptobot chart frame using its source-controlled
/// raw SQL contract, together with the precomputed recent gap zones.
pub async fn compute_crypto_frame(
    klines: Vec<Kline>,
    ticker: ValidatedTicker,
) -> Result<(Box<dyn ComputedFrame>, Vec<GapZoneRecord>), MarketError> {
    let decision_time_ms = klines.last().map(|kline| kline.time).unwrap_or(i64::MAX);
    let rows = collect_crypto_rows(&klines).await?;
    if rows.len() != klines.len() {
        return Err(MarketError::computation(
            "indicator row count does not match market row count",
        ));
    }
    let source = crypto_output_frame(rows.clone(), &klines)?;
    let (sl_percent, tol_percent) = ticker.risk_percentages();
    let query = RawQuery::source_controlled(build_crypto_sql(sl_percent, tol_percent));

    let projected = DuckDBQuery::new()
        .project(source.clone(), query)
        .map(|frame| Box::new(frame) as Box<dyn ComputedFrame>)?;
    let zones = recent_gap_zones(source, decision_time_ms, 64)?.zones;
    Ok((projected, zones))
}

/// Adapts one projected Cryptobot frame without dropping its chart columns or color fields.
pub(crate) fn adapt_chartlib_dataset(
    ticker: &str,
    timeframe: &str,
    display_symbol: &str,
    frame: &dyn ComputedFrame,
    zones: &[GapZoneRecord],
) -> Result<InteractiveDataset, MarketError> {
    Ok(InteractiveDataset {
        key: DatasetKey::new(ticker, timeframe),
        display_symbol: display_symbol.to_owned(),
        records: frame.to_json_records()?,
        gap_zones: zones
            .iter()
            .map(|zone| GapZone {
                time_ms: zone.time_ms,
                open: zone.open,
                high: zone.high,
                low: zone.low,
                close: zone.close,
                volume: zone.volume,
                body_bottom: zone.body_bottom,
                body_top: zone.body_top,
                body_ratio: zone.body_ratio,
                direction: match zone.direction {
                    GapZoneDirection::Bullish => GapDirection::Bullish,
                    GapZoneDirection::Bearish => GapDirection::Bearish,
                    GapZoneDirection::Flat => GapDirection::Flat,
                },
            })
            .collect(),
    })
}

fn build_crypto_sql(sl_percent: f64, tol_percent: f64) -> String {
    let risk_adjustment = sql_double(sl_percent / (1.0 + tol_percent));
    format!(
        "WITH crypto_base AS (SELECT *, CAST(time AS VARCHAR) AS \"Date\" FROM computed()) SELECT {} FROM crypto_base ORDER BY time",
        crypto_select_expressions(&risk_adjustment).join(", ")
    )
}

fn crypto_select_expressions(risk_adjustment: &str) -> Vec<String> {
    vec![
        "open", "high", "low", "close", "volume", "time", "adj_close", "\"Date\"", "atr",
        "CASE WHEN close >= open THEN 'rgba(76, 175, 80, 0.3)' ELSE 'rgba(242, 54, 69, 0.3)' END AS volume_color",
        "volume_sma", "bias_reversion", "'rgba(178, 181, 190, 0.2)' AS bias_reversion_color", "ema200", "'rgba(156, 39, 176, 0.5)' AS ema200_color",
        "neutral_revrsi", "'rgba(178,181,190,0.2)' AS neutral_revrsi_color", "bullish_revrsi", "'rgba(33,150,243,0.2)' AS bullish_revrsi_color",
        "bearish_revrsi", "'rgba(255,152,0,0.2)' AS bearish_revrsi_color", "atr_upperband", "'rgba(76, 175, 80, 0.2)' AS atr_upperband_color",
        "atr_lowerband", "'rgba(242, 54, 69, 0.2)' AS atr_lowerband_color", "iching_original_energy",
        "iching_transformed_energy", "iching_nuclear_energy", "structure_power",
        "iching_open", "iching_high", "iching_low", "iching_close", "iching_moving_line",
        "iching_transformed_close", "iching_nuclear_close",
        "CASE WHEN structure_power >= 0.0 THEN 'rgba(0, 137, 123, 1)' ELSE 'rgba(136, 14, 79, 1)' END AS structure_power_color",
        "structure_power_sma", "3.0 * structure_power - 2.0 * structure_power_sma AS structure_power_direction", "atr_percent", "atr_reversion_percent",
        "CASE WHEN atr_reversion_percent > 50.0 THEN 'rgba(76, 175, 80, 0.5)' WHEN atr_reversion_percent < -50.0 THEN 'rgba(242, 54, 69, 0.5)' ELSE 'rgba(41, 98, 255, 0.2)' END AS atr_reversion_percent_color",
        &format!("CASE WHEN atr - atr = 0 AND atr <> 0.0 THEN {risk_adjustment} * open / atr ELSE NULL END AS leverage"),
        "is_atr_gap", "body_ratio",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn sql_double(value: f64) -> String {
    let value = if value.is_finite() {
        value.to_string()
    } else {
        "NULL".to_string()
    };
    format!("CAST({value} AS DOUBLE)")
}

#[cfg(test)]
mod tests {
    use super::{
        CryptoIndicatorRow, CryptoIndicatorState, CryptoIndicators, build_crypto_sql, child_prior,
        collect_crypto_rows, compute_crypto_frame, crypto_output_frame,
    };
    use algotrap::engine::error::ErrorKind;
    use algotrap::engine::frame::SourceColumnData;
    use algotrap::engine::traits::ComputedFrame;
    use algotrap::engine::validation::ValidatedTicker;
    use algotrap::prelude::Kline;
    use algotrap::ta::ops::{GapCandidateDirection, GapCandidateInput, gap_candidate_facts};
    use algotrap::ta::prelude::PriorState;

    #[test]
    fn crypto_sql_is_source_controlled_and_never_embeds_ohlc_literals() {
        let sl_percent = 0.02;
        let tol_percent = 0.01;
        let sql = build_crypto_sql(sl_percent, tol_percent);

        assert!(sql.contains("computed()"));
        assert!(!sql.contains("100.0"));
        assert!(sql.contains("ORDER BY time"));
        assert!(sql.contains(&format!(
            "CAST({} AS DOUBLE)",
            sl_percent / (1.0 + tol_percent)
        )));

        let ordered_aliases = [
            "AS volume_color",
            "AS bias_reversion_color",
            "AS ema200_color",
            "AS structure_power_direction",
            "AS leverage",
        ];
        let positions = ordered_aliases
            .iter()
            .map(|alias| {
                sql.find(alias)
                    .unwrap_or_else(|| panic!("missing SQL alias {alias}"))
            })
            .collect::<Vec<_>>();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        for present in [
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_nuclear_energy",
        ] {
            assert!(sql.contains(present), "{present} must appear in SQL");
        }
        for removed in [
            "rssi",
            "rssi_ma",
            "rssi_direction",
            "rssi_color",
            "climax_signal",
            "sharpe",
            "sharpe_color",
            "trust",
        ] {
            assert!(!sql.contains(removed), "{removed} must be absent from SQL");
        }
    }

    #[tokio::test]
    async fn crypto_frame_preserves_presentation_schema_and_supplied_rows() {
        let klines = klines();
        let (frame, _zones) = compute_crypto_frame(klines.clone(), ticker())
            .await
            .unwrap();

        assert_eq!(frame.columns().first().map(String::as_str), Some("open"));
        assert!(frame.has_column("leverage"));
        assert_eq!(frame.len(), klines.len());
        assert_eq!(
            frame.f64_at("time", 0).unwrap(),
            Some(klines[0].time as f64)
        );
    }

    #[tokio::test]
    async fn collection_preserves_empty_one_and_multi_row_cardinality_order_and_association() {
        assert!(collect_crypto_rows(&[]).await.unwrap().is_empty());

        let candles = klines();
        let one = collect_crypto_rows(&candles[..1]).await.unwrap();
        assert_eq!(one.len(), 1);
        assert_rows_match(&direct_aggregate_rows(&candles[..1])[0], &one[0]);
        assert!(
            (one[0].atr_upperband.unwrap()
                - one[0].atr.unwrap() * super::ATR_BAND_MULTIPLIER
                - candles[0].open)
                .abs()
                <= 1e-12
        );

        let selected = [19, 2, 87, 4];
        let supplied = selected
            .iter()
            .map(|&index| candles[index])
            .collect::<Vec<_>>();
        let collected = collect_crypto_rows(&supplied).await.unwrap();
        let direct = direct_aggregate_rows(&supplied);

        assert_eq!(collected.len(), supplied.len());
        for (position, (expected, actual)) in direct.iter().zip(&collected).enumerate() {
            assert_rows_match(expected, actual);
            assert!(
                (actual.atr_upperband.unwrap()
                    - actual.atr.unwrap() * super::ATR_BAND_MULTIPLIER
                    - supplied[position].open)
                    .abs()
                    <= 1e-12,
                "row {position} is not associated with its supplied candle"
            );
        }
    }

    #[tokio::test]
    async fn stamped_pipeline_emits_exact_zero_based_source_ids() {
        use algotrap::adapter::{Stamped, StreamPipelineError, kernel_stream_pipeline};
        use futures::TryStreamExt;
        use std::convert::Infallible;

        let supplied = klines().into_iter().take(5).collect::<Vec<_>>();
        let source =
            futures::stream::iter(supplied.iter().cloned().enumerate().map(|(id, value)| {
                Ok::<_, StreamPipelineError<Infallible, usize>>(Stamped { id, value })
            }));
        let stamped: Vec<Stamped<usize, CryptoIndicatorRow>> =
            match kernel_stream_pipeline(source, CryptoIndicators::new())
                .try_collect()
                .await
            {
                Ok(rows) => rows,
                Err(_) => panic!("valid stamped source must collect"),
            };

        assert_eq!(
            stamped.iter().map(|row| row.id).collect::<Vec<_>>(),
            (0..supplied.len()).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn invalid_kline_terminates_collection_and_compute_without_a_frame() {
        let mut invalid = klines();
        invalid[1].high = f64::NAN;

        let collection_error = collect_crypto_rows(&invalid).await.unwrap_err();
        assert_eq!(collection_error.kind, ErrorKind::ValidationError);

        let compute_error = match compute_crypto_frame(invalid, ticker()).await {
            Ok(_) => panic!("invalid Kline must not produce a frame"),
            Err(error) => error,
        };
        assert_eq!(compute_error.kind, ErrorKind::ValidationError);
    }

    #[test]
    fn projector_empty_preserves_all_30_names_types_and_zero_lengths() {
        let frame = crypto_output_frame(vec![], &[]).unwrap();
        let expected = [
            ("open", "number"),
            ("high", "number"),
            ("low", "number"),
            ("close", "number"),
            ("volume", "number"),
            ("time", "number"),
            ("adj_close", "number"),
            ("atr", "number"),
            ("volume_sma", "number"),
            ("ema200", "number"),
            ("bias_reversion", "number"),
            ("neutral_revrsi", "number"),
            ("bullish_revrsi", "number"),
            ("bearish_revrsi", "number"),
            ("atr_upperband", "number"),
            ("atr_lowerband", "number"),
            ("iching_original_energy", "number"),
            ("iching_transformed_energy", "number"),
            ("iching_nuclear_energy", "number"),
            ("iching_open", "number"),
            ("iching_high", "number"),
            ("iching_low", "number"),
            ("iching_close", "number"),
            ("iching_moving_line", "number"),
            ("iching_transformed_close", "number"),
            ("iching_nuclear_close", "number"),
            ("structure_power", "number"),
            ("structure_power_sma", "number"),
            ("atr_percent", "number"),
            ("atr_reversion_percent", "number"),
            ("band_reversion", "number"),
            ("body_ratio", "number"),
            ("is_atr_gap", "boolean"),
            ("gap_candidate_qualifies", "boolean"),
            ("gap_candidate_body_bottom", "number"),
            ("gap_candidate_body_top", "number"),
            ("gap_candidate_direction", "text"),
        ];

        assert_eq!(frame.len(), 0);
        assert_eq!(frame.column_names(), expected.map(|(name, _)| name));
        for (name, kind) in expected {
            match (kind, frame.column(name)) {
                ("number", Some(SourceColumnData::Number(values))) => assert!(values.is_empty()),
                ("boolean", Some(SourceColumnData::Boolean(values))) => assert!(values.is_empty()),
                ("text", Some(SourceColumnData::Text(values))) => assert!(values.is_empty()),
                _ => panic!("{name} must be an empty {kind} column"),
            }
        }
    }

    #[test]
    fn projector_rejects_row_kline_cardinality_mismatch_with_computation_error() {
        let error = crypto_output_frame(vec![direct_aggregate_rows(&klines()[..1]).remove(0)], &[])
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::ComputationError);
        assert_eq!(
            error.message,
            "indicator row count does not match market row count"
        );
    }

    #[tokio::test]
    async fn deterministic_aggregate_collection_and_duckdb_parity_cover_all_fields_and_schema() {
        let candles = klines();
        let direct = direct_aggregate_rows(&candles);
        let collected = collect_crypto_rows(&candles).await.unwrap();
        let positions = [0, 250];

        for position in positions {
            assert_rows_match(&direct[position], &collected[position]);
        }

        let (frame, _zones) = compute_crypto_frame(candles.clone(), ticker())
            .await
            .unwrap();
        let expected_schema = vec![
            "open",
            "high",
            "low",
            "close",
            "volume",
            "time",
            "adj_close",
            "Date",
            "atr",
            "volume_color",
            "volume_sma",
            "bias_reversion",
            "bias_reversion_color",
            "ema200",
            "ema200_color",
            "neutral_revrsi",
            "neutral_revrsi_color",
            "bullish_revrsi",
            "bullish_revrsi_color",
            "bearish_revrsi",
            "bearish_revrsi_color",
            "atr_upperband",
            "atr_upperband_color",
            "atr_lowerband",
            "atr_lowerband_color",
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_nuclear_energy",
            "structure_power",
            "iching_open",
            "iching_high",
            "iching_low",
            "iching_close",
            "iching_moving_line",
            "iching_transformed_close",
            "iching_nuclear_close",
            "structure_power_color",
            "structure_power_sma",
            "structure_power_direction",
            "atr_percent",
            "atr_reversion_percent",
            "atr_reversion_percent_color",
            "leverage",
            "is_atr_gap",
            "body_ratio",
        ];
        assert_eq!(frame.columns(), expected_schema);
        assert_eq!(frame.len(), candles.len());
        for present in [
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_nuclear_energy",
            "iching_open",
            "iching_high",
            "iching_low",
            "iching_close",
            "iching_moving_line",
            "iching_transformed_close",
            "iching_nuclear_close",
        ] {
            assert!(frame.has_column(present), "{present} must be projected");
        }
        for removed in [
            "rssi",
            "rssi_ma",
            "rssi_direction",
            "rssi_color",
            "climax_signal",
            "climax_signal_pos",
            "climax_signal_color",
            "climax_signal_shape",
            "sharpe",
            "sharpe_color",
            "trust",
        ] {
            assert!(!frame.has_column(removed), "{removed} must be absent");
        }
        for source_only in [
            "gap_candidate_qualifies",
            "gap_candidate_body_bottom",
            "gap_candidate_body_top",
            "gap_candidate_direction",
        ] {
            assert!(
                !frame.has_column(source_only),
                "{source_only} must remain source-only and not leak into the chart projection"
            );
        }
        let sql = build_crypto_sql(0.02, 0.01);
        for source_only in [
            "gap_candidate_qualifies",
            "gap_candidate_body_bottom",
            "gap_candidate_body_top",
            "gap_candidate_direction",
        ] {
            assert!(
                !sql.contains(source_only),
                "{source_only} must not appear in the chart-facing SQL"
            );
        }

        let mature = positions[1];
        assert_option_f64_close(
            frame.f64_at("open", mature).unwrap(),
            Some(candles[mature].open),
            "open",
        );
        assert_option_f64_close(
            frame.f64_at("atr", mature).unwrap(),
            direct[mature].atr,
            "atr",
        );
        assert_option_f64_close(
            frame.f64_at("iching_original_energy", mature).unwrap(),
            direct[mature].iching_original_energy,
            "iching_original_energy",
        );
        assert_option_f64_close(
            frame.f64_at("iching_transformed_energy", mature).unwrap(),
            direct[mature].iching_transformed_energy,
            "iching_transformed_energy",
        );
        assert_option_f64_close(
            frame.f64_at("iching_nuclear_energy", mature).unwrap(),
            direct[mature].iching_nuclear_energy,
            "iching_nuclear_energy",
        );
        assert_option_f64_close(
            frame.f64_at("body_ratio", mature).unwrap(),
            direct[mature].body_ratio,
            "body_ratio",
        );
        assert_option_f64_close(
            frame.f64_at("structure_power_direction", mature).unwrap(),
            Some(
                3.0 * direct[mature].structure_power.unwrap()
                    - 2.0 * direct[mature].structure_power_sma.unwrap(),
            ),
            "structure_power_direction",
        );
        assert_option_f64_close(
            frame.f64_at("leverage", mature).unwrap(),
            Some(0.02 / (1.0 + 0.01) * candles[mature].open / direct[mature].atr.unwrap()),
            "leverage",
        );
        assert_eq!(
            frame.string_at("volume_color", mature).unwrap().as_deref(),
            Some("rgba(76, 175, 80, 0.3)")
        );
        assert_eq!(
            frame.string_at("Date", mature).unwrap(),
            Some(format!("{}.0", candles[mature].time))
        );
    }

    #[test]
    fn u4_1_reuses_u1_owned_exact_19_field_row_and_fixed_periods() {
        assert_eq!(super::VOLUME_EMA_PERIOD, 20);
        assert_eq!(super::EMA_PERIOD, 200);
        assert_eq!(super::REVERSE_RSI_PERIOD, 14);
        assert_eq!(super::ATR_PERIOD, 42);
        assert_eq!(super::BIAS_PERIOD, 9);
        assert_eq!(super::STRUCTURE_PERIOD, 9);
        assert_eq!(super::STRUCTURE_SMA_PERIOD, 16);

        fn number(_: Option<f64>) {}
        fn boolean(_: Option<bool>) {}
        fn candidate_bool(_: bool) {}
        fn candidate_direction(_: Option<GapCandidateDirection>) {}

        let mut processor = algotrap::ta::prelude::Processor::new(CryptoIndicators::new());
        let row = processor.process(&klines()[0]).unwrap();

        number(row.atr);
        number(row.volume_sma);
        number(row.ema200);
        number(row.bias_reversion);
        number(row.neutral_revrsi);
        number(row.bullish_revrsi);
        number(row.bearish_revrsi);
        number(row.atr_upperband);
        number(row.atr_lowerband);
        number(row.iching_original_energy);
        number(row.iching_transformed_energy);
        number(row.iching_nuclear_energy);
        number(row.structure_power);
        number(row.structure_power_sma);
        number(row.atr_percent);
        number(row.atr_reversion_percent);
        number(row.band_reversion);
        number(row.body_ratio);
        boolean(row.is_atr_gap);
        candidate_bool(row.gap_candidate_qualifies);
        number(row.gap_candidate_body_bottom);
        number(row.gap_candidate_body_top);
        candidate_direction(row.gap_candidate_direction);
    }

    #[test]
    fn u4_1_projects_complete_ordered_pre_duckdb_schema_with_typed_columns() {
        let mut candles = klines();
        candles.truncate(2);
        candles[0].adjclose = Some(100.25);
        let rows = vec![
            CryptoIndicatorRow {
                atr: Some(1.0),
                volume_sma: Some(2.0),
                ema200: Some(3.0),
                bias_reversion: Some(4.0),
                neutral_revrsi: Some(5.0),
                bullish_revrsi: Some(6.0),
                bearish_revrsi: Some(7.0),
                atr_upperband: Some(8.0),
                atr_lowerband: Some(9.0),
                iching_original_energy: Some(10.0),
                iching_transformed_energy: Some(11.0),
                iching_nuclear_energy: Some(11.5),
                structure_power: Some(12.0),
                structure_power_sma: Some(13.0),
                atr_percent: Some(14.0),
                atr_reversion_percent: Some(15.0),
                band_reversion: Some(16.0),
                body_ratio: Some(18.0),
                is_atr_gap: Some(true),
                gap_candidate_qualifies: true,
                gap_candidate_body_bottom: Some(100.0),
                gap_candidate_body_top: Some(100.5),
                gap_candidate_direction: Some(GapCandidateDirection::Bullish),
            },
            CryptoIndicatorRow {
                atr: None,
                volume_sma: Some(22.0),
                ema200: Some(23.0),
                bias_reversion: Some(24.0),
                neutral_revrsi: Some(25.0),
                bullish_revrsi: Some(26.0),
                bearish_revrsi: Some(27.0),
                atr_upperband: Some(28.0),
                atr_lowerband: Some(29.0),
                iching_original_energy: Some(30.0),
                iching_transformed_energy: Some(31.0),
                iching_nuclear_energy: Some(-31.5),
                structure_power: Some(32.0),
                structure_power_sma: Some(33.0),
                atr_percent: Some(34.0),
                atr_reversion_percent: Some(35.0),
                band_reversion: Some(36.0),
                body_ratio: Some(38.0),
                is_atr_gap: None,
                gap_candidate_qualifies: false,
                gap_candidate_body_bottom: None,
                gap_candidate_body_top: None,
                gap_candidate_direction: None,
            },
        ];
        let trajectories = candles
            .iter()
            .map(|candle| {
                algotrap::ta::iching_bar_trajectory(candle.time, candle.time + 60_000).unwrap()
            })
            .collect::<Vec<_>>();

        let expected_columns = vec![
            (
                "open".to_string(),
                SourceColumnData::Number(vec![Some(100.0), Some(101.0)]),
            ),
            (
                "high".to_string(),
                SourceColumnData::Number(vec![Some(102.0), Some(103.0)]),
            ),
            (
                "low".to_string(),
                SourceColumnData::Number(vec![Some(99.0), Some(100.0)]),
            ),
            (
                "close".to_string(),
                SourceColumnData::Number(vec![Some(100.5), Some(101.5)]),
            ),
            (
                "volume".to_string(),
                SourceColumnData::Number(vec![Some(1_000.0), Some(1_000.0)]),
            ),
            (
                "time".to_string(),
                SourceColumnData::Number(vec![
                    Some(1_700_000_000_000.0),
                    Some(1_700_000_060_000.0),
                ]),
            ),
            (
                "adj_close".to_string(),
                SourceColumnData::Number(vec![Some(100.25), None]),
            ),
            (
                "atr".to_string(),
                SourceColumnData::Number(vec![Some(1.0), None]),
            ),
            (
                "volume_sma".to_string(),
                SourceColumnData::Number(vec![Some(2.0), Some(22.0)]),
            ),
            (
                "ema200".to_string(),
                SourceColumnData::Number(vec![Some(3.0), Some(23.0)]),
            ),
            (
                "bias_reversion".to_string(),
                SourceColumnData::Number(vec![Some(4.0), Some(24.0)]),
            ),
            (
                "neutral_revrsi".to_string(),
                SourceColumnData::Number(vec![Some(5.0), Some(25.0)]),
            ),
            (
                "bullish_revrsi".to_string(),
                SourceColumnData::Number(vec![Some(6.0), Some(26.0)]),
            ),
            (
                "bearish_revrsi".to_string(),
                SourceColumnData::Number(vec![Some(7.0), Some(27.0)]),
            ),
            (
                "atr_upperband".to_string(),
                SourceColumnData::Number(vec![Some(8.0), Some(28.0)]),
            ),
            (
                "atr_lowerband".to_string(),
                SourceColumnData::Number(vec![Some(9.0), Some(29.0)]),
            ),
            (
                "iching_original_energy".to_string(),
                SourceColumnData::Number(
                    trajectories
                        .iter()
                        .map(|trajectory| Some(trajectory.energy_open))
                        .collect(),
                ),
            ),
            (
                "iching_transformed_energy".to_string(),
                SourceColumnData::Number(
                    trajectories
                        .iter()
                        .map(|trajectory| Some(trajectory.transformed_open))
                        .collect(),
                ),
            ),
            (
                "iching_nuclear_energy".to_string(),
                SourceColumnData::Number(
                    trajectories
                        .iter()
                        .map(|trajectory| Some(trajectory.nuclear_open))
                        .collect(),
                ),
            ),
            (
                "iching_open".to_string(),
                SourceColumnData::Number(
                    trajectories
                        .iter()
                        .map(|trajectory| Some(trajectory.energy_open))
                        .collect(),
                ),
            ),
            (
                "iching_high".to_string(),
                SourceColumnData::Number(
                    trajectories
                        .iter()
                        .map(|trajectory| Some(trajectory.energy_high))
                        .collect(),
                ),
            ),
            (
                "iching_low".to_string(),
                SourceColumnData::Number(
                    trajectories
                        .iter()
                        .map(|trajectory| Some(trajectory.energy_low))
                        .collect(),
                ),
            ),
            (
                "iching_close".to_string(),
                SourceColumnData::Number(
                    trajectories
                        .iter()
                        .map(|trajectory| Some(trajectory.energy_close))
                        .collect(),
                ),
            ),
            (
                "iching_moving_line".to_string(),
                SourceColumnData::Number(
                    trajectories
                        .iter()
                        .map(|trajectory| trajectory.moving_line.map(f64::from))
                        .collect(),
                ),
            ),
            (
                "iching_transformed_close".to_string(),
                SourceColumnData::Number(
                    trajectories
                        .iter()
                        .map(|trajectory| Some(trajectory.transformed_close))
                        .collect(),
                ),
            ),
            (
                "iching_nuclear_close".to_string(),
                SourceColumnData::Number(
                    trajectories
                        .iter()
                        .map(|trajectory| Some(trajectory.nuclear_close))
                        .collect(),
                ),
            ),
            (
                "structure_power".to_string(),
                SourceColumnData::Number(vec![Some(12.0), Some(32.0)]),
            ),
            (
                "structure_power_sma".to_string(),
                SourceColumnData::Number(vec![Some(13.0), Some(33.0)]),
            ),
            (
                "atr_percent".to_string(),
                SourceColumnData::Number(vec![Some(14.0), Some(34.0)]),
            ),
            (
                "atr_reversion_percent".to_string(),
                SourceColumnData::Number(vec![Some(15.0), Some(35.0)]),
            ),
            (
                "band_reversion".to_string(),
                SourceColumnData::Number(vec![Some(16.0), Some(36.0)]),
            ),
            (
                "body_ratio".to_string(),
                SourceColumnData::Number(vec![Some(18.0), Some(38.0)]),
            ),
            (
                "is_atr_gap".to_string(),
                SourceColumnData::Boolean(vec![Some(true), None]),
            ),
            (
                "gap_candidate_qualifies".to_string(),
                SourceColumnData::Boolean(vec![Some(true), Some(false)]),
            ),
            (
                "gap_candidate_body_bottom".to_string(),
                SourceColumnData::Number(vec![Some(100.0), None]),
            ),
            (
                "gap_candidate_body_top".to_string(),
                SourceColumnData::Number(vec![Some(100.5), None]),
            ),
            (
                "gap_candidate_direction".to_string(),
                SourceColumnData::Text(vec![Some("bullish".to_string()), None]),
            ),
        ];

        let frame = crypto_output_frame(rows, &candles).unwrap();
        let expected_names = expected_columns
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            frame.column_names(),
            expected_names
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        );

        for (name, expected) in expected_columns {
            let actual = frame
                .column(&name)
                .unwrap_or_else(|| panic!("missing {name}"));
            match expected {
                SourceColumnData::Number(_) => {
                    assert!(matches!(actual, SourceColumnData::Number(_)))
                }
                SourceColumnData::Boolean(_) => {
                    assert!(matches!(actual, SourceColumnData::Boolean(_)))
                }
                SourceColumnData::Text(_) => assert!(matches!(actual, SourceColumnData::Text(_))),
            }
            assert_eq!(actual, &expected, "{name}");
        }
    }

    #[test]
    fn aggregate_contract_has_exact_row_and_child_state_shapes() {
        fn assert_row(row: CryptoIndicatorRow) {
            let CryptoIndicatorRow {
                atr,
                volume_sma,
                ema200,
                bias_reversion,
                neutral_revrsi,
                bullish_revrsi,
                bearish_revrsi,
                atr_upperband,
                atr_lowerband,
                iching_original_energy,
                iching_transformed_energy,
                iching_nuclear_energy,
                structure_power,
                structure_power_sma,
                atr_percent,
                atr_reversion_percent,
                band_reversion,
                body_ratio,
                is_atr_gap,
                gap_candidate_qualifies,
                gap_candidate_body_bottom,
                gap_candidate_body_top,
                gap_candidate_direction,
                ..
            } = row;
            let _ = (
                atr,
                volume_sma,
                ema200,
                bias_reversion,
                neutral_revrsi,
                bullish_revrsi,
                bearish_revrsi,
                atr_upperband,
                atr_lowerband,
                iching_original_energy,
                iching_transformed_energy,
                iching_nuclear_energy,
                structure_power,
                structure_power_sma,
                atr_percent,
                atr_reversion_percent,
                band_reversion,
                body_ratio,
                is_atr_gap,
                gap_candidate_qualifies,
                gap_candidate_body_bottom,
                gap_candidate_body_top,
                gap_candidate_direction,
            );
        }

        fn assert_state(state: CryptoIndicatorState) {
            let CryptoIndicatorState {
                bar_bias,
                atr,
                volume_ema,
                ema200,
                bias_reversion,
                neutral_revrsi,
                bullish_revrsi,
                bearish_revrsi,
                structure_power,
                structure_power_sma,
                body_ratio,
                band_reversion,
                band_reversion_percent,
                is_atr_gap,
            } = state;
            let _ = (
                bar_bias,
                atr,
                volume_ema,
                ema200,
                bias_reversion,
                neutral_revrsi,
                bullish_revrsi,
                bearish_revrsi,
                structure_power,
                structure_power_sma,
                body_ratio,
                band_reversion,
                band_reversion_percent,
                is_atr_gap,
            );
        }

        let _ = (assert_row, assert_state);
    }

    #[test]
    fn aggregate_constructor_uses_locked_periods_and_emits_one_full_row_per_kline() {
        let mut processor = algotrap::ta::prelude::Processor::new(CryptoIndicators::new());

        for kline in klines().iter().take(3) {
            let row = processor.process(kline).unwrap();
            assert!(row.atr.is_some());
            assert!(row.iching_original_energy.is_some());
            assert!(row.iching_transformed_energy.is_some());
            assert!(row.iching_nuclear_energy.is_some());
            assert!(row.structure_power.is_some());
        }
    }

    #[test]
    fn aggregate_required_children_are_available_from_the_first_valid_candle() {
        let mut processor = algotrap::ta::prelude::Processor::new(CryptoIndicators::new());
        let row = processor.process(&klines()[0]).unwrap();

        assert!(row.atr.is_some());
        assert!(row.iching_original_energy.is_some());
        assert!(row.iching_transformed_energy.is_some());
        assert!(row.iching_nuclear_energy.is_some());
        assert!(row.structure_power.is_some());
        assert!(row.bias_reversion.is_some());
    }

    #[test]
    fn aggregate_child_prior_repeatedly_borrows_sibling_fields_without_moving_parent() {
        struct Parent {
            first: u8,
            second: u16,
        }

        let parent = Parent {
            first: 7,
            second: 11,
        };
        let prior = PriorState::Existing(&parent);
        let first = child_prior(&prior, |state| &state.first);
        let second = child_prior(&prior, |state| &state.second);

        assert!(matches!(first, PriorState::Existing(value) if *value == 7));
        assert!(matches!(second, PriorState::Existing(value) if *value == 11));
    }

    #[test]
    fn aggregate_transition_commits_only_one_complete_successor_without_child_processors() {
        let aggregate = CryptoIndicators::new();
        let mut processor = algotrap::ta::prelude::Processor::new(aggregate);
        let _ = processor.process(&klines()[0]).unwrap();
    }

    struct FailureSwitch(std::rc::Rc<std::cell::Cell<bool>>);

    impl FailureSwitch {
        fn arm(&self) {
            self.0.set(true);
        }
    }

    struct LaterChild {
        failure: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl algotrap::ta::prelude::Kernel for LaterChild {
        type Input = Kline;
        type Output = ();
        type State = ();

        fn transition(
            &self,
            _prior: PriorState<'_, Self::State>,
            _input: &Self::Input,
        ) -> algotrap::ta::prelude::TaResult<
            algotrap::ta::prelude::KernelStep<Self::Output, Self::State>,
        > {
            if self.failure.replace(false) {
                return Err(algotrap::ta::prelude::TaError::computation(
                    "injected later-child failure",
                ));
            }
            Ok(algotrap::ta::prelude::KernelStep {
                output: (),
                next_state: (),
            })
        }
    }

    struct FailureHarness {
        aggregate: CryptoIndicators,
        later: LaterChild,
    }

    struct FailureHarnessState {
        aggregate: CryptoIndicatorState,
        later: (),
    }

    impl FailureHarness {
        fn new() -> (Self, FailureSwitch) {
            let failure = std::rc::Rc::new(std::cell::Cell::new(false));
            (
                Self {
                    aggregate: CryptoIndicators::new(),
                    later: LaterChild {
                        failure: failure.clone(),
                    },
                },
                FailureSwitch(failure),
            )
        }
    }

    impl algotrap::ta::prelude::Kernel for FailureHarness {
        type Input = Kline;
        type Output = CryptoIndicatorRow;
        type State = FailureHarnessState;

        fn transition(
            &self,
            prior: PriorState<'_, Self::State>,
            input: &Self::Input,
        ) -> algotrap::ta::prelude::TaResult<
            algotrap::ta::prelude::KernelStep<Self::Output, Self::State>,
        > {
            let aggregate_step = self
                .aggregate
                .transition(child_prior(&prior, |state| &state.aggregate), input)?;
            self.later
                .transition(child_prior(&prior, |state| &state.later), input)?;

            Ok(algotrap::ta::prelude::KernelStep {
                output: aggregate_step.output,
                next_state: FailureHarnessState {
                    aggregate: aggregate_step.next_state,
                    later: (),
                },
            })
        }
    }

    fn assert_rows_match(expected: &CryptoIndicatorRow, actual: &CryptoIndicatorRow) {
        fn assert_option_f64(expected: Option<f64>, actual: Option<f64>, field: &str) {
            match (expected, actual) {
                (Some(expected), Some(actual)) => {
                    assert!(
                        (expected - actual).abs() <= 1e-12,
                        "{field}: {expected} != {actual}"
                    );
                }
                (None, None) => {}
                (expected, actual) => panic!("{field}: {expected:?} != {actual:?}"),
            }
        }

        assert_option_f64(expected.atr, actual.atr, "atr");
        assert_option_f64(expected.volume_sma, actual.volume_sma, "volume_sma");
        assert_option_f64(expected.ema200, actual.ema200, "ema200");
        assert_option_f64(
            expected.bias_reversion,
            actual.bias_reversion,
            "bias_reversion",
        );
        assert_option_f64(
            expected.neutral_revrsi,
            actual.neutral_revrsi,
            "neutral_revrsi",
        );
        assert_option_f64(
            expected.bullish_revrsi,
            actual.bullish_revrsi,
            "bullish_revrsi",
        );
        assert_option_f64(
            expected.bearish_revrsi,
            actual.bearish_revrsi,
            "bearish_revrsi",
        );
        assert_option_f64(
            expected.atr_upperband,
            actual.atr_upperband,
            "atr_upperband",
        );
        assert_option_f64(
            expected.atr_lowerband,
            actual.atr_lowerband,
            "atr_lowerband",
        );
        assert_option_f64(
            expected.iching_original_energy,
            actual.iching_original_energy,
            "iching_original_energy",
        );
        assert_option_f64(
            expected.iching_transformed_energy,
            actual.iching_transformed_energy,
            "iching_transformed_energy",
        );
        assert_option_f64(
            expected.iching_nuclear_energy,
            actual.iching_nuclear_energy,
            "iching_nuclear_energy",
        );
        assert_option_f64(
            expected.structure_power,
            actual.structure_power,
            "structure_power",
        );
        assert_option_f64(
            expected.structure_power_sma,
            actual.structure_power_sma,
            "structure_power_sma",
        );
        assert_option_f64(expected.atr_percent, actual.atr_percent, "atr_percent");
        assert_option_f64(
            expected.atr_reversion_percent,
            actual.atr_reversion_percent,
            "atr_reversion_percent",
        );
        assert_option_f64(
            expected.band_reversion,
            actual.band_reversion,
            "band_reversion",
        );
        assert_option_f64(expected.body_ratio, actual.body_ratio, "body_ratio");
        assert_eq!(expected.is_atr_gap, actual.is_atr_gap);
        assert_eq!(
            expected.gap_candidate_qualifies, actual.gap_candidate_qualifies,
            "gap_candidate_qualifies"
        );
        assert_option_f64(
            expected.gap_candidate_body_bottom,
            actual.gap_candidate_body_bottom,
            "gap_candidate_body_bottom",
        );
        assert_option_f64(
            expected.gap_candidate_body_top,
            actual.gap_candidate_body_top,
            "gap_candidate_body_top",
        );
        assert_eq!(
            expected.gap_candidate_direction, actual.gap_candidate_direction,
            "gap_candidate_direction"
        );
    }

    fn assert_option_f64_close(expected: Option<f64>, actual: Option<f64>, field: &str) {
        match (expected, actual) {
            (Some(expected), Some(actual)) => {
                assert!(
                    (expected - actual).abs() <= 1e-12,
                    "{field}: {expected} != {actual}"
                );
            }
            (None, None) => {}
            (expected, actual) => panic!("{field}: {expected:?} != {actual:?}"),
        }
    }

    fn direct_aggregate_rows(klines: &[Kline]) -> Vec<CryptoIndicatorRow> {
        let mut processor = algotrap::ta::prelude::Processor::new(CryptoIndicators::new());
        klines
            .iter()
            .map(|kline| processor.process(kline).unwrap())
            .collect()
    }

    #[test]
    fn aggregate_later_child_failure_preserves_outer_state_for_same_input_retry() {
        let (subject_kernel, failure) = FailureHarness::new();
        let (control_kernel, _control_failure) = FailureHarness::new();
        let mut subject = algotrap::ta::prelude::Processor::new(subject_kernel);
        let mut control = algotrap::ta::prelude::Processor::new(control_kernel);
        let candles = klines();

        subject.process(&candles[0]).unwrap();
        control.process(&candles[0]).unwrap();

        failure.arm();
        let error = subject.process(&candles[1]).unwrap_err();
        assert_eq!(error.kind, algotrap::ta::prelude::TaErrorKind::Computation);
        assert_eq!(error.message, "injected later-child failure");

        let retry = subject.process(&candles[1]).unwrap();
        let control_retry = control.process(&candles[1]).unwrap();
        assert_rows_match(&control_retry, &retry);

        let following = subject.process(&candles[2]).unwrap();
        let control_following = control.process(&candles[2]).unwrap();
        assert_rows_match(&control_following, &following);
    }

    fn ticker() -> ValidatedTicker {
        ValidatedTicker::new("BTCUSDT", 0.02, 0.01).unwrap()
    }

    fn focused_gap_klines() -> Vec<Kline> {
        let mut klines = Vec::with_capacity(53);
        let mut time = 1_700_000_000_000_i64;
        let push = |klines: &mut Vec<Kline>,
                    time: &mut i64,
                    open: f64,
                    high: f64,
                    low: f64,
                    close: f64| {
            klines.push(Kline {
                open,
                high,
                low,
                close,
                volume: 1_000.0,
                time: *time,
                adjclose: None,
            });
            *time += 60_000;
        };
        for _ in 0..50 {
            push(&mut klines, &mut time, 100.0, 100.5, 99.5, 100.1);
        }
        push(&mut klines, &mut time, 100.0, 109.0, 99.0, 108.0);
        push(&mut klines, &mut time, 200.0, 201.0, 190.0, 192.0);
        push(&mut klines, &mut time, 200.0, 200.5, 199.5, 200.1);
        klines
    }

    #[tokio::test]
    async fn gap_candidate_scalars_equal_shipped_pure_facts_for_every_supplied_row() {
        let supplied = focused_gap_klines();
        let collected = collect_crypto_rows(&supplied).await.unwrap();
        assert_eq!(collected.len(), supplied.len());

        let mut saw_qualifying = false;
        let mut saw_non_qualifying = false;
        for (position, (kline, row)) in supplied.iter().zip(&collected).enumerate() {
            let body_ratio = row
                .body_ratio
                .unwrap_or_else(|| panic!("row {position} must preserve upstream body_ratio"));
            let is_atr_gap = row
                .is_atr_gap
                .unwrap_or_else(|| panic!("row {position} must preserve upstream is_atr_gap"));
            let expected = gap_candidate_facts(GapCandidateInput {
                open: kline.open,
                close: kline.close,
                is_atr_gap,
                body_ratio,
                body_ratio_threshold: super::GAP_CANDIDATE_BODY_RATIO_THRESHOLD,
            })
            .unwrap_or_else(|_| panic!("row {position} has valid scalar inputs"));
            assert_eq!(
                row.gap_candidate_qualifies, expected.qualifies,
                "row {position} qualifies mismatch"
            );
            assert_eq!(
                row.gap_candidate_body_bottom, expected.body_bottom,
                "row {position} body_bottom mismatch"
            );
            assert_eq!(
                row.gap_candidate_body_top, expected.body_top,
                "row {position} body_top mismatch"
            );
            assert_eq!(
                row.gap_candidate_direction, expected.direction,
                "row {position} direction mismatch"
            );
            if expected.qualifies {
                saw_qualifying = true;
            } else {
                saw_non_qualifying = true;
                assert_eq!(row.gap_candidate_body_bottom, None);
                assert_eq!(row.gap_candidate_body_top, None);
                assert_eq!(row.gap_candidate_direction, None);
            }
        }
        assert!(
            saw_qualifying,
            "focused fixtures must include at least one qualifying row"
        );
        assert!(
            saw_non_qualifying,
            "focused fixtures must include at least one non-qualifying row"
        );
    }

    #[test]
    fn gap_candidate_body_ratio_threshold_default_is_app_owned() {
        assert!(
            (super::GAP_CANDIDATE_BODY_RATIO_THRESHOLD - 0.618).abs() <= f64::EPSILON,
            "app default must be 0.618"
        );
        assert!(
            (0.0..=1.0).contains(&super::GAP_CANDIDATE_BODY_RATIO_THRESHOLD),
            "app default must lie in domain [0.0, 1.0]"
        );
        let indicators = CryptoIndicators::new();
        assert!(
            (indicators.body_ratio_threshold - super::GAP_CANDIDATE_BODY_RATIO_THRESHOLD).abs()
                <= f64::EPSILON,
            "CryptoIndicators::new() must initialize from the app default"
        );
    }

    #[test]
    fn atr_band_multiplier_default_is_app_owned() {
        assert!(
            (super::ATR_BAND_MULTIPLIER - 1.618).abs() <= f64::EPSILON,
            "app default must be 1.618"
        );
        let indicators = CryptoIndicators::new();
        assert!(
            (indicators.atr_multiplier - super::ATR_BAND_MULTIPLIER).abs() <= f64::EPSILON,
            "CryptoIndicators::new() must initialize atr_multiplier from the app default"
        );

        let mut band = algotrap::ta::prelude::Processor::new(
            algotrap::ta::prelude::band_reversion(super::ATR_BAND_MULTIPLIER),
        );
        let band_point = algotrap::ta::prelude::BandPoint {
            open: 100.0,
            atr: 2.0,
            signal: 105.0,
        };
        let band_output = band.process(&band_point).unwrap().unwrap();
        assert!(
            (band_output - 1.764).abs() <= 1e-12,
            "band ops must use ATR_BAND_MULTIPLIER 1.618 (expected 1.764, got {band_output})"
        );

        let mut processor = algotrap::ta::prelude::Processor::new(CryptoIndicators::new());
        let kline = &klines()[0];
        let row = processor.process(kline).unwrap();
        let atr = row.atr.expect("ATR must be available");
        assert!(
            (row.atr_upperband.unwrap() - (kline.open + atr * super::ATR_BAND_MULTIPLIER)).abs()
                <= 1e-12,
            "ATR upper band must use ATR_BAND_MULTIPLIER"
        );
        assert!(
            (row.atr_lowerband.unwrap() - (kline.open - atr * super::ATR_BAND_MULTIPLIER)).abs()
                <= 1e-12,
            "ATR lower band must use ATR_BAND_MULTIPLIER"
        );
    }

    #[test]
    fn atr_gap_multiplier_default_is_app_owned_and_respects_boundary() {
        assert!(
            (super::ATR_GAP_MULTIPLIER - 1.0).abs() <= f64::EPSILON,
            "app default must be 1.0"
        );

        let open = 100.0;
        let atr = 2.0;
        let oscillation = atr * super::ATR_GAP_MULTIPLIER;
        assert!(
            (oscillation - 2.0).abs() <= f64::EPSILON,
            "ATR_GAP_MULTIPLIER 1.0 must give oscillation 2.0 for atr 2.0"
        );
        let mut gap = algotrap::ta::prelude::Processor::new(algotrap::ta::prelude::is_atr_gap(
            super::ATR_GAP_MULTIPLIER,
        ));
        assert_eq!(
            gap.process(&algotrap::ta::prelude::BandPoint {
                open,
                atr,
                signal: open + oscillation,
            })
            .unwrap(),
            Some(false),
            "exact open + atr*1.0 must not gap"
        );
        assert_eq!(
            gap.process(&algotrap::ta::prelude::BandPoint {
                open,
                atr,
                signal: open - oscillation,
            })
            .unwrap(),
            Some(false),
            "exact open - atr*1.0 must not gap"
        );
        let epsilon = 1e-9;
        assert_eq!(
            gap.process(&algotrap::ta::prelude::BandPoint {
                open,
                atr,
                signal: open + oscillation + epsilon,
            })
            .unwrap(),
            Some(true),
            "epsilon above open + atr*1.0 must gap"
        );
        assert_eq!(
            gap.process(&algotrap::ta::prelude::BandPoint {
                open,
                atr,
                signal: open - oscillation - epsilon,
            })
            .unwrap(),
            Some(true),
            "epsilon below open - atr*1.0 must gap"
        );

        let candles = klines();
        let rows = direct_aggregate_rows(&candles);
        assert_eq!(rows.len(), candles.len());
        for (position, (kline, row)) in candles.iter().zip(&rows).enumerate() {
            let atr = row
                .atr
                .unwrap_or_else(|| panic!("row {position} must preserve upstream atr"));
            let expected = kline.close > kline.open + atr * super::ATR_GAP_MULTIPLIER
                || kline.close < kline.open - atr * super::ATR_GAP_MULTIPLIER;
            assert_eq!(
                row.is_atr_gap,
                Some(expected),
                "row {position} CryptoIndicators::new() must respect ATR_GAP_MULTIPLIER 1.0"
            );
        }
    }

    #[test]
    fn gap_candidate_threshold_controls_transition_qualification() {
        let klines = focused_gap_klines();
        let default_rows = direct_aggregate_rows(&klines);
        let position = default_rows
            .iter()
            .position(|row| {
                row.is_atr_gap == Some(true)
                    && matches!(row.body_ratio, Some(body) if body > 0.0 && body < 1.0)
            })
            .expect("focused fixtures must contain an ATR-gap row with interior body_ratio");
        let body_ratio = default_rows[position].body_ratio.unwrap();
        let kline = &klines[position];

        let run_with_threshold = |threshold: f64| {
            let mut indicators = CryptoIndicators::new();
            indicators.body_ratio_threshold = threshold;
            let mut processor = algotrap::ta::prelude::Processor::new(indicators);
            let mut last = None;
            for candle in &klines[..=position] {
                last = Some(processor.process(candle).unwrap());
            }
            last.unwrap()
        };

        let permissive = run_with_threshold(0.0);
        assert!(
            permissive.gap_candidate_qualifies,
            "threshold 0.0 must qualify an ATR-gap row at position {position}"
        );
        assert_eq!(permissive.body_ratio, Some(body_ratio));
        assert_eq!(permissive.is_atr_gap, Some(true));

        let strict = run_with_threshold(1.0);
        assert!(
            !strict.gap_candidate_qualifies,
            "threshold 1.0 must reject interior body_ratio {body_ratio} at position {position}"
        );
        assert_eq!(strict.body_ratio, Some(body_ratio));
        assert_eq!(strict.is_atr_gap, Some(true));
        assert_eq!(strict.gap_candidate_body_bottom, None);
        assert_eq!(strict.gap_candidate_body_top, None);
        assert_eq!(strict.gap_candidate_direction, None);

        let expected_default = gap_candidate_facts(GapCandidateInput {
            open: kline.open,
            close: kline.close,
            is_atr_gap: true,
            body_ratio,
            body_ratio_threshold: super::GAP_CANDIDATE_BODY_RATIO_THRESHOLD,
        })
        .unwrap();
        assert_eq!(
            default_rows[position].gap_candidate_qualifies, expected_default.qualifies,
            "default transition must match pure facts with the app default at position {position}"
        );
        assert_eq!(
            default_rows[position].gap_candidate_qualifies,
            body_ratio >= super::GAP_CANDIDATE_BODY_RATIO_THRESHOLD,
            "default qualification must flip on the 0.618 boundary"
        );
    }

    #[test]
    fn crypto_source_frame_encodes_locked_candidate_columns_with_preserved_raw_order() {
        let bullish = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: 0.8,
            body_ratio_threshold: super::GAP_CANDIDATE_BODY_RATIO_THRESHOLD,
        })
        .unwrap();
        let bearish = gap_candidate_facts(GapCandidateInput {
            open: 105.0,
            close: 100.0,
            is_atr_gap: true,
            body_ratio: 0.9,
            body_ratio_threshold: super::GAP_CANDIDATE_BODY_RATIO_THRESHOLD,
        })
        .unwrap();
        let flat = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 100.0,
            is_atr_gap: true,
            body_ratio: 0.7,
            body_ratio_threshold: super::GAP_CANDIDATE_BODY_RATIO_THRESHOLD,
        })
        .unwrap();
        let non_candidate = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 100.5,
            is_atr_gap: false,
            body_ratio: 0.2,
            body_ratio_threshold: super::GAP_CANDIDATE_BODY_RATIO_THRESHOLD,
        })
        .unwrap();
        assert_eq!(bullish.direction, Some(GapCandidateDirection::Bullish));
        assert_eq!(bearish.direction, Some(GapCandidateDirection::Bearish));
        assert_eq!(flat.direction, Some(GapCandidateDirection::Flat));
        assert!(!non_candidate.qualifies);
        for facts in [bullish, bearish, flat] {
            assert!(facts.qualifies);
            let bottom = facts.body_bottom.unwrap();
            let top = facts.body_top.unwrap();
            assert!(
                bottom <= top,
                "qualifying body bounds must be ordered: {bottom} <= {top}"
            );
        }
        assert_eq!(non_candidate.body_bottom, None);
        assert_eq!(non_candidate.body_top, None);
        assert_eq!(non_candidate.direction, None);

        let klines = vec![
            Kline {
                open: 100.0,
                high: 106.0,
                low: 99.0,
                close: 105.0,
                volume: 1_000.0,
                time: 1_700_000_000_000,
                adjclose: Some(104.5),
            },
            Kline {
                open: 105.0,
                high: 106.0,
                low: 99.0,
                close: 100.0,
                volume: 2_000.0,
                time: 1_700_000_060_000,
                adjclose: None,
            },
            Kline {
                open: 100.0,
                high: 101.0,
                low: 99.0,
                close: 100.0,
                volume: 3_000.0,
                time: 1_700_000_120_000,
                adjclose: Some(100.0),
            },
            Kline {
                open: 100.0,
                high: 101.0,
                low: 99.0,
                close: 100.5,
                volume: 4_000.0,
                time: 1_700_000_180_000,
                adjclose: None,
            },
        ];
        let facts = [bullish, bearish, flat, non_candidate];
        let rows = facts
            .iter()
            .enumerate()
            .map(|(index, candidate)| CryptoIndicatorRow {
                atr: Some(1.0 + index as f64),
                volume_sma: None,
                ema200: None,
                bias_reversion: None,
                neutral_revrsi: None,
                bullish_revrsi: None,
                bearish_revrsi: None,
                atr_upperband: None,
                atr_lowerband: None,
                iching_original_energy: None,
                iching_transformed_energy: None,
                iching_nuclear_energy: None,
                structure_power: None,
                structure_power_sma: None,
                atr_percent: None,
                atr_reversion_percent: None,
                band_reversion: None,
                body_ratio: Some(0.5 + index as f64),
                is_atr_gap: Some(index != 3),
                gap_candidate_qualifies: candidate.qualifies,
                gap_candidate_body_bottom: candidate.body_bottom,
                gap_candidate_body_top: candidate.body_top,
                gap_candidate_direction: candidate.direction,
            })
            .collect::<Vec<_>>();

        let frame = crypto_output_frame(rows, &klines).unwrap();
        let expected_names = [
            "open",
            "high",
            "low",
            "close",
            "volume",
            "time",
            "adj_close",
            "atr",
            "volume_sma",
            "ema200",
            "bias_reversion",
            "neutral_revrsi",
            "bullish_revrsi",
            "bearish_revrsi",
            "atr_upperband",
            "atr_lowerband",
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_nuclear_energy",
            "iching_open",
            "iching_high",
            "iching_low",
            "iching_close",
            "iching_moving_line",
            "iching_transformed_close",
            "iching_nuclear_close",
            "structure_power",
            "structure_power_sma",
            "atr_percent",
            "atr_reversion_percent",
            "band_reversion",
            "body_ratio",
            "is_atr_gap",
            "gap_candidate_qualifies",
            "gap_candidate_body_bottom",
            "gap_candidate_body_top",
            "gap_candidate_direction",
        ];
        assert_eq!(frame.column_names(), expected_names);
        assert_eq!(frame.len(), klines.len());

        for (position, kline) in klines.iter().enumerate() {
            assert_eq!(
                frame.f64_at("open", position).unwrap(),
                Some(kline.open),
                "open at {position}"
            );
            assert_eq!(
                frame.f64_at("high", position).unwrap(),
                Some(kline.high),
                "high at {position}"
            );
            assert_eq!(
                frame.f64_at("low", position).unwrap(),
                Some(kline.low),
                "low at {position}"
            );
            assert_eq!(
                frame.f64_at("close", position).unwrap(),
                Some(kline.close),
                "close at {position}"
            );
            assert_eq!(
                frame.f64_at("volume", position).unwrap(),
                Some(kline.volume),
                "volume at {position}"
            );
            assert_eq!(
                frame.f64_at("time", position).unwrap(),
                Some(kline.time as f64),
                "time at {position}"
            );
            assert_eq!(
                frame.f64_at("adj_close", position).unwrap(),
                kline.adjclose,
                "adj_close at {position}"
            );
        }

        match frame.column("gap_candidate_qualifies") {
            Some(SourceColumnData::Boolean(values)) => {
                assert_eq!(
                    *values,
                    vec![Some(true), Some(true), Some(true), Some(false)]
                );
            }
            other => panic!("gap_candidate_qualifies must be Boolean, got {other:?}"),
        }
        match frame.column("gap_candidate_body_bottom") {
            Some(SourceColumnData::Number(values)) => {
                assert_eq!(*values, vec![Some(100.0), Some(100.0), Some(100.0), None]);
            }
            other => panic!("gap_candidate_body_bottom must be Number, got {other:?}"),
        }
        match frame.column("gap_candidate_body_top") {
            Some(SourceColumnData::Number(values)) => {
                assert_eq!(*values, vec![Some(105.0), Some(105.0), Some(100.0), None]);
            }
            other => panic!("gap_candidate_body_top must be Number, got {other:?}"),
        }
        match frame.column("gap_candidate_direction") {
            Some(SourceColumnData::Text(values)) => {
                assert_eq!(
                    *values,
                    vec![
                        Some("bullish".to_string()),
                        Some("bearish".to_string()),
                        Some("flat".to_string()),
                        None,
                    ]
                );
            }
            other => panic!("gap_candidate_direction must be Text, got {other:?}"),
        }

        let one_kline = klines[..1].to_vec();
        let one_row = vec![CryptoIndicatorRow {
            atr: Some(1.0),
            volume_sma: None,
            ema200: None,
            bias_reversion: None,
            neutral_revrsi: None,
            bullish_revrsi: None,
            bearish_revrsi: None,
            atr_upperband: None,
            atr_lowerband: None,
            iching_original_energy: None,
            iching_transformed_energy: None,
            iching_nuclear_energy: None,
            structure_power: None,
            structure_power_sma: None,
            atr_percent: None,
            atr_reversion_percent: None,
            band_reversion: None,
            body_ratio: Some(0.8),
            is_atr_gap: Some(true),
            gap_candidate_qualifies: bullish.qualifies,
            gap_candidate_body_bottom: bullish.body_bottom,
            gap_candidate_body_top: bullish.body_top,
            gap_candidate_direction: bullish.direction,
        }];
        let one_frame = crypto_output_frame(one_row, &one_kline).unwrap();
        assert_eq!(one_frame.len(), 1);
        assert_eq!(one_frame.column_names(), expected_names);
        assert_eq!(
            one_frame
                .string_at("gap_candidate_direction", 0)
                .unwrap()
                .as_deref(),
            Some("bullish")
        );
    }

    #[test]
    fn iching_replaces_rssi_sharpe_in_source_and_projection() {
        let frame = crypto_output_frame(vec![], &[]).unwrap();
        for present in [
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_nuclear_energy",
            "iching_open",
            "iching_high",
            "iching_low",
            "iching_close",
            "iching_moving_line",
            "iching_transformed_close",
            "iching_nuclear_close",
        ] {
            assert!(
                frame.column_names().contains(&present),
                "{present} must be present in source frame"
            );
        }
        for removed in [
            "rssi",
            "rssi_ma",
            "rssi_direction",
            "rssi_color",
            "sharpe",
            "sharpe_color",
            "trust",
        ] {
            assert!(
                !frame.column_names().contains(&removed),
                "{removed} must be absent from source frame"
            );
            assert!(
                frame.column(removed).is_none(),
                "{removed} column must be absent"
            );
        }
        let sql = build_crypto_sql(0.02, 0.01);
        for present in [
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_nuclear_energy",
        ] {
            assert!(sql.contains(present), "{present} must appear in SQL");
        }
        for removed in [
            "rssi",
            "rssi_ma",
            "rssi_direction",
            "rssi_color",
            "climax_signal",
            "sharpe",
            "sharpe_color",
            "trust",
        ] {
            assert!(!sql.contains(removed), "{removed} must be absent from SQL");
        }
    }

    #[tokio::test]
    async fn iching_values_equal_direct_allow_facade_within_energy_bounds() {
        let candles = klines();
        let collected = collect_crypto_rows(&candles).await.unwrap();
        assert_eq!(collected.len(), candles.len());
        for (position, (kline, row)) in candles.iter().zip(&collected).enumerate() {
            let datetime = chrono::DateTime::from_timestamp_millis(kline.time)
                .unwrap_or_else(|| panic!("fixture time at {position} must convert"));
            let expected = algotrap::ta::plum_blossom_signal_with_policy(
                datetime,
                algotrap::ta::LeapMonthPolicy::Allow,
            )
            .unwrap_or_else(|_| panic!("fixture time at {position} must succeed under Allow"));
            let transformed = expected
                .transformed
                .unwrap_or_else(|| panic!("Plum Blossom must supply transformed at {position}"));
            for (label, value) in [
                ("iching_original_energy", row.iching_original_energy),
                ("iching_transformed_energy", row.iching_transformed_energy),
                ("iching_nuclear_energy", row.iching_nuclear_energy),
            ] {
                let value = value.unwrap_or_else(|| panic!("row {position} {label} must be Some"));
                assert!(
                    value.is_finite(),
                    "row {position} {label} must be finite, got {value}"
                );
                assert!(
                    (-31.5..=31.5).contains(&value),
                    "row {position} {label} must be within [-31.5, 31.5], got {value}"
                );
            }
            assert!(
                (row.iching_original_energy.unwrap() - expected.original.energy).abs() <= 1e-12,
                "row {position} original mismatch"
            );
            assert!(
                (row.iching_transformed_energy.unwrap() - transformed.energy).abs() <= 1e-12,
                "row {position} transformed mismatch"
            );
            assert!(
                (row.iching_nuclear_energy.unwrap() - expected.nuclear.energy).abs() <= 1e-12,
                "row {position} nuclear mismatch"
            );
        }
        let (frame, _zones) = compute_crypto_frame(candles.clone(), ticker())
            .await
            .unwrap();
        assert_eq!(frame.len(), candles.len());
        for present in [
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_nuclear_energy",
        ] {
            assert!(frame.has_column(present), "{present} must be projected");
        }
        for removed in [
            "rssi",
            "rssi_ma",
            "rssi_direction",
            "rssi_color",
            "climax_signal",
            "climax_signal_pos",
            "climax_signal_color",
            "climax_signal_shape",
            "sharpe",
            "sharpe_color",
            "trust",
        ] {
            assert!(!frame.has_column(removed), "{removed} must be absent");
        }
    }

    #[tokio::test]
    async fn invalid_timestamp_fails_presentation_transformation() {
        assert!(
            chrono::DateTime::from_timestamp_millis(i64::MAX).is_none(),
            "i64::MAX must be an out-of-range timestamp"
        );
        let mut klines = klines();
        klines.truncate(2);
        klines[1].time = i64::MAX;
        let mut processor = algotrap::ta::prelude::Processor::new(CryptoIndicators::new());
        processor.process(&klines[0]).unwrap();
        assert!(
            processor.process(&klines[1]).is_err(),
            "out-of-range timestamp must fail transformation"
        );
        assert!(
            collect_crypto_rows(&klines).await.is_err(),
            "collection must fail on invalid timestamp rather than skip or null"
        );
        assert!(
            compute_crypto_frame(klines, ticker()).await.is_err(),
            "compute must fail without a frame on invalid timestamp"
        );
    }

    #[tokio::test]
    async fn facade_failure_propagates_without_serialized_substitution() {
        use chrono::{TimeZone, Utc};
        let mut facade_failure: Option<(chrono::DateTime<Utc>, algotrap::ta::TaError)> = None;
        for year in [0, 10_000, 20_000] {
            let Some(datetime) = Utc.with_ymd_and_hms(year, 1, 1, 0, 0, 0).single() else {
                continue;
            };
            if chrono::DateTime::from_timestamp_millis(datetime.timestamp_millis()).is_none() {
                continue;
            }
            if let Err(error) = algotrap::ta::plum_blossom_signal_with_policy(
                datetime,
                algotrap::ta::LeapMonthPolicy::Allow,
            ) {
                facade_failure = Some((datetime, error));
                break;
            }
        }
        let (datetime, expected) =
            facade_failure.expect("a valid-chrono but facade-failing date must exist");
        let failing_kline = Kline {
            open: 100.0,
            high: 102.0,
            low: 99.0,
            close: 100.5,
            volume: 1_000.0,
            time: datetime.timestamp_millis(),
            adjclose: None,
        };
        let mut processor = algotrap::ta::prelude::Processor::new(CryptoIndicators::new());
        let error = processor
            .process(&failing_kline)
            .expect_err("facade failure must propagate, not serialize");
        assert_eq!(error.kind, expected.kind);
        assert_eq!(error.message, expected.message);
        let collection_error = collect_crypto_rows(std::slice::from_ref(&failing_kline))
            .await
            .expect_err("collection must propagate facade failure");
        assert_eq!(
            collection_error.message, expected.message,
            "collection must preserve facade message rather than substitute"
        );
        assert!(
            compute_crypto_frame(vec![failing_kline], ticker())
                .await
                .is_err(),
            "compute must fail without a frame on facade failure"
        );
    }

    #[test]
    fn atr_reversion_is_bias_reversion_band_distance_normalized_by_atr_band() {
        assert_eq!(super::ATR_PERIOD, 42);
        assert!(
            (super::ATR_BAND_MULTIPLIER - 1.618).abs() <= f64::EPSILON,
            "ATR band multiplier must remain 1.618"
        );
        let mut candles = Vec::with_capacity(82);
        let mut time = 1_700_000_000_000_i64;
        for _ in 0..60 {
            candles.push(Kline {
                open: 100.0,
                high: 100.5,
                low: 99.5,
                close: 100.1,
                volume: 1_000.0,
                time,
                adjclose: None,
            });
            time += 60_000;
        }
        // Upward open jump: close stays near open (ATR gap false) while the
        // smoothed bias lags far below the lower band (large negative).
        candles.push(Kline {
            open: 300.0,
            high: 301.0,
            low: 299.0,
            close: 300.1,
            volume: 1_000.0,
            time,
            adjclose: None,
        });
        time += 60_000;
        for _ in 0..20 {
            candles.push(Kline {
                open: 300.0,
                high: 300.5,
                low: 299.5,
                close: 300.1,
                volume: 1_000.0,
                time,
                adjclose: None,
            });
            time += 60_000;
        }
        // Downward open jump: close stays near open (ATR gap false) while the
        // smoothed bias lags far above the upper band (large positive).
        candles.push(Kline {
            open: 100.0,
            high: 100.5,
            low: 99.5,
            close: 100.1,
            volume: 1_000.0,
            time,
            adjclose: None,
        });

        let rows = direct_aggregate_rows(&candles);
        assert_eq!(rows.len(), candles.len());

        // Zero-oscillation branch of the percent kernel (ATR == 0) must be 0.
        {
            use algotrap::ta::prelude::{BandPoint, Kernel, PriorState};
            let zero_point = BandPoint {
                open: 100.0,
                atr: 0.0,
                signal: 100.0,
            };
            let reversion = algotrap::ta::prelude::band_reversion(super::ATR_BAND_MULTIPLIER)
                .transition(PriorState::Initial, &zero_point)
                .unwrap()
                .output
                .unwrap();
            assert_eq!(reversion, 0.0, "zero-width band must revert 0");
            let percent = algotrap::ta::prelude::band_reversion_percent(super::ATR_BAND_MULTIPLIER)
                .transition(PriorState::Initial, &zero_point)
                .unwrap()
                .output
                .unwrap();
            assert_eq!(percent, 0.0, "zero oscillation percent must be 0");
        }

        let mut saw_in_band = false;
        let mut saw_above = false;
        let mut saw_below = false;
        let mut saw_bias_not_close = false;
        let mut saw_gap_diverges_from_reversion = false;

        for (position, (kline, row)) in candles.iter().zip(&rows).enumerate() {
            let atr = row
                .atr
                .unwrap_or_else(|| panic!("row {position} ATR must be available"));
            assert!(
                atr.is_finite() && atr != 0.0,
                "row {position} ATR must be nonzero finite, got {atr}"
            );
            let bias = row
                .bias_reversion
                .unwrap_or_else(|| panic!("row {position} bias_reversion must be available"));
            assert!(
                bias.is_finite(),
                "row {position} bias must be finite, got {bias}"
            );
            let band = row
                .band_reversion
                .unwrap_or_else(|| panic!("row {position} band_reversion must be available"));
            let percent = row.atr_reversion_percent.unwrap_or_else(|| {
                panic!("row {position} atr_reversion_percent must be available")
            });
            assert!(
                band.is_finite(),
                "row {position} band must be finite, got {band}"
            );
            assert!(
                percent.is_finite(),
                "row {position} percent must be finite, got {percent}"
            );

            // Locked metric: signal is bias_reversion, not close.
            let oscillation = atr * super::ATR_BAND_MULTIPLIER;
            assert!(
                oscillation.is_finite() && oscillation != 0.0,
                "row {position} oscillation must be nonzero finite, got {oscillation}"
            );
            let upper = kline.open + oscillation;
            let lower = kline.open - oscillation;
            let expected_band = if lower <= bias && bias <= upper {
                0.0
            } else if bias > upper {
                bias - upper
            } else {
                bias - lower
            };
            let expected_percent = if oscillation == 0.0 {
                0.0
            } else {
                100.0 * expected_band / oscillation
            };
            assert!(
                (band - expected_band).abs() <= 1e-12,
                "row {position} band {band} must equal bias-based distance {expected_band} (open {}, atr {atr}, bias {bias})",
                kline.open,
            );
            assert!(
                (percent - expected_percent).abs() <= 1e-12,
                "row {position} percent {percent} must equal 100*band/osc {expected_percent}"
            );

            // Field values must equal existing TA kernel outputs with same BandPoint.
            {
                use algotrap::ta::prelude::{BandPoint, Kernel, PriorState};
                let point = BandPoint {
                    open: kline.open,
                    atr,
                    signal: bias,
                };
                let kernel_band = algotrap::ta::prelude::band_reversion(super::ATR_BAND_MULTIPLIER)
                    .transition(PriorState::Initial, &point)
                    .unwrap()
                    .output
                    .unwrap();
                let kernel_percent =
                    algotrap::ta::prelude::band_reversion_percent(super::ATR_BAND_MULTIPLIER)
                        .transition(PriorState::Initial, &point)
                        .unwrap()
                        .output
                        .unwrap();
                assert!(
                    (band - kernel_band).abs() <= 1e-12,
                    "row {position} band {band} must equal TA kernel {kernel_band}"
                );
                assert!(
                    (percent - kernel_percent).abs() <= 1e-12,
                    "row {position} percent {percent} must equal TA kernel {kernel_percent}"
                );
            }

            // ATR gap must still use close against open±ATR (not bias), and must
            // not conflate with ATR reversion.
            let expected_gap = kline.close > kline.open + atr * super::ATR_GAP_MULTIPLIER
                || kline.close < kline.open - atr * super::ATR_GAP_MULTIPLIER;
            assert_eq!(
                row.is_atr_gap,
                Some(expected_gap),
                "row {position} is_atr_gap must use close against open±ATR"
            );

            if expected_band == 0.0 {
                assert_eq!(band, 0.0, "row {position} in-band must be numeric 0");
                assert_eq!(
                    percent, 0.0,
                    "row {position} in-band percent must be numeric 0"
                );
                saw_in_band = true;
            } else if expected_band > 0.0 {
                assert!(
                    band > 0.0 && percent > 0.0,
                    "row {position} above upper must be positive"
                );
                assert!(
                    (band - (bias - upper)).abs() <= 1e-12,
                    "row {position} above must equal signal-upper"
                );
                saw_above = true;
            } else {
                assert!(
                    band < 0.0 && percent < 0.0,
                    "row {position} below lower must be negative"
                );
                assert!(
                    (band - (bias - lower)).abs() <= 1e-12,
                    "row {position} below must equal signal-lower"
                );
                saw_below = true;
            }

            // Prove signal is bias, not close: close-based continuous metric
            // must visibly differ on lag rows, while bias-based matches.
            let close_based = kline.close - kline.open;
            if (close_based - band).abs() > 1.0 {
                saw_bias_not_close = true;
            }
            if expected_gap != (band != 0.0) {
                saw_gap_diverges_from_reversion = true;
            }
        }

        assert!(
            saw_in_band,
            "fixture must include at least one in-band (0) row"
        );
        assert!(
            saw_above,
            "fixture must include at least one above-upper (positive) row"
        );
        assert!(
            saw_below,
            "fixture must include at least one below-lower (negative) row"
        );
        assert!(
            saw_bias_not_close,
            "fixture must prove signal is bias_reversion, not close (close-open differs from band)"
        );
        assert!(
            saw_gap_diverges_from_reversion,
            "ATR gap (close-based) must diverge from ATR reversion (bias-based) on at least one row"
        );
    }

    #[tokio::test]
    async fn source_and_projected_schemas_omit_rssi_and_trust() {
        use algotrap::query::gap_zones::recent_gap_zones;

        let candles = klines();
        let rows = collect_crypto_rows(&candles).await.unwrap();
        let source = crypto_output_frame(rows, &candles).unwrap();
        for removed in ["rssi", "rssi_ma", "rssi_direction", "rssi_color", "trust"] {
            assert!(
                !source.column_names().contains(&removed),
                "{removed} must be absent from source frame"
            );
            assert!(
                source.column(removed).is_none(),
                "{removed} column must be absent from source"
            );
        }
        let (projected, zones) = compute_crypto_frame(candles.clone(), ticker())
            .await
            .unwrap();
        for removed in ["rssi", "rssi_ma", "rssi_direction", "rssi_color", "trust"] {
            assert!(
                !projected.has_column(removed),
                "{removed} must be absent from projected frame"
            );
        }
        let expected_zones = recent_gap_zones(source, candles.last().unwrap().time, 64)
            .unwrap()
            .zones;
        assert_eq!(
            zones, expected_zones,
            "gap zones must remain raw and ordered"
        );
        let sql = build_crypto_sql(0.02, 0.01);
        for removed in ["rssi", "rssi_ma", "trust"] {
            assert!(!sql.contains(removed), "{removed} must be absent from SQL");
        }
    }

    fn klines() -> Vec<Kline> {
        (0..300)
            .map(|index| {
                let open = 100.0 + index as f64;
                Kline {
                    open,
                    high: open + 2.0,
                    low: open - 1.0,
                    close: open + 0.5,
                    volume: 1_000.0,
                    time: 1_700_000_000_000 + index * 60_000,
                    adjclose: None,
                }
            })
            .collect()
    }
}

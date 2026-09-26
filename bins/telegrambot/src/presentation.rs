//! Telegram-owned indicator configuration, projection, and SQL presentation contract.

use algotrap::adapter::{Stamped, StreamPipelineError, kernel_stream_pipeline};
use algotrap::engine::error::MarketError;
use algotrap::engine::frame::{SourceColumnData, SourceFrame};
use algotrap::engine::traits::ComputedFrame;
use algotrap::engine::validation::ValidatedTicker;
use algotrap::prelude::Kline;
use algotrap::query::RawQuery;
use algotrap::query::duckdb::DuckDBQuery;
use algotrap::query::gap_zones::{GapZoneRecord, recent_gap_zones};
use algotrap::ta::ops::{GapCandidateDirection, GapCandidateInput, gap_candidate_facts};
use algotrap::ta::prelude::{
    Atr, AtrState, BandPoint, BandReversion, BandReversionPercent, BandReversionPercentState,
    BandReversionState, BarBias, BarBiasState, BiasReversion, BiasReversionInput,
    BiasReversionState, BodyRatio, BodyRatioState, Ema, EmaState, IsAtrGap, IsAtrGapState, Kernel,
    KernelStep, PriorState, ReverseRsi, ReverseRsiState, Rma, RmaState, Rsi, RsiState, Sharpe,
    SharpeState, Sma, SmaState, TaError, TaResult, atr, atr_percent, band_reversion,
    band_reversion_percent, bar_bias, bias_reversion, body_ratio, ema, iching_bar_trajectory,
    is_atr_gap, option_map2, require_output, reverse_rsi, rma, rsi, sharpe, sma,
};
use futures::TryStreamExt;
use std::convert::Infallible;

const BASE_COLUMNS: [&str; 8] = [
    "open",
    "high",
    "low",
    "close",
    "volume",
    "time",
    "adj_close",
    "Date",
];

const ICHING_ENERGY_COLUMNS: [&str; 13] = [
    "iching_original_energy",
    "iching_transformed_energy",
    "iching_mutual_energy",
    "iching_open",
    "iching_high",
    "iching_low",
    "iching_close",
    "iching_moving_line",
    "iching_transformed_close",
    "iching_mutual_close",
    "iching_mutual_high",
    "iching_mutual_low",
    "iching_mutual_mean",
];

pub(crate) struct TelegramIndicators {
    bar_bias: BarBias,
    atr: Atr,
    volume_ema: Ema,
    ema200: Ema,
    bias_reversion: BiasReversion,
    neutral_revrsi: ReverseRsi,
    bullish_revrsi: ReverseRsi,
    bearish_revrsi: ReverseRsi,
    rsi: Rsi,
    rsi_ma: Ema,
    structure_power: Rma,
    structure_power_sma: Sma,
    sharpe: Sharpe,
    body_ratio: BodyRatio,
    band_reversion: BandReversion,
    band_reversion_percent: BandReversionPercent,
    is_atr_gap: IsAtrGap,
    atr_multiplier: f64,
    body_ratio_threshold: f64,
}

pub(crate) struct TelegramIndicatorState {
    bar_bias: BarBiasState,
    atr: AtrState,
    volume_ema: EmaState,
    ema200: EmaState,
    bias_reversion: BiasReversionState,
    neutral_revrsi: ReverseRsiState,
    bullish_revrsi: ReverseRsiState,
    bearish_revrsi: ReverseRsiState,
    rsi: RsiState,
    rsi_ma: EmaState,
    structure_power: RmaState,
    structure_power_sma: SmaState,
    sharpe: SharpeState,
    body_ratio: BodyRatioState,
    band_reversion: BandReversionState,
    band_reversion_percent: BandReversionPercentState,
    is_atr_gap: IsAtrGapState,
}

#[derive(Debug, PartialEq)]
pub(crate) struct TelegramIndicatorRow {
    pub atr: Option<f64>,
    pub volume_sma: Option<f64>,
    pub ema200: Option<f64>,
    pub bias_reversion: Option<f64>,
    pub neutral_revrsi: Option<f64>,
    pub bullish_revrsi: Option<f64>,
    pub bearish_revrsi: Option<f64>,
    pub atr_upperband: Option<f64>,
    pub atr_lowerband: Option<f64>,
    pub rssi: Option<f64>,
    pub rssi_ma: Option<f64>,
    pub structure_power: Option<f64>,
    pub structure_power_sma: Option<f64>,
    pub atr_percent: Option<f64>,
    pub atr_reversion_percent: Option<f64>,
    pub band_reversion: Option<f64>,
    pub sharpe: Option<f64>,
    pub body_ratio: Option<f64>,
    pub is_atr_gap: Option<bool>,
    pub gap_candidate_qualifies: bool,
    pub gap_candidate_body_bottom: Option<f64>,
    pub gap_candidate_body_top: Option<f64>,
    pub gap_candidate_direction: Option<GapCandidateDirection>,
}

impl TelegramIndicators {
    fn new(
        periods: IndicatorPeriods,
        body_ratio_threshold: f64,
        atr_band_multiplier: f64,
        atr_gap_multiplier: f64,
    ) -> Self {
        Self {
            bar_bias: bar_bias(),
            atr: atr(periods.atr),
            volume_ema: ema(periods.volume_ema),
            ema200: ema(periods.ema),
            bias_reversion: bias_reversion(periods.bias),
            neutral_revrsi: reverse_rsi(periods.reverse_rsi, 50.0),
            bullish_revrsi: reverse_rsi(periods.reverse_rsi, 70.0),
            bearish_revrsi: reverse_rsi(periods.reverse_rsi, 30.0),
            rsi: rsi(periods.rsi),
            rsi_ma: ema(periods.rsi_smooth),
            structure_power: rma(periods.structure),
            structure_power_sma: sma(periods.structure_sma),
            sharpe: sharpe(periods.sharpe),
            body_ratio: body_ratio(),
            band_reversion: band_reversion(atr_band_multiplier),
            band_reversion_percent: band_reversion_percent(atr_band_multiplier),
            is_atr_gap: is_atr_gap(atr_gap_multiplier),
            atr_multiplier: atr_band_multiplier,
            body_ratio_threshold,
        }
    }
}

impl Kernel for TelegramIndicators {
    type Input = Kline;
    type Output = TelegramIndicatorRow;
    type State = TelegramIndicatorState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        kline: &Kline,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
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
        let bias_reversion_value = require_output("bias reversion", bias_reversion_step.output)?;
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
        let rsi_step = self.rsi.transition(
            child_prior(&prior, |state| &state.rsi),
            &(kline.open + bias),
        )?;
        let rsi_value = require_output("RSI", rsi_step.output)?;
        let rsi_ma_step = self
            .rsi_ma
            .transition(child_prior(&prior, |state| &state.rsi_ma), &rsi_value)?;
        let structure_step = self
            .structure_power
            .transition(child_prior(&prior, |state| &state.structure_power), &bias)?;
        let structure_value = require_output("structure power", structure_step.output)?;
        let structure_sma_step = self.structure_power_sma.transition(
            child_prior(&prior, |state| &state.structure_power_sma),
            &structure_value,
        )?;
        let sharpe_step = self
            .sharpe
            .transition(child_prior(&prior, |state| &state.sharpe), &kline.close)?;
        let body_step = self
            .body_ratio
            .transition(child_prior(&prior, |state| &state.body_ratio), kline)?;
        let band_input = BandPoint {
            open: kline.open,
            atr,
            signal: bias_reversion_value,
        };
        let band_step = self.band_reversion.transition(
            child_prior(&prior, |state| &state.band_reversion),
            &band_input,
        )?;
        let band_percent_step = self.band_reversion_percent.transition(
            child_prior(&prior, |state| &state.band_reversion_percent),
            &band_input,
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
        let output = TelegramIndicatorRow {
            atr: Some(atr),
            volume_sma: volume_step.output,
            ema200: ema_step.output,
            bias_reversion: Some(bias_reversion_value),
            neutral_revrsi: neutral_revrsi_step.output,
            bullish_revrsi: bullish_revrsi_step.output,
            bearish_revrsi: bearish_revrsi_step.output,
            atr_upperband: option_map2(Some(kline.open), Some(atr), |open, atr| {
                open + atr * self.atr_multiplier
            })?,
            atr_lowerband: option_map2(Some(kline.open), Some(atr), |open, atr| {
                open - atr * self.atr_multiplier
            })?,
            rssi: Some(rsi_value),
            rssi_ma: rsi_ma_step.output,
            structure_power: Some(structure_value),
            structure_power_sma: structure_sma_step.output,
            atr_percent: option_map2(Some(atr), Some(kline.open), atr_percent)?,
            atr_reversion_percent: band_percent_step.output,
            band_reversion: band_step.output,
            sharpe: sharpe_step.output,
            body_ratio: Some(body_ratio),
            is_atr_gap: Some(is_atr_gap),
            gap_candidate_qualifies: candidate.qualifies,
            gap_candidate_body_bottom: candidate.body_bottom,
            gap_candidate_body_top: candidate.body_top,
            gap_candidate_direction: candidate.direction,
        };
        let next_state = TelegramIndicatorState {
            bar_bias: bias_step.next_state,
            atr: atr_step.next_state,
            volume_ema: volume_step.next_state,
            ema200: ema_step.next_state,
            bias_reversion: bias_reversion_step.next_state,
            neutral_revrsi: neutral_revrsi_step.next_state,
            bullish_revrsi: bullish_revrsi_step.next_state,
            bearish_revrsi: bearish_revrsi_step.next_state,
            rsi: rsi_step.next_state,
            rsi_ma: rsi_ma_step.next_state,
            structure_power: structure_step.next_state,
            structure_power_sma: structure_sma_step.next_state,
            sharpe: sharpe_step.next_state,
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

async fn collect_telegram_rows(
    klines: &[Kline],
    periods: IndicatorPeriods,
    body_ratio_threshold: f64,
    atr_band_multiplier: f64,
    atr_gap_multiplier: f64,
) -> Result<Vec<TelegramIndicatorRow>, MarketError> {
    let source =
        futures::stream::iter(klines.iter().cloned().enumerate().map(|(id, value)| {
            Ok::<_, StreamPipelineError<Infallible, usize>>(Stamped { id, value })
        }));
    let stamped_rows: Vec<Stamped<usize, TelegramIndicatorRow>> = kernel_stream_pipeline(
        source,
        TelegramIndicators::new(
            periods,
            body_ratio_threshold,
            atr_band_multiplier,
            atr_gap_multiplier,
        ),
    )
    .try_collect()
    .await
    .map_err(map_telegram_stream_error)?;

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

fn map_telegram_stream_error(error: StreamPipelineError<Infallible, usize>) -> MarketError {
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

fn telegram_output_frame(
    rows: Vec<TelegramIndicatorRow>,
    klines: &[Kline],
    outputs: &crate::memory::TelegramOutputConfig,
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

    let mut columns = vec![
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
    ];

    columns.extend([
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
            "iching_mutual_energy".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.mutual_open))
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
            "iching_mutual_close".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.mutual_close))
                    .collect(),
            ),
        ),
        (
            "iching_mutual_high".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.mutual_high))
                    .collect(),
            ),
        ),
        (
            "iching_mutual_low".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.mutual_low))
                    .collect(),
            ),
        ),
        (
            "iching_mutual_mean".into(),
            SourceColumnData::Number(
                trajectories
                    .iter()
                    .map(|trajectory| Some(trajectory.mutual_mean))
                    .collect(),
            ),
        ),
    ]);

    let number = |select: fn(&TelegramIndicatorRow) -> Option<f64>| {
        SourceColumnData::Number(rows.iter().map(select).collect())
    };
    macro_rules! push_number {
        ($active:expr, $name:literal, $field:ident) => {
            if $active {
                columns.push(($name.into(), number(|row| row.$field)));
            }
        };
    }
    push_number!(outputs.atr.active, "atr", atr);
    push_number!(outputs.volume_sma.active, "volume_sma", volume_sma);
    push_number!(outputs.ema200.active, "ema200", ema200);
    push_number!(
        outputs.bias_reversion.active,
        "bias_reversion",
        bias_reversion
    );
    push_number!(
        outputs.neutral_revrsi.active,
        "neutral_revrsi",
        neutral_revrsi
    );
    push_number!(
        outputs.bullish_revrsi.active,
        "bullish_revrsi",
        bullish_revrsi
    );
    push_number!(
        outputs.bearish_revrsi.active,
        "bearish_revrsi",
        bearish_revrsi
    );
    push_number!(outputs.atr_upperband.active, "atr_upperband", atr_upperband);
    push_number!(outputs.atr_lowerband.active, "atr_lowerband", atr_lowerband);
    push_number!(outputs.rssi.active, "rssi", rssi);
    push_number!(outputs.rssi_ma.active, "rssi_ma", rssi_ma);
    push_number!(
        outputs.structure_power.active,
        "structure_power",
        structure_power
    );
    push_number!(
        outputs.structure_power_sma.active,
        "structure_power_sma",
        structure_power_sma
    );
    push_number!(outputs.atr_percent.active, "atr_percent", atr_percent);
    push_number!(
        outputs.atr_reversion_percent.active,
        "atr_reversion_percent",
        atr_reversion_percent
    );
    push_number!(
        outputs.band_reversion.active,
        "band_reversion",
        band_reversion
    );
    push_number!(outputs.sharpe.active, "sharpe", sharpe);
    push_number!(outputs.body_ratio.active, "body_ratio", body_ratio);
    if outputs.is_atr_gap.active {
        columns.push((
            "is_atr_gap".into(),
            SourceColumnData::Boolean(rows.iter().map(|row| row.is_atr_gap).collect()),
        ));
    }
    if outputs.leverage.active && !outputs.atr.active {
        columns.push((
            "__leverage_atr".into(),
            SourceColumnData::Number(rows.iter().map(|row| row.atr).collect()),
        ));
    }

    columns.push((
        "gap_candidate_qualifies".into(),
        SourceColumnData::Boolean(
            rows.iter()
                .map(|row| Some(row.gap_candidate_qualifies))
                .collect(),
        ),
    ));
    columns.push((
        "gap_candidate_body_bottom".into(),
        SourceColumnData::Number(
            rows.iter()
                .map(|row| row.gap_candidate_body_bottom)
                .collect(),
        ),
    ));
    columns.push((
        "gap_candidate_body_top".into(),
        SourceColumnData::Number(rows.iter().map(|row| row.gap_candidate_body_top).collect()),
    ));
    columns.push((
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
    ));

    SourceFrame::from_columns(columns)
}

/// Computes one Telegram frame using the bot's indicator and source-controlled
/// raw SQL contract, alongside the pre-budgeted recent gap zones for that
/// timeframe.
///
/// The projected chart frame contract is unchanged. Zones are carried in a
/// parallel vector (not embedded in the frame) because the
/// `HashMap<Timeframe, Box<dyn ComputedFrame>>` contract is consumed widely.
pub(crate) async fn compute_telegram_frame(
    klines: Vec<Kline>,
    ticker: ValidatedTicker,
    indicator_config: &crate::memory::IndicatorConfig,
) -> Result<(Box<dyn ComputedFrame>, Vec<GapZoneRecord>), MarketError> {
    let periods = indicator_periods(&indicator_config.periods)?;
    let body_ratio_threshold = indicator_config.gap_zones.body_ratio_threshold.clamped();
    let atr_band_multiplier = indicator_config.gap_zones.atr_band_multiplier.clamped();
    let atr_gap_multiplier = indicator_config.gap_zones.atr_gap_multiplier.clamped();
    let rows = collect_telegram_rows(
        &klines,
        periods,
        body_ratio_threshold,
        atr_band_multiplier,
        atr_gap_multiplier,
    )
    .await?;
    let frame = telegram_output_frame(rows, &klines, &indicator_config.outputs)?;
    let (sl_percent, tol_percent) = ticker.risk_percentages();

    // Decision time is the last supplied kline time; empty input has no
    // decision candle so use i64::MAX (strict `<` then admits nothing extra,
    // and the empty source yields zero zones anyway).
    let decision_time_ms = klines.last().map(|k| k.time).unwrap_or(i64::MAX);
    // Per-timeframe budget from persisted config; clamp hard to [1, 32] so a
    // zero config cannot break the chart path and the 32 hard max holds.
    let budget = (indicator_config.gap_zones.max_zones.clamped() as usize).clamp(1, 32);

    let projected = DuckDBQuery::new()
        .project(
            frame.clone(),
            RawQuery::source_controlled(build_telegram_sql(
                &indicator_config.outputs,
                sl_percent,
                tol_percent,
            )),
        )
        .map(|frame| Box::new(frame) as Box<dyn ComputedFrame>)?;
    let zones = recent_gap_zones(frame, decision_time_ms, budget)?.zones;
    Ok((projected, zones))
}

#[derive(Debug, Clone, Copy)]
struct IndicatorPeriods {
    // Fixed 20-period EMA retained for the source-controlled `volume_sma`
    // presentation contract.
    volume_ema: usize,
    ema: usize,
    rsi: usize,
    rsi_smooth: usize,
    reverse_rsi: usize,
    atr: usize,
    bias: usize,
    structure: usize,
    // Fixed 16-period smoothing retained for the source-controlled
    // `structure_power_sma` presentation contract.
    structure_sma: usize,
    sharpe: usize,
}

fn indicator_periods(
    periods: &crate::memory::IndicatorPeriods,
) -> Result<IndicatorPeriods, MarketError> {
    Ok(IndicatorPeriods {
        volume_ema: require_positive("volume_sma period", periods.volume_ema.clamped() as usize)?,
        ema: require_positive("ema200 period", periods.ema.clamped() as usize)?,
        rsi: require_positive("rssi period", periods.rsi.clamped() as usize)?,
        rsi_smooth: require_positive("rssi_ma smooth", periods.rsi_smooth.clamped() as usize)?,
        reverse_rsi: require_positive("revrsi period", periods.reverse_rsi.clamped() as usize)?,
        atr: require_positive("atr period", periods.atr.clamped() as usize)?,
        bias: require_positive("bias_reversion smooth", periods.bias.clamped() as usize)?,
        structure: require_positive(
            "structure_power period",
            periods.structure.clamped() as usize,
        )?,
        structure_sma: require_positive(
            "structure_power_sma period",
            periods.structure_sma.clamped() as usize,
        )?,
        sharpe: require_positive("sharpe period", periods.sharpe.clamped() as usize)?,
    })
}

fn build_telegram_sql(
    outputs: &crate::memory::TelegramOutputConfig,
    sl_percent: f64,
    tol_percent: f64,
) -> String {
    let select_clause = telegram_select_expressions(outputs, sl_percent, tol_percent).join(", ");
    format!(
        "WITH telegram_base AS (SELECT *, CAST(time AS VARCHAR) AS \"Date\" FROM computed()) SELECT {select_clause} FROM telegram_base ORDER BY time"
    )
}

fn telegram_select_expressions(
    outputs: &crate::memory::TelegramOutputConfig,
    sl_percent: f64,
    tol_percent: f64,
) -> Vec<String> {
    let risk_adjustment = sql_double(sl_percent / (1.0 + tol_percent));
    telegram_output_columns(outputs)
        .into_iter()
        .map(|column| match column.as_str() {
            "leverage" => {
                let atr_column = if outputs.atr.active {
                    "atr"
                } else {
                    "__leverage_atr"
                };
                format!(
                    "CASE WHEN {atr_column} - {atr_column} = 0 AND {atr_column} <> 0.0 THEN {risk_adjustment} * open / {atr_column} ELSE NULL END AS leverage"
                )
            }
            _ => quote_ident(&column),
        })
        .collect()
}

fn telegram_output_columns(outputs: &crate::memory::TelegramOutputConfig) -> Vec<String> {
    let mut columns = BASE_COLUMNS
        .iter()
        .map(|column| (*column).to_string())
        .collect::<Vec<_>>();

    columns.extend(
        ICHING_ENERGY_COLUMNS
            .iter()
            .map(|column| (*column).to_string()),
    );

    if outputs.atr.active {
        columns.push("atr".to_string());
    }
    if outputs.volume_sma.active {
        columns.push("volume_sma".to_string());
    }
    if outputs.ema200.active {
        columns.push("ema200".to_string());
    }
    if outputs.bias_reversion.active {
        columns.push("bias_reversion".to_string());
    }
    if outputs.neutral_revrsi.active {
        columns.push("neutral_revrsi".to_string());
    }
    if outputs.bullish_revrsi.active {
        columns.push("bullish_revrsi".to_string());
    }
    if outputs.bearish_revrsi.active {
        columns.push("bearish_revrsi".to_string());
    }
    if outputs.atr_upperband.active {
        columns.push("atr_upperband".to_string());
    }
    if outputs.atr_lowerband.active {
        columns.push("atr_lowerband".to_string());
    }
    if outputs.rssi.active {
        columns.push("rssi".to_string());
    }
    if outputs.rssi_ma.active {
        columns.push("rssi_ma".to_string());
    }
    if outputs.structure_power.active {
        columns.push("structure_power".to_string());
    }
    if outputs.structure_power_sma.active {
        columns.push("structure_power_sma".to_string());
    }
    if outputs.atr_percent.active {
        columns.push("atr_percent".to_string());
    }
    if outputs.atr_reversion_percent.active {
        columns.push("atr_reversion_percent".to_string());
    }
    if outputs.band_reversion.active {
        columns.push("band_reversion".to_string());
    }
    if outputs.sharpe.active {
        columns.push("sharpe".to_string());
    }
    if outputs.body_ratio.active {
        columns.push("body_ratio".to_string());
    }
    if outputs.is_atr_gap.active {
        columns.push("is_atr_gap".to_string());
    }
    if outputs.leverage.active {
        columns.push("leverage".to_string());
    }

    columns
}

fn require_positive(label: &str, value: usize) -> Result<usize, MarketError> {
    (value > 0)
        .then_some(value)
        .ok_or_else(|| MarketError::validation(format!("{label} must be greater than zero")))
}

fn quote_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
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
        BASE_COLUMNS, ICHING_ENERGY_COLUMNS, TelegramIndicatorRow, TelegramIndicatorState,
        TelegramIndicators, build_telegram_sql, child_prior, collect_telegram_rows,
        compute_telegram_frame, indicator_periods, telegram_output_columns, telegram_output_frame,
        telegram_select_expressions,
    };
    use crate::memory::{IndicatorConfig, ParamSpec};
    use algotrap::adapter::{Stamped, StreamPipelineError, kernel_stream_pipeline};
    use algotrap::engine::error::ErrorKind;
    use algotrap::engine::frame::{SourceColumnData, SourceFrame};
    use algotrap::engine::traits::ComputedFrame;
    use algotrap::engine::validation::ValidatedTicker;
    use algotrap::prelude::Kline;
    use algotrap::query::RawQuery;
    use algotrap::query::duckdb::DuckDBQuery;
    use algotrap::ta::ops::{GapCandidateInput, gap_candidate_facts};
    use algotrap::ta::prelude::{
        BandPoint, Kernel, PriorState, band_reversion, band_reversion_percent, is_atr_gap,
    };
    use futures::TryStreamExt;
    use std::collections::HashSet;
    use std::convert::Infallible;

    const DEFAULT_FRAME_OUTPUTS: &[&str] = &[
        "atr",
        "volume_sma",
        "ema200",
        "bias_reversion",
        "neutral_revrsi",
        "bullish_revrsi",
        "bearish_revrsi",
        "atr_upperband",
        "atr_lowerband",
        "rssi",
        "rssi_ma",
        "structure_power",
        "structure_power_sma",
        "atr_percent",
        "atr_reversion_percent",
        "band_reversion",
        "sharpe",
        "body_ratio",
        "is_atr_gap",
        "leverage",
    ];

    #[tokio::test]
    async fn all_default_output_schema_remains_canonical() {
        let config = IndicatorConfig::default();
        let frame = compute_default_frame(&config).await;

        assert_eq!(frame.columns(), expected_columns(DEFAULT_FRAME_OUTPUTS));
    }

    #[tokio::test]
    async fn deactivating_one_output_removes_only_its_final_frame_column() {
        let mut config = IndicatorConfig::default();
        config.outputs.ema200.active = false;

        let frame = compute_default_frame(&config).await;

        assert!(!frame.has_column("ema200"));
        assert!(frame.has_column("volume_sma"));
    }

    #[tokio::test]
    async fn active_rssi_ma_with_inactive_rssi_returns_only_rssi_ma() {
        let config = only_active_outputs(&["rssi_ma"]);
        let frame = compute_default_frame(&config).await;

        assert_eq!(frame.columns(), expected_columns(&["rssi_ma"]));
        assert!(frame.has_column("rssi_ma"));
        assert!(!frame.has_column("rssi"));
    }

    #[tokio::test]
    async fn active_leverage_with_inactive_atr_returns_leverage_but_not_private_atr() {
        let config = only_active_outputs(&["leverage"]);
        let frame = compute_default_frame(&config).await;

        assert_eq!(frame.columns(), expected_columns(&["leverage"]));
        assert!(frame.has_column("leverage"));
        assert!(!frame.has_column("atr"));
        assert!(!frame.has_column("__leverage_atr"));
    }

    #[tokio::test]
    async fn active_is_atr_gap_with_inactive_atr_returns_gap_flag_but_not_atr() {
        let config = only_active_outputs(&["is_atr_gap"]);
        let frame = compute_default_frame(&config).await;

        assert_eq!(frame.columns(), expected_columns(&["is_atr_gap"]));
        assert!(frame.has_column("is_atr_gap"));
        assert!(!frame.has_column("atr"));
    }

    #[tokio::test]
    async fn base_columns_are_always_present_even_with_no_active_outputs() {
        let config = only_active_outputs(&[]);
        let frame = compute_default_frame(&config).await;

        assert_eq!(frame.columns(), expected_columns(&[]));
        for column in BASE_COLUMNS {
            assert!(frame.has_column(column), "missing base column {column}");
        }
    }

    #[test]
    fn output_order_is_canonical_and_contains_no_duplicates() {
        let config = only_active_outputs(&["leverage", "atr_percent", "rssi_ma", "atr"]);
        let columns = telegram_output_columns(&config.outputs);
        let select_expressions = telegram_select_expressions(&config.outputs, 0.02, 0.01);

        assert_eq!(
            columns,
            expected_columns(&["atr", "rssi_ma", "atr_percent", "leverage"])
        );
        assert_eq!(columns.len(), columns.iter().collect::<HashSet<_>>().len());
        assert_eq!(
            select_expressions,
            vec![
                "\"open\"".to_string(),
                "\"high\"".to_string(),
                "\"low\"".to_string(),
                "\"close\"".to_string(),
                "\"volume\"".to_string(),
                "\"time\"".to_string(),
                "\"adj_close\"".to_string(),
                "\"Date\"".to_string(),
                "\"iching_original_energy\"".to_string(),
                "\"iching_transformed_energy\"".to_string(),
                "\"iching_mutual_energy\"".to_string(),
                "\"iching_open\"".to_string(),
                "\"iching_high\"".to_string(),
                "\"iching_low\"".to_string(),
                "\"iching_close\"".to_string(),
                "\"iching_moving_line\"".to_string(),
                "\"iching_transformed_close\"".to_string(),
                "\"iching_mutual_close\"".to_string(),
                "\"iching_mutual_high\"".to_string(),
                "\"iching_mutual_low\"".to_string(),
                "\"iching_mutual_mean\"".to_string(),
                "\"atr\"".to_string(),
                "\"rssi_ma\"".to_string(),
                "\"atr_percent\"".to_string(),
                "CASE WHEN atr - atr = 0 AND atr <> 0.0 THEN CAST(0.019801980198019802 AS DOUBLE) * open / atr ELSE NULL END AS leverage".to_string(),
            ],
        );
    }

    #[test]
    fn configured_periods_are_clamped_and_reject_zero_after_conversion() {
        let mut config = IndicatorConfig::default();
        config.periods.ema = ParamSpec::new(9_999.0, 50.0, 500.0);
        config.periods.rsi = ParamSpec::new(-50.0, 5.0, 50.0);
        let periods = indicator_periods(&config.periods).unwrap();

        assert_eq!(periods.ema, 500);
        assert_eq!(periods.rsi, 5);
        assert_eq!(periods.volume_ema, 20);
        assert_eq!(periods.structure_sma, 16);

        config.periods.atr = ParamSpec::new(0.0, 0.0, 0.0);
        let error = indicator_periods(&config.periods).unwrap_err();
        assert_eq!(error.kind, ErrorKind::ValidationError);
        assert_eq!(error.message, "atr period must be greater than zero");
    }

    #[tokio::test]
    async fn aggregate_collects_complete_23_field_rows_with_current_row_dependencies() {
        let candles = klines();
        let default_config = IndicatorConfig::default();
        let rows = collect_telegram_rows(
            &candles,
            indicator_periods(&default_config.periods).unwrap(),
            default_config.gap_zones.body_ratio_threshold.clamped(),
            default_config.gap_zones.atr_band_multiplier.clamped(),
            default_config.gap_zones.atr_gap_multiplier.clamped(),
        )
        .await
        .unwrap();

        assert_eq!(rows.len(), candles.len());
        for (row, candle) in rows.iter().zip(&candles) {
            assert_complete_row(row);
            let atr = row.atr.unwrap();
            assert_close(
                row.atr_upperband.unwrap(),
                candle.open + 1.618 * atr,
                "atr_upperband",
            );
            assert_close(
                row.atr_lowerband.unwrap(),
                candle.open - 1.618 * atr,
                "atr_lowerband",
            );
            assert_close(row.atr_percent.unwrap(), atr / candle.open, "atr_percent");
        }
    }

    #[tokio::test]
    async fn configured_period_changes_are_propagated_through_the_aggregate() {
        let candles = varied_klines();
        let default_config = IndicatorConfig::default();
        let default_threshold = default_config.gap_zones.body_ratio_threshold.clamped();
        let default_band = default_config.gap_zones.atr_band_multiplier.clamped();
        let default_gap = default_config.gap_zones.atr_gap_multiplier.clamped();
        let default_rows = collect_telegram_rows(
            &candles,
            indicator_periods(&default_config.periods).unwrap(),
            default_threshold,
            default_band,
            default_gap,
        )
        .await
        .unwrap();
        let default = default_rows.last().unwrap();
        let configured_row = |config: IndicatorConfig| {
            let candles = candles.clone();
            async move {
                let threshold = config.gap_zones.body_ratio_threshold.clamped();
                let band = config.gap_zones.atr_band_multiplier.clamped();
                let gap = config.gap_zones.atr_gap_multiplier.clamped();
                collect_telegram_rows(
                    &candles,
                    indicator_periods(&config.periods).unwrap(),
                    threshold,
                    band,
                    gap,
                )
                .await
                .unwrap()
                .pop()
                .unwrap()
            }
        };

        let mut config = IndicatorConfig::default();
        config.periods.ema = ParamSpec::new(50.0, 50.0, 500.0);
        assert_ne!(
            default.ema200,
            configured_row(config).await.ema200,
            "ema -> ema200"
        );

        let mut config = IndicatorConfig::default();
        config.periods.rsi = ParamSpec::new(5.0, 5.0, 50.0);
        assert_ne!(
            default.rssi,
            configured_row(config).await.rssi,
            "rsi -> rssi"
        );

        let mut config = IndicatorConfig::default();
        config.periods.rsi_smooth = ParamSpec::new(3.0, 3.0, 30.0);
        assert_ne!(
            default.rssi_ma,
            configured_row(config).await.rssi_ma,
            "rsi_smooth -> rssi_ma"
        );

        let mut config = IndicatorConfig::default();
        config.periods.reverse_rsi = ParamSpec::new(5.0, 5.0, 50.0);
        let configured = configured_row(config).await;
        assert_ne!(default.neutral_revrsi, configured.neutral_revrsi);
        assert_ne!(default.bullish_revrsi, configured.bullish_revrsi);
        assert_ne!(default.bearish_revrsi, configured.bearish_revrsi);

        let mut config = IndicatorConfig::default();
        config.periods.atr = ParamSpec::new(10.0, 10.0, 100.0);
        let configured = configured_row(config).await;
        assert_ne!(default.atr, configured.atr);
        assert_ne!(default.atr_upperband, configured.atr_upperband);
        assert_ne!(default.atr_lowerband, configured.atr_lowerband);
        assert_ne!(default.atr_percent, configured.atr_percent);

        let mut config = IndicatorConfig::default();
        config.periods.bias = ParamSpec::new(3.0, 3.0, 30.0);
        assert_ne!(
            default.bias_reversion,
            configured_row(config).await.bias_reversion,
            "bias -> bias_reversion"
        );

        let mut config = IndicatorConfig::default();
        config.periods.structure = ParamSpec::new(3.0, 3.0, 30.0);
        let configured = configured_row(config).await;
        assert_ne!(default.structure_power, configured.structure_power);
        assert_ne!(default.structure_power_sma, configured.structure_power_sma);

        let mut config = IndicatorConfig::default();
        config.periods.sharpe = ParamSpec::new(50.0, 50.0, 500.0);
        assert_ne!(
            default.sharpe,
            configured_row(config).await.sharpe,
            "sharpe -> sharpe"
        );

        let periods = indicator_periods(&IndicatorConfig::default().periods).unwrap();
        assert_eq!(periods.volume_ema, 20);
        assert_eq!(periods.structure_sma, 16);
    }

    #[tokio::test]
    async fn aggregate_atr_reversion_uses_bias_signal_while_gap_uses_close_signal() {
        let candles = klines();
        let config = IndicatorConfig::default();
        let periods = indicator_periods(&config.periods).unwrap();
        let body_ratio_threshold = config.gap_zones.body_ratio_threshold.clamped();
        let atr_band_multiplier = config.gap_zones.atr_band_multiplier.clamped();
        let atr_gap_multiplier = config.gap_zones.atr_gap_multiplier.clamped();
        let rows = collect_telegram_rows(
            &candles,
            periods,
            body_ratio_threshold,
            atr_band_multiplier,
            atr_gap_multiplier,
        )
        .await
        .unwrap();

        let mut saw_out_of_band_bias_reversion = false;
        let mut saw_in_band_bias_reversion = false;
        for (position, (candle, row)) in candles.iter().zip(&rows).enumerate() {
            let atr = row.atr.unwrap();
            let bias_reversion = row.bias_reversion.unwrap();
            let bias_point = BandPoint {
                open: candle.open,
                atr,
                signal: bias_reversion,
            };
            let expected_percent = band_reversion_percent(atr_band_multiplier)
                .transition(PriorState::Initial, &bias_point)
                .unwrap()
                .output
                .unwrap();
            let expected_reversion = band_reversion(atr_band_multiplier)
                .transition(PriorState::Initial, &bias_point)
                .unwrap()
                .output
                .unwrap();
            assert_close(
                row.atr_reversion_percent.unwrap(),
                expected_percent,
                "atr_reversion_percent must use bias_reversion",
            );
            assert_close(
                row.band_reversion.unwrap(),
                expected_reversion,
                "band_reversion must use bias_reversion",
            );

            if expected_percent.abs() <= 1e-12 {
                saw_in_band_bias_reversion = true;
                assert_eq!(row.atr_reversion_percent, Some(0.0));
                assert_eq!(row.band_reversion, Some(0.0));
            } else {
                saw_out_of_band_bias_reversion = true;
                let open_signal_percent = band_reversion_percent(atr_band_multiplier)
                    .transition(
                        PriorState::Initial,
                        &BandPoint {
                            open: candle.open,
                            atr,
                            signal: candle.open,
                        },
                    )
                    .unwrap()
                    .output
                    .unwrap();
                assert_eq!(
                    open_signal_percent, 0.0,
                    "row {position} open signal is in-band"
                );
                assert!(
                    row.atr_reversion_percent.unwrap().abs() > 1e-12,
                    "row {position} bias signal must be distinguishably out-of-band"
                );
            }
        }
        assert!(
            saw_in_band_bias_reversion,
            "controlled candles must include an in-band bias reversion"
        );
        assert!(
            saw_out_of_band_bias_reversion,
            "controlled candles must include an out-of-band bias reversion"
        );

        let gap_candle = Kline {
            open: 400.0,
            high: 415.0,
            low: 400.0,
            close: 412.0,
            volume: 1_000.0,
            time: 1_700_000_000_000 + candles.len() as i64 * 60_000,
            adjclose: None,
        };
        let mut gap_candles = candles;
        gap_candles.push(gap_candle);
        let gap_rows = collect_telegram_rows(
            &gap_candles,
            periods,
            body_ratio_threshold,
            atr_band_multiplier,
            0.5,
        )
        .await
        .unwrap();
        let gap_row = gap_rows.last().unwrap();
        let atr = gap_row.atr.unwrap();
        let close_gap = is_atr_gap(0.5)
            .transition(
                PriorState::Initial,
                &BandPoint {
                    open: gap_candle.open,
                    atr,
                    signal: gap_candle.close,
                },
            )
            .unwrap()
            .output;
        let open_gap = is_atr_gap(0.5)
            .transition(
                PriorState::Initial,
                &BandPoint {
                    open: gap_candle.open,
                    atr,
                    signal: gap_candle.open,
                },
            )
            .unwrap()
            .output;
        assert_eq!(gap_row.is_atr_gap, close_gap);
        assert_eq!(close_gap, Some(true));
        assert_eq!(open_gap, Some(false));
    }

    #[tokio::test]
    async fn aggregate_rows_match_static_23_field_early_and_mature_fixtures() {
        let default_config = IndicatorConfig::default();
        let rows = collect_telegram_rows(
            &klines(),
            indicator_periods(&default_config.periods).unwrap(),
            default_config.gap_zones.body_ratio_threshold.clamped(),
            default_config.gap_zones.atr_band_multiplier.clamped(),
            default_config.gap_zones.atr_gap_multiplier.clamped(),
        )
        .await
        .unwrap();
        assert_telegram_row_fixture(
            &rows[0],
            TelegramIndicatorRow {
                atr: Some(3.0),
                volume_sma: Some(1_000.0),
                ema200: Some(100.5),
                bias_reversion: Some(98.5),
                neutral_revrsi: Some(101.5),
                bullish_revrsi: Some(102.0),
                bearish_revrsi: Some(99.0),
                atr_upperband: Some(104.854),
                atr_lowerband: Some(95.146),
                rssi: Some(50.0),
                rssi_ma: Some(50.0),
                structure_power: Some(1.5),
                structure_power_sma: Some(1.5),
                atr_percent: Some(0.03),
                atr_reversion_percent: Some(0.0),
                band_reversion: Some(0.0),
                sharpe: Some(0.0),
                body_ratio: Some(0.166_666_666_666_666_66),
                is_atr_gap: Some(false),
                gap_candidate_qualifies: false,
                gap_candidate_body_bottom: None,
                gap_candidate_body_top: None,
                gap_candidate_direction: None,
            },
            "early",
        );
        assert_telegram_row_fixture(
            &rows[299],
            TelegramIndicatorRow {
                atr: Some(3.0),
                volume_sma: Some(1_000.0),
                ema200: Some(305.003_475_280_649_47),
                bias_reversion: Some(393.5),
                neutral_revrsi: Some(387.500_000_003_095_47),
                bullish_revrsi: Some(395.428_571_429_898_06),
                bearish_revrsi: Some(367.666_666_673_889_4),
                atr_upperband: Some(403.854),
                atr_lowerband: Some(394.146),
                rssi: Some(100.0),
                rssi_ma: Some(100.0),
                structure_power: Some(1.5),
                structure_power_sma: Some(1.5),
                atr_percent: Some(0.007_518_796_992_481_203),
                atr_reversion_percent: Some(-13.308_611_454_470_85),
                band_reversion: Some(-0.646),
                sharpe: Some(1.505_290_731_575_520_2),
                body_ratio: Some(0.166_666_666_666_666_66),
                is_atr_gap: Some(false),
                gap_candidate_qualifies: false,
                gap_candidate_body_bottom: None,
                gap_candidate_body_top: None,
                gap_candidate_direction: None,
            },
            "mature",
        );
    }

    #[tokio::test]
    async fn direct_pipeline_preserves_zero_based_stamps_before_row_extraction() {
        let candles = [klines()[2], klines()[0], klines()[1]];
        let source =
            futures::stream::iter(candles.iter().cloned().enumerate().map(|(id, value)| {
                Ok::<_, StreamPipelineError<Infallible, usize>>(Stamped { id, value })
            }));
        let default_config = IndicatorConfig::default();
        let stamped: Vec<Stamped<usize, TelegramIndicatorRow>> = match kernel_stream_pipeline(
            source,
            TelegramIndicators::new(
                indicator_periods(&default_config.periods).unwrap(),
                default_config.gap_zones.body_ratio_threshold.clamped(),
                default_config.gap_zones.atr_band_multiplier.clamped(),
                default_config.gap_zones.atr_gap_multiplier.clamped(),
            ),
        )
        .try_collect()
        .await
        {
            Ok(rows) => rows,
            Err(_) => panic!("valid stamped source must collect"),
        };

        assert_eq!(stamped.len(), candles.len());
        assert_eq!(
            stamped.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        let rows = stamped.into_iter().map(|row| row.value).collect::<Vec<_>>();
        assert_eq!(rows.len(), candles.len());
    }

    #[tokio::test]
    async fn gap_candidate_scalars_equal_shipped_facts_for_every_supplied_row() {
        let mut varied = varied_klines();
        varied.truncate(3);
        let mut supplied = vec![klines()[0]];
        supplied.extend(varied);
        supplied.push(Kline {
            open: 1_000.0,
            high: 1_001.0,
            low: 999.0,
            close: 10_000.0,
            volume: 1_000.0,
            time: 1_700_000_000_000 + 4 * 60_000,
            adjclose: Some(9_999.0),
        });

        let default_config = IndicatorConfig::default();
        let body_ratio_threshold = default_config.gap_zones.body_ratio_threshold.clamped();
        let atr_band_multiplier = default_config.gap_zones.atr_band_multiplier.clamped();
        let atr_gap_multiplier = default_config.gap_zones.atr_gap_multiplier.clamped();
        let rows = collect_telegram_rows(
            &supplied,
            indicator_periods(&default_config.periods).unwrap(),
            body_ratio_threshold,
            atr_band_multiplier,
            atr_gap_multiplier,
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), supplied.len());

        let mut saw_qualifying = false;
        let mut saw_non_qualifying = false;
        for (position, (kline, row)) in supplied.iter().zip(&rows).enumerate() {
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
                body_ratio_threshold,
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
        assert!(saw_qualifying, "fixture must include a qualifying row");
        assert!(
            saw_non_qualifying,
            "fixture must include a non-qualifying row"
        );
    }

    #[tokio::test]
    async fn persisted_body_ratio_threshold_drives_scalar_candidate_detection() {
        let mut candles = klines();
        candles.push(Kline {
            open: 400.0,
            high: 420.0,
            low: 400.0,
            close: 412.0,
            volume: 1_000.0,
            time: 1_700_000_000_000 + 300 * 60_000,
            adjclose: None,
        });
        let tail = candles.len() - 1;

        let default_config = IndicatorConfig::default();
        let default_threshold = default_config.gap_zones.body_ratio_threshold.clamped();
        assert!(
            (default_threshold - 0.618).abs() <= 1e-12,
            "persisted default must remain 0.618, got {default_threshold}"
        );
        let periods = indicator_periods(&default_config.periods).unwrap();
        let default_band = default_config.gap_zones.atr_band_multiplier.clamped();
        let default_gap = default_config.gap_zones.atr_gap_multiplier.clamped();
        let default_rows = collect_telegram_rows(
            &candles,
            periods,
            default_threshold,
            default_band,
            default_gap,
        )
        .await
        .unwrap();
        assert_eq!(default_rows.len(), candles.len());
        let default_tail = &default_rows[tail];
        let body_ratio = default_tail
            .body_ratio
            .expect("tail must preserve upstream body_ratio");
        let is_atr_gap = default_tail
            .is_atr_gap
            .expect("tail must preserve upstream is_atr_gap");
        assert!(
            (body_ratio - 0.6).abs() <= 1e-12,
            "tailored body_ratio must be 0.6, got {body_ratio}"
        );
        assert!(
            is_atr_gap,
            "tailored candle must be an ATR gap so the threshold decides"
        );
        assert!(
            !default_tail.gap_candidate_qualifies,
            "0.6 must not qualify under persisted 0.618"
        );

        let mut custom_config = IndicatorConfig::default();
        custom_config.gap_zones.body_ratio_threshold = ParamSpec::new(0.5, 0.0, 1.0);
        let custom_threshold = custom_config.gap_zones.body_ratio_threshold.clamped();
        assert!(
            (custom_threshold - 0.5).abs() <= 1e-12,
            "custom persisted threshold must clamp to 0.5, got {custom_threshold}"
        );
        let custom_band = custom_config.gap_zones.atr_band_multiplier.clamped();
        let custom_gap = custom_config.gap_zones.atr_gap_multiplier.clamped();
        let custom_rows =
            collect_telegram_rows(&candles, periods, custom_threshold, custom_band, custom_gap)
                .await
                .unwrap();
        assert_eq!(custom_rows.len(), candles.len());

        for (position, (default_row, custom_row)) in
            default_rows.iter().zip(custom_rows.iter()).enumerate()
        {
            assert_eq!(
                default_row.body_ratio, custom_row.body_ratio,
                "row {position} upstream body_ratio must be threshold-independent"
            );
            assert_eq!(
                default_row.is_atr_gap, custom_row.is_atr_gap,
                "row {position} upstream is_atr_gap must be threshold-independent"
            );
            for (threshold, row) in [
                (default_threshold, default_row),
                (custom_threshold, custom_row),
            ] {
                let expected = gap_candidate_facts(GapCandidateInput {
                    open: candles[position].open,
                    close: candles[position].close,
                    is_atr_gap: row.is_atr_gap.unwrap(),
                    body_ratio: row.body_ratio.unwrap(),
                    body_ratio_threshold: threshold,
                })
                .unwrap_or_else(|_| panic!("row {position} has valid scalar inputs"));
                assert_eq!(
                    row.gap_candidate_qualifies, expected.qualifies,
                    "row {position} qualifies must equal shipped facts at {threshold}"
                );
            }
        }

        let custom_tail = &custom_rows[tail];
        assert_eq!(
            custom_tail.body_ratio, default_tail.body_ratio,
            "tail upstream body_ratio must remain unchanged"
        );
        assert_eq!(
            custom_tail.is_atr_gap, default_tail.is_atr_gap,
            "tail upstream is_atr_gap must remain unchanged"
        );
        assert!(
            custom_tail.gap_candidate_qualifies,
            "0.6 must qualify under customized persisted 0.5"
        );
        assert_ne!(
            default_tail.gap_candidate_qualifies, custom_tail.gap_candidate_qualifies,
            "customized persisted threshold must flip tail qualifies"
        );
        assert_eq!(custom_tail.gap_candidate_body_bottom, Some(400.0));
        assert_eq!(custom_tail.gap_candidate_body_top, Some(412.0));
        assert_eq!(
            default_tail.gap_candidate_body_bottom, None,
            "non-qualifying tail must carry no bounds"
        );
        assert_eq!(
            default_tail.gap_candidate_body_top, None,
            "non-qualifying tail must carry no bounds"
        );
    }

    #[tokio::test]
    async fn persisted_atr_band_multiplier_drives_bands() {
        let candles = klines();
        let default_config = IndicatorConfig::default();
        let periods = indicator_periods(&default_config.periods).unwrap();
        let threshold = default_config.gap_zones.body_ratio_threshold.clamped();
        let default_band = default_config.gap_zones.atr_band_multiplier.clamped();
        let default_gap = default_config.gap_zones.atr_gap_multiplier.clamped();
        assert!(
            (default_band - 1.618).abs() <= 1e-12,
            "persisted default band must remain 1.618, got {default_band}"
        );
        assert!(
            (default_gap - 1.0).abs() <= 1e-12,
            "persisted default gap must remain 1.0, got {default_gap}"
        );
        let default_rows =
            collect_telegram_rows(&candles, periods, threshold, default_band, default_gap)
                .await
                .unwrap();
        let custom_band = 3.0;
        let custom_rows =
            collect_telegram_rows(&candles, periods, threshold, custom_band, default_gap)
                .await
                .unwrap();
        assert_eq!(default_rows.len(), candles.len());
        assert_eq!(custom_rows.len(), candles.len());
        let mut saw_band_difference = false;
        for (position, ((candle, default_row), custom_row)) in candles
            .iter()
            .zip(&default_rows)
            .zip(&custom_rows)
            .enumerate()
        {
            let atr = default_row.atr.unwrap();
            assert_eq!(
                custom_row.atr, default_row.atr,
                "row {position} upstream atr must be band-independent"
            );
            assert_eq!(
                custom_row.body_ratio, default_row.body_ratio,
                "row {position} upstream body_ratio must be band-independent"
            );
            assert_eq!(
                custom_row.is_atr_gap, default_row.is_atr_gap,
                "row {position} upstream is_atr_gap must be band-independent"
            );
            assert_close(
                default_row.atr_upperband.unwrap(),
                candle.open + default_band * atr,
                "default atr_upperband",
            );
            assert_close(
                default_row.atr_lowerband.unwrap(),
                candle.open - default_band * atr,
                "default atr_lowerband",
            );
            assert_close(
                custom_row.atr_upperband.unwrap(),
                candle.open + custom_band * atr,
                "custom atr_upperband",
            );
            assert_close(
                custom_row.atr_lowerband.unwrap(),
                candle.open - custom_band * atr,
                "custom atr_lowerband",
            );
            if (default_row.atr_upperband.unwrap() - custom_row.atr_upperband.unwrap()).abs()
                > 1e-12
            {
                saw_band_difference = true;
            }
            assert_ne!(
                default_row.atr_upperband, custom_row.atr_upperband,
                "row {position} upperband must move with band multiplier"
            );
            assert_ne!(
                default_row.atr_lowerband, custom_row.atr_lowerband,
                "row {position} lowerband must move with band multiplier"
            );
        }
        assert!(
            saw_band_difference,
            "band multiplier must change at least one band"
        );

        // End-to-end: compute_telegram_frame threads the persisted band value.
        let mut custom_config = IndicatorConfig::default();
        custom_config.gap_zones.atr_band_multiplier = ParamSpec::new(3.0, 0.5, 5.0);
        let (default_frame, _) = compute_telegram_frame(
            candles.clone(),
            ValidatedTicker::new("BTCUSDT", 0.02, 0.01).unwrap(),
            &IndicatorConfig::default(),
        )
        .await
        .unwrap();
        let (custom_frame, _) = compute_telegram_frame(
            candles.clone(),
            ValidatedTicker::new("BTCUSDT", 0.02, 0.01).unwrap(),
            &custom_config,
        )
        .await
        .unwrap();
        let position = candles.len() - 1;
        let default_upper = default_frame
            .f64_at("atr_upperband", position)
            .unwrap()
            .unwrap();
        let custom_upper = custom_frame
            .f64_at("atr_upperband", position)
            .unwrap()
            .unwrap();
        assert!(
            (default_upper - custom_upper).abs() > 1e-12,
            "compute_telegram_frame must thread persisted band multiplier"
        );
    }

    #[tokio::test]
    async fn persisted_atr_gap_multiplier_drives_gap_qualification() {
        let mut candles = klines();
        candles.push(Kline {
            open: 400.0,
            high: 415.0,
            low: 400.0,
            close: 412.0,
            volume: 1_000.0,
            time: 1_700_000_000_000 + 300 * 60_000,
            adjclose: None,
        });
        let tail = candles.len() - 1;
        let default_config = IndicatorConfig::default();
        let periods = indicator_periods(&default_config.periods).unwrap();
        let threshold = default_config.gap_zones.body_ratio_threshold.clamped();
        let band = default_config.gap_zones.atr_band_multiplier.clamped();
        let default_gap = default_config.gap_zones.atr_gap_multiplier.clamped();
        assert!(
            (default_gap - 1.0).abs() <= 1e-12,
            "persisted default gap must remain 1.0, got {default_gap}"
        );
        let narrow_rows = collect_telegram_rows(&candles, periods, threshold, band, 0.5)
            .await
            .unwrap();
        let wide_rows = collect_telegram_rows(&candles, periods, threshold, band, 5.0)
            .await
            .unwrap();
        assert_eq!(narrow_rows.len(), candles.len());
        assert_eq!(wide_rows.len(), candles.len());
        let narrow_tail = &narrow_rows[tail];
        let wide_tail = &wide_rows[tail];
        assert_eq!(
            narrow_tail.body_ratio, wide_tail.body_ratio,
            "tail upstream body_ratio must remain unchanged"
        );
        assert_eq!(
            narrow_tail.atr, wide_tail.atr,
            "tail upstream atr must remain unchanged"
        );
        assert_eq!(
            narrow_tail.atr_upperband, wide_tail.atr_upperband,
            "tail bands must remain unchanged when only gap multiplier moves"
        );
        assert_eq!(
            narrow_tail.atr_lowerband, wide_tail.atr_lowerband,
            "tail bands must remain unchanged when only gap multiplier moves"
        );
        assert_eq!(narrow_tail.is_atr_gap, Some(true));
        assert_eq!(wide_tail.is_atr_gap, Some(false));
        assert!(
            narrow_tail.gap_candidate_qualifies,
            "narrow gap must qualify with body_ratio 0.8 under 0.618"
        );
        assert!(
            !wide_tail.gap_candidate_qualifies,
            "wide gap must not qualify when is_atr_gap flips false"
        );
        assert_ne!(
            narrow_tail.is_atr_gap, wide_tail.is_atr_gap,
            "customized persisted gap multiplier must flip tail is_atr_gap"
        );
        assert_ne!(
            narrow_tail.gap_candidate_qualifies, wide_tail.gap_candidate_qualifies,
            "customized persisted gap multiplier must flip tail qualifies"
        );
        assert_eq!(narrow_tail.gap_candidate_body_bottom, Some(400.0));
        assert_eq!(narrow_tail.gap_candidate_body_top, Some(412.0));
        assert_eq!(wide_tail.gap_candidate_body_bottom, None);
        assert_eq!(wide_tail.gap_candidate_body_top, None);

        // End-to-end: compute_telegram_frame threads the persisted gap value.
        let mut narrow_config = IndicatorConfig::default();
        narrow_config.gap_zones.atr_gap_multiplier = ParamSpec::new(0.5, 0.5, 5.0);
        let mut wide_config = IndicatorConfig::default();
        wide_config.gap_zones.atr_gap_multiplier = ParamSpec::new(5.0, 0.5, 5.0);
        let narrow_frame_rows = collect_telegram_rows(
            &candles,
            periods,
            narrow_config.gap_zones.body_ratio_threshold.clamped(),
            narrow_config.gap_zones.atr_band_multiplier.clamped(),
            narrow_config.gap_zones.atr_gap_multiplier.clamped(),
        )
        .await
        .unwrap();
        let wide_frame_rows = collect_telegram_rows(
            &candles,
            periods,
            wide_config.gap_zones.body_ratio_threshold.clamped(),
            wide_config.gap_zones.atr_band_multiplier.clamped(),
            wide_config.gap_zones.atr_gap_multiplier.clamped(),
        )
        .await
        .unwrap();
        assert_ne!(
            narrow_frame_rows[tail].is_atr_gap, wide_frame_rows[tail].is_atr_gap,
            "persisted gap multiplier must flip is_atr_gap end-to-end"
        );
    }

    #[tokio::test]
    async fn collect_and_compute_handle_empty_one_multi_and_invalid_inputs() {
        let default_config = IndicatorConfig::default();
        let periods = indicator_periods(&default_config.periods).unwrap();
        let body_ratio_threshold = default_config.gap_zones.body_ratio_threshold.clamped();
        let atr_band_multiplier = default_config.gap_zones.atr_band_multiplier.clamped();
        let atr_gap_multiplier = default_config.gap_zones.atr_gap_multiplier.clamped();
        assert!(
            collect_telegram_rows(
                &[],
                periods,
                body_ratio_threshold,
                atr_band_multiplier,
                atr_gap_multiplier
            )
            .await
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            collect_telegram_rows(
                &klines()[..1],
                periods,
                body_ratio_threshold,
                atr_band_multiplier,
                atr_gap_multiplier
            )
            .await
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            collect_telegram_rows(
                &klines()[..3],
                periods,
                body_ratio_threshold,
                atr_band_multiplier,
                atr_gap_multiplier
            )
            .await
            .unwrap()
            .len(),
            3
        );

        let (empty, empty_zones) = compute_telegram_frame(
            Vec::new(),
            ValidatedTicker::new("BTCUSDT", 0.02, 0.01).unwrap(),
            &IndicatorConfig::default(),
        )
        .await
        .unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.columns(), expected_columns(DEFAULT_FRAME_OUTPUTS));
        assert!(empty_zones.is_empty());

        let mut invalid = klines()[..3].to_vec();
        invalid[2].high = f64::NAN;
        let error = collect_telegram_rows(
            &invalid,
            periods,
            body_ratio_threshold,
            atr_band_multiplier,
            atr_gap_multiplier,
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::ValidationError);
        assert_eq!(error.message, "high input must be finite");
    }

    #[tokio::test]
    async fn every_toggle_projects_only_its_canonical_final_column_without_duplicates() {
        for output in DEFAULT_FRAME_OUTPUTS {
            let config = only_active_outputs(&[output]);
            let frame = compute_default_frame(&config).await;
            assert_eq!(frame.columns(), expected_columns(&[output]), "{output}");
            assert_eq!(
                frame.columns().len(),
                frame.columns().iter().collect::<HashSet<_>>().len(),
                "{output}"
            );
        }
    }

    #[tokio::test]
    async fn dependent_outputs_remain_visible_without_visible_prerequisites() {
        for output in [
            "rssi_ma",
            "structure_power_sma",
            "atr_upperband",
            "atr_lowerband",
            "atr_percent",
            "atr_reversion_percent",
            "band_reversion",
            "is_atr_gap",
        ] {
            let frame = compute_default_frame(&only_active_outputs(&[output])).await;
            assert_eq!(frame.columns(), expected_columns(&[output]), "{output}");
            assert!(!frame.has_column("atr"), "{output}");
        }
    }

    #[test]
    fn projector_rejects_mismatched_cardinality_as_a_computation_error() {
        let error = telegram_output_frame(
            Vec::new(),
            &klines()[..1],
            &IndicatorConfig::default().outputs,
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::ComputationError);
        assert_eq!(
            error.message,
            "indicator row count does not match market row count"
        );
    }

    #[test]
    fn gap_candidate_source_frame_encodes_bullish_bearish_flat_and_noncandidate() {
        fn row_with_facts(
            open: f64,
            close: f64,
            is_atr_gap: bool,
            body_ratio: f64,
        ) -> TelegramIndicatorRow {
            let body_ratio_threshold = IndicatorConfig::default()
                .gap_zones
                .body_ratio_threshold
                .clamped();
            let facts = gap_candidate_facts(GapCandidateInput {
                open,
                close,
                is_atr_gap,
                body_ratio,
                body_ratio_threshold,
            })
            .unwrap();
            let mut row = sample_row();
            row.body_ratio = Some(body_ratio);
            row.is_atr_gap = Some(is_atr_gap);
            row.gap_candidate_qualifies = facts.qualifies;
            row.gap_candidate_body_bottom = facts.body_bottom;
            row.gap_candidate_body_top = facts.body_top;
            row.gap_candidate_direction = facts.direction;
            row
        }

        let rows = vec![
            row_with_facts(100.0, 105.0, true, 0.8),
            row_with_facts(105.0, 100.0, true, 0.8),
            row_with_facts(100.0, 100.0, true, 0.8),
            row_with_facts(100.0, 105.0, false, 0.5),
        ];
        let candles = klines()[..rows.len()].to_vec();
        let config = only_active_outputs(&[]);
        let frame = telegram_output_frame(rows, &candles, &config.outputs).unwrap();

        assert_eq!(frame.len(), 4);
        assert_eq!(
            frame.column_names(),
            vec![
                "open",
                "high",
                "low",
                "close",
                "volume",
                "time",
                "adj_close",
                "iching_original_energy",
                "iching_transformed_energy",
                "iching_mutual_energy",
                "iching_open",
                "iching_high",
                "iching_low",
                "iching_close",
                "iching_moving_line",
                "iching_transformed_close",
                "iching_mutual_close",
                "iching_mutual_high",
                "iching_mutual_low",
                "iching_mutual_mean",
                "gap_candidate_qualifies",
                "gap_candidate_body_bottom",
                "gap_candidate_body_top",
                "gap_candidate_direction",
            ]
        );

        match frame.column("gap_candidate_qualifies") {
            Some(SourceColumnData::Boolean(values)) => {
                assert_eq!(
                    values.as_slice(),
                    &[Some(true), Some(true), Some(true), Some(false)]
                );
            }
            _ => panic!("gap_candidate_qualifies must be Boolean"),
        }
        match frame.column("gap_candidate_body_bottom") {
            Some(SourceColumnData::Number(values)) => {
                assert_eq!(
                    values.as_slice(),
                    &[Some(100.0), Some(100.0), Some(100.0), None]
                );
            }
            _ => panic!("gap_candidate_body_bottom must be Number"),
        }
        match frame.column("gap_candidate_body_top") {
            Some(SourceColumnData::Number(values)) => {
                assert_eq!(
                    values.as_slice(),
                    &[Some(105.0), Some(105.0), Some(100.0), None]
                );
            }
            _ => panic!("gap_candidate_body_top must be Number"),
        }
        match frame.column("gap_candidate_direction") {
            Some(SourceColumnData::Text(values)) => {
                let expected = [
                    Some("bullish".to_string()),
                    Some("bearish".to_string()),
                    Some("flat".to_string()),
                    None,
                ];
                assert_eq!(values.as_slice(), expected.as_slice());
            }
            _ => panic!("gap_candidate_direction must be Text"),
        }
    }

    #[test]
    fn source_frame_preserves_raw_source_fields_at_each_position() {
        for length in [2, 3] {
            let mut candles = klines()[..length].to_vec();
            if length > 1 {
                candles[0].adjclose = Some(candles[0].open + 0.25);
                candles[length - 1].adjclose = Some(candles[length - 1].close - 0.25);
            }
            let rows = (0..length).map(|_| sample_row()).collect::<Vec<_>>();
            let config = only_active_outputs(&[]);
            let frame = telegram_output_frame(rows, &candles, &config.outputs).unwrap();

            assert_eq!(frame.len(), length);
            assert_eq!(
                &frame.column_names()[..7],
                &[
                    "open",
                    "high",
                    "low",
                    "close",
                    "volume",
                    "time",
                    "adj_close"
                ]
            );
            let SourceColumnData::Number(adj_close) = frame.column("adj_close").unwrap() else {
                panic!("adj_close must be Number");
            };

            for (position, candle) in candles.iter().enumerate() {
                for (column, expected) in [
                    ("open", candle.open),
                    ("high", candle.high),
                    ("low", candle.low),
                    ("close", candle.close),
                    ("volume", candle.volume),
                    ("time", candle.time as f64),
                ] {
                    assert_eq!(
                        frame.f64_at(column, position).unwrap(),
                        Some(expected),
                        "{column} at row {position}"
                    );
                }
                assert_eq!(adj_close[position], candle.adjclose);
            }
        }
    }

    #[test]
    fn source_frame_schema_is_configuration_dependent_with_stable_candidate_tail() {
        let default_config = IndicatorConfig::default();
        let inactive_config = only_active_outputs(&[]);
        let leverage_config = only_active_outputs(&["leverage"]);
        let candles = &klines()[..2];

        let default_frame = telegram_output_frame(
            vec![sample_row(), sample_row()],
            candles,
            &default_config.outputs,
        )
        .unwrap();
        let inactive_frame = telegram_output_frame(
            vec![sample_row(), sample_row()],
            candles,
            &inactive_config.outputs,
        )
        .unwrap();
        let leverage_frame = telegram_output_frame(
            vec![sample_row(), sample_row()],
            candles,
            &leverage_config.outputs,
        )
        .unwrap();

        let default_names = vec![
            "open",
            "high",
            "low",
            "close",
            "volume",
            "time",
            "adj_close",
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_mutual_energy",
            "iching_open",
            "iching_high",
            "iching_low",
            "iching_close",
            "iching_moving_line",
            "iching_transformed_close",
            "iching_mutual_close",
            "iching_mutual_high",
            "iching_mutual_low",
            "iching_mutual_mean",
            "atr",
            "volume_sma",
            "ema200",
            "bias_reversion",
            "neutral_revrsi",
            "bullish_revrsi",
            "bearish_revrsi",
            "atr_upperband",
            "atr_lowerband",
            "rssi",
            "rssi_ma",
            "structure_power",
            "structure_power_sma",
            "atr_percent",
            "atr_reversion_percent",
            "band_reversion",
            "sharpe",
            "body_ratio",
            "is_atr_gap",
            "gap_candidate_qualifies",
            "gap_candidate_body_bottom",
            "gap_candidate_body_top",
            "gap_candidate_direction",
        ];
        let inactive_names = vec![
            "open",
            "high",
            "low",
            "close",
            "volume",
            "time",
            "adj_close",
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_mutual_energy",
            "iching_open",
            "iching_high",
            "iching_low",
            "iching_close",
            "iching_moving_line",
            "iching_transformed_close",
            "iching_mutual_close",
            "iching_mutual_high",
            "iching_mutual_low",
            "iching_mutual_mean",
            "gap_candidate_qualifies",
            "gap_candidate_body_bottom",
            "gap_candidate_body_top",
            "gap_candidate_direction",
        ];
        let leverage_names = vec![
            "open",
            "high",
            "low",
            "close",
            "volume",
            "time",
            "adj_close",
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_mutual_energy",
            "iching_open",
            "iching_high",
            "iching_low",
            "iching_close",
            "iching_moving_line",
            "iching_transformed_close",
            "iching_mutual_close",
            "iching_mutual_high",
            "iching_mutual_low",
            "iching_mutual_mean",
            "__leverage_atr",
            "gap_candidate_qualifies",
            "gap_candidate_body_bottom",
            "gap_candidate_body_top",
            "gap_candidate_direction",
        ];
        let candidate_tail = [
            "gap_candidate_qualifies",
            "gap_candidate_body_bottom",
            "gap_candidate_body_top",
            "gap_candidate_direction",
        ];

        assert_eq!(default_frame.column_names(), default_names);
        assert_eq!(inactive_frame.column_names(), inactive_names);
        assert_eq!(leverage_frame.column_names(), leverage_names);
        assert_eq!(default_frame.len(), 2);
        assert_eq!(inactive_frame.len(), 2);
        assert_eq!(leverage_frame.len(), 2);

        for frame in [&default_frame, &inactive_frame, &leverage_frame] {
            let names = frame.column_names();
            assert_eq!(&names[names.len() - 4..], candidate_tail.as_slice());
        }
        assert_eq!(
            &leverage_frame.column_names()[leverage_frame.column_names().len() - 5..],
            ["__leverage_atr",]
                .iter()
                .chain(candidate_tail.iter())
                .copied()
                .collect::<Vec<_>>()
                .as_slice()
        );

        for name in candidate_tail {
            assert_eq!(default_frame.column(name), inactive_frame.column(name));
            assert_eq!(default_frame.column(name), leverage_frame.column(name));
        }
    }

    #[test]
    fn empty_source_frames_retain_configuration_schema_and_typed_zero_lengths() {
        fn assert_empty_columns(frame: &SourceFrame, names: &[&str]) {
            for name in names {
                match frame.column(name).unwrap() {
                    SourceColumnData::Number(values) => {
                        assert!(values.is_empty(), "{name} must have zero length")
                    }
                    SourceColumnData::Boolean(values) => {
                        assert!(values.is_empty(), "{name} must have zero length")
                    }
                    SourceColumnData::Text(values) => {
                        assert!(values.is_empty(), "{name} must have zero length")
                    }
                }
            }
        }

        let default_config = IndicatorConfig::default();
        let inactive_config = only_active_outputs(&[]);
        let default_frame =
            telegram_output_frame(Vec::new(), &[], &default_config.outputs).unwrap();
        let inactive_frame =
            telegram_output_frame(Vec::new(), &[], &inactive_config.outputs).unwrap();

        let default_names = [
            "open",
            "high",
            "low",
            "close",
            "volume",
            "time",
            "adj_close",
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_mutual_energy",
            "iching_open",
            "iching_high",
            "iching_low",
            "iching_close",
            "iching_moving_line",
            "iching_transformed_close",
            "iching_mutual_close",
            "iching_mutual_high",
            "iching_mutual_low",
            "iching_mutual_mean",
            "atr",
            "volume_sma",
            "ema200",
            "bias_reversion",
            "neutral_revrsi",
            "bullish_revrsi",
            "bearish_revrsi",
            "atr_upperband",
            "atr_lowerband",
            "rssi",
            "rssi_ma",
            "structure_power",
            "structure_power_sma",
            "atr_percent",
            "atr_reversion_percent",
            "band_reversion",
            "sharpe",
            "body_ratio",
            "is_atr_gap",
            "gap_candidate_qualifies",
            "gap_candidate_body_bottom",
            "gap_candidate_body_top",
            "gap_candidate_direction",
        ];
        let inactive_names = [
            "open",
            "high",
            "low",
            "close",
            "volume",
            "time",
            "adj_close",
            "iching_original_energy",
            "iching_transformed_energy",
            "iching_mutual_energy",
            "iching_open",
            "iching_high",
            "iching_low",
            "iching_close",
            "iching_moving_line",
            "iching_transformed_close",
            "iching_mutual_close",
            "iching_mutual_high",
            "iching_mutual_low",
            "iching_mutual_mean",
            "gap_candidate_qualifies",
            "gap_candidate_body_bottom",
            "gap_candidate_body_top",
            "gap_candidate_direction",
        ];

        assert_eq!(default_frame.column_names(), default_names.to_vec());
        assert_eq!(inactive_frame.column_names(), inactive_names.to_vec());
        assert!(default_frame.is_empty());
        assert!(inactive_frame.is_empty());
        assert_empty_columns(&default_frame, &default_names);
        assert_empty_columns(&inactive_frame, &inactive_names);

        assert!(matches!(
            default_frame.column("gap_candidate_qualifies"),
            Some(SourceColumnData::Boolean(values)) if values.is_empty()
        ));
        assert!(matches!(
            default_frame.column("gap_candidate_body_bottom"),
            Some(SourceColumnData::Number(values)) if values.is_empty()
        ));
        assert!(matches!(
            default_frame.column("gap_candidate_body_top"),
            Some(SourceColumnData::Number(values)) if values.is_empty()
        ));
        assert!(matches!(
            default_frame.column("gap_candidate_direction"),
            Some(SourceColumnData::Text(values)) if values.is_empty()
        ));
    }

    #[test]
    fn private_leverage_atr_is_added_only_to_the_source_frame_when_required() {
        let candles = &klines()[..2];
        let hidden_atr = telegram_output_frame(
            vec![sample_row(), sample_row()],
            candles,
            &only_active_outputs(&["leverage"]).outputs,
        )
        .unwrap();
        assert!(matches!(
            hidden_atr.column("__leverage_atr"),
            Some(SourceColumnData::Number(_))
        ));
        assert!(!hidden_atr.column_names().contains(&"atr"));

        let visible_atr = telegram_output_frame(
            vec![sample_row(), sample_row()],
            candles,
            &only_active_outputs(&["atr", "leverage"]).outputs,
        )
        .unwrap();
        assert!(visible_atr.column("atr").is_some());
        assert!(visible_atr.column("__leverage_atr").is_none());

        let inactive_leverage = telegram_output_frame(
            vec![sample_row(), sample_row()],
            candles,
            &only_active_outputs(&["atr"]).outputs,
        )
        .unwrap();
        assert!(inactive_leverage.column("__leverage_atr").is_none());
    }

    #[test]
    fn sql_uses_only_authorized_atr_identifiers_and_preserves_ordering() {
        let hidden = only_active_outputs(&["leverage"]);
        let hidden_select = telegram_select_expressions(&hidden.outputs, 0.02, 0.01);
        assert!(hidden_select.last().unwrap().contains("__leverage_atr"));
        assert!(
            !hidden_select
                .iter()
                .any(|item| item == "\"__leverage_atr\"")
        );
        let hidden_sql = build_telegram_sql(&hidden.outputs, 0.02, 0.01);
        assert!(hidden_sql.contains("computed()"));
        assert!(!hidden_sql.contains("100.0"));
        assert!(hidden_sql.ends_with("ORDER BY time"));

        let visible = only_active_outputs(&["atr", "leverage"]);
        let visible_select = telegram_select_expressions(&visible.outputs, 0.02, 0.01);
        assert!(visible_select.last().unwrap().contains(" atr"));
        assert!(!visible_select.last().unwrap().contains("__leverage_atr"));
    }

    #[test]
    fn leverage_projection_preserves_null_warmup_and_zero_atr_without_exposing_private_atr() {
        for atr in [None, Some(0.0)] {
            let mut row = sample_row();
            row.atr = atr;
            let config = only_active_outputs(&["leverage"]);
            let source =
                telegram_output_frame(vec![row, sample_row()], &klines()[..2], &config.outputs)
                    .unwrap();

            assert!(source.has_column("__leverage_atr"));
            assert!(!source.has_column("atr"));

            let result = DuckDBQuery::new()
                .project(
                    source,
                    RawQuery::source_controlled(build_telegram_sql(&config.outputs, 0.02, 0.01)),
                )
                .unwrap();
            assert_eq!(result.f64_at("leverage", 0).unwrap(), None);
            assert!(!result.has_column("__leverage_atr"));
            assert!(!result.has_column("atr"));
        }
    }

    #[tokio::test]
    async fn leverage_nonfinite_atr_output_fails_in_ta_before_private_projection() {
        let mut candle = klines()[0];
        candle.high = f64::MAX;
        candle.low = -f64::MAX;
        let error = match compute_telegram_frame(
            vec![candle],
            ValidatedTicker::new("BTCUSDT", 0.02, 0.01).unwrap(),
            &only_active_outputs(&["leverage"]),
        )
        .await
        {
            Ok(_) => panic!("non-finite ATR output must not produce a frame"),
            Err(error) => error,
        };

        assert_eq!(error.kind, ErrorKind::ComputationError);
        assert_eq!(error.message, "OHLC true range produced a non-finite value");
    }

    #[test]
    fn malformed_source_controlled_query_propagates_duckdb_data_access_error() {
        let error = DuckDBQuery::new()
            .project(
                telegram_output_frame(
                    vec![sample_row(), sample_row()],
                    &klines()[..2],
                    &only_active_outputs(&[]).outputs,
                )
                .unwrap(),
                RawQuery::source_controlled("SELEC FROM computed()"),
            )
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::DataAccessError);
        assert!(error.message.starts_with("DuckDB result access failed:"));
    }

    #[test]
    fn aggregate_contract_has_exact_row_and_child_state_shapes() {
        fn assert_row(row: TelegramIndicatorRow) {
            let TelegramIndicatorRow {
                atr,
                volume_sma,
                ema200,
                bias_reversion,
                neutral_revrsi,
                bullish_revrsi,
                bearish_revrsi,
                atr_upperband,
                atr_lowerband,
                rssi,
                rssi_ma,
                structure_power,
                structure_power_sma,
                atr_percent,
                atr_reversion_percent,
                band_reversion,
                sharpe,
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
                rssi,
                rssi_ma,
                structure_power,
                structure_power_sma,
                atr_percent,
                atr_reversion_percent,
                band_reversion,
                sharpe,
                body_ratio,
                is_atr_gap,
                gap_candidate_qualifies,
                gap_candidate_body_bottom,
                gap_candidate_body_top,
                gap_candidate_direction,
            );
        }

        fn assert_state(state: TelegramIndicatorState) {
            let TelegramIndicatorState {
                bar_bias,
                atr,
                volume_ema,
                ema200,
                bias_reversion,
                neutral_revrsi,
                bullish_revrsi,
                bearish_revrsi,
                rsi,
                rsi_ma,
                structure_power,
                structure_power_sma,
                sharpe,
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
                rsi,
                rsi_ma,
                structure_power,
                structure_power_sma,
                sharpe,
                body_ratio,
                band_reversion,
                band_reversion_percent,
                is_atr_gap,
            );
        }

        let _ = (assert_row, assert_state);
    }

    #[test]
    fn aggregate_constructor_uses_validated_clamped_periods_and_emits_full_rows() {
        let config = IndicatorConfig::default();
        let periods = indicator_periods(&config.periods).unwrap();
        let body_ratio_threshold = config.gap_zones.body_ratio_threshold.clamped();
        let atr_band_multiplier = config.gap_zones.atr_band_multiplier.clamped();
        let atr_gap_multiplier = config.gap_zones.atr_gap_multiplier.clamped();
        let mut processor = algotrap::ta::prelude::Processor::new(TelegramIndicators::new(
            periods,
            body_ratio_threshold,
            atr_band_multiplier,
            atr_gap_multiplier,
        ));

        for kline in klines().iter().take(3) {
            let row = processor.process(kline).unwrap();
            assert!(row.atr.is_some());
            assert!(row.rssi.is_some());
            assert!(row.structure_power.is_some());
        }
    }

    #[test]
    fn aggregate_required_children_are_available_from_the_first_valid_candle() {
        let config = IndicatorConfig::default();
        let periods = indicator_periods(&config.periods).unwrap();
        let body_ratio_threshold = config.gap_zones.body_ratio_threshold.clamped();
        let atr_band_multiplier = config.gap_zones.atr_band_multiplier.clamped();
        let atr_gap_multiplier = config.gap_zones.atr_gap_multiplier.clamped();
        let mut processor = algotrap::ta::prelude::Processor::new(TelegramIndicators::new(
            periods,
            body_ratio_threshold,
            atr_band_multiplier,
            atr_gap_multiplier,
        ));
        let row = processor.process(&klines()[0]).unwrap();

        assert!(row.atr.is_some());
        assert!(row.rssi.is_some());
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
        let default_config = IndicatorConfig::default();
        let aggregate = TelegramIndicators::new(
            indicator_periods(&default_config.periods).unwrap(),
            default_config.gap_zones.body_ratio_threshold.clamped(),
            default_config.gap_zones.atr_band_multiplier.clamped(),
            default_config.gap_zones.atr_gap_multiplier.clamped(),
        );
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
        aggregate: TelegramIndicators,
        later: LaterChild,
    }

    struct FailureHarnessState {
        aggregate: TelegramIndicatorState,
        later: (),
    }

    impl FailureHarness {
        fn new() -> (Self, FailureSwitch) {
            let failure = std::rc::Rc::new(std::cell::Cell::new(false));
            let default_config = IndicatorConfig::default();
            (
                Self {
                    aggregate: TelegramIndicators::new(
                        indicator_periods(&default_config.periods).unwrap(),
                        default_config.gap_zones.body_ratio_threshold.clamped(),
                        default_config.gap_zones.atr_band_multiplier.clamped(),
                        default_config.gap_zones.atr_gap_multiplier.clamped(),
                    ),
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
        type Output = TelegramIndicatorRow;
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

    fn assert_rows_match(expected: &TelegramIndicatorRow, actual: &TelegramIndicatorRow) {
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
        assert_option_f64(expected.rssi, actual.rssi, "rssi");
        assert_option_f64(expected.rssi_ma, actual.rssi_ma, "rssi_ma");
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
        assert_option_f64(expected.sharpe, actual.sharpe, "sharpe");
        assert_option_f64(expected.body_ratio, actual.body_ratio, "body_ratio");
        assert_eq!(expected.is_atr_gap, actual.is_atr_gap);
        assert_eq!(
            expected.gap_candidate_qualifies,
            actual.gap_candidate_qualifies
        );
        assert_eq!(
            expected.gap_candidate_body_bottom,
            actual.gap_candidate_body_bottom
        );
        assert_eq!(
            expected.gap_candidate_body_top,
            actual.gap_candidate_body_top
        );
        assert_eq!(
            expected.gap_candidate_direction,
            actual.gap_candidate_direction
        );
    }

    fn assert_telegram_row_fixture(
        actual: &TelegramIndicatorRow,
        expected: TelegramIndicatorRow,
        index: &str,
    ) {
        assert_rows_match(&expected, actual);
        assert!(actual.atr.is_some(), "{index} ATR must be present");
        assert!(actual.rssi_ma.is_some(), "{index} RSI EMA must be present");
        assert!(
            actual.structure_power_sma.is_some(),
            "{index} structure SMA must be present"
        );
        assert!(actual.sharpe.is_some(), "{index} Sharpe must be present");
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

    async fn compute_default_frame(
        config: &IndicatorConfig,
    ) -> Box<dyn algotrap::engine::traits::ComputedFrame> {
        compute_telegram_frame(
            klines(),
            ValidatedTicker::new("BTCUSDT", 0.02, 0.01).unwrap(),
            config,
        )
        .await
        .unwrap()
        .0
    }

    fn assert_close(actual: f64, expected: f64, field: &str) {
        assert!(
            (actual - expected).abs() <= 1e-12,
            "{field}: {actual} != {expected}"
        );
    }

    fn assert_complete_row(row: &TelegramIndicatorRow) {
        let TelegramIndicatorRow {
            atr,
            volume_sma,
            ema200,
            bias_reversion,
            neutral_revrsi,
            bullish_revrsi,
            bearish_revrsi,
            atr_upperband,
            atr_lowerband,
            rssi,
            rssi_ma,
            structure_power,
            structure_power_sma,
            atr_percent,
            atr_reversion_percent,
            band_reversion,
            sharpe,
            body_ratio,
            is_atr_gap,
            gap_candidate_qualifies,
            gap_candidate_body_bottom,
            gap_candidate_body_top,
            gap_candidate_direction,
            ..
        } = row;
        for (name, value) in [
            ("atr", atr.unwrap()),
            ("volume_sma", volume_sma.unwrap()),
            ("ema200", ema200.unwrap()),
            ("bias_reversion", bias_reversion.unwrap()),
            ("neutral_revrsi", neutral_revrsi.unwrap()),
            ("bullish_revrsi", bullish_revrsi.unwrap()),
            ("bearish_revrsi", bearish_revrsi.unwrap()),
            ("atr_upperband", atr_upperband.unwrap()),
            ("atr_lowerband", atr_lowerband.unwrap()),
            ("rssi", rssi.unwrap()),
            ("rssi_ma", rssi_ma.unwrap()),
            ("structure_power", structure_power.unwrap()),
            ("structure_power_sma", structure_power_sma.unwrap()),
            ("atr_percent", atr_percent.unwrap()),
            ("atr_reversion_percent", atr_reversion_percent.unwrap()),
            ("band_reversion", band_reversion.unwrap()),
            ("sharpe", sharpe.unwrap()),
            ("body_ratio", body_ratio.unwrap()),
        ] {
            assert!(value.is_finite(), "{name} must be finite");
        }
        assert!(is_atr_gap.is_some());
        let _ = (
            gap_candidate_qualifies,
            gap_candidate_body_bottom,
            gap_candidate_body_top,
            gap_candidate_direction,
        );
    }

    fn sample_row() -> TelegramIndicatorRow {
        TelegramIndicatorRow {
            atr: Some(2.0),
            volume_sma: Some(1.0),
            ema200: Some(1.0),
            bias_reversion: Some(1.0),
            neutral_revrsi: Some(1.0),
            bullish_revrsi: Some(1.0),
            bearish_revrsi: Some(1.0),
            atr_upperband: Some(106.0),
            atr_lowerband: Some(94.0),
            rssi: Some(50.0),
            rssi_ma: Some(50.0),
            structure_power: Some(1.0),
            structure_power_sma: Some(1.0),
            atr_percent: Some(0.02),
            atr_reversion_percent: Some(1.0),
            band_reversion: Some(1.0),
            sharpe: Some(1.0),
            body_ratio: Some(0.5),
            is_atr_gap: Some(false),
            gap_candidate_qualifies: false,
            gap_candidate_body_bottom: None,
            gap_candidate_body_top: None,
            gap_candidate_direction: None,
        }
    }

    fn expected_columns(active_outputs: &[&str]) -> Vec<String> {
        BASE_COLUMNS
            .iter()
            .map(|column| (*column).to_string())
            .chain(
                ICHING_ENERGY_COLUMNS
                    .iter()
                    .map(|column| (*column).to_string()),
            )
            .chain(active_outputs.iter().map(|column| (*column).to_string()))
            .collect()
    }

    fn only_active_outputs(names: &[&str]) -> IndicatorConfig {
        let mut config = IndicatorConfig::default();
        config.outputs.atr.active = false;
        config.outputs.volume_sma.active = false;
        config.outputs.ema200.active = false;
        config.outputs.bias_reversion.active = false;
        config.outputs.neutral_revrsi.active = false;
        config.outputs.bullish_revrsi.active = false;
        config.outputs.bearish_revrsi.active = false;
        config.outputs.atr_upperband.active = false;
        config.outputs.atr_lowerband.active = false;
        config.outputs.rssi.active = false;
        config.outputs.rssi_ma.active = false;
        config.outputs.structure_power.active = false;
        config.outputs.structure_power_sma.active = false;
        config.outputs.atr_percent.active = false;
        config.outputs.atr_reversion_percent.active = false;
        config.outputs.band_reversion.active = false;
        config.outputs.sharpe.active = false;
        config.outputs.body_ratio.active = false;
        config.outputs.is_atr_gap.active = false;
        config.outputs.leverage.active = false;

        for name in names {
            match *name {
                "atr" => config.outputs.atr.active = true,
                "volume_sma" => config.outputs.volume_sma.active = true,
                "ema200" => config.outputs.ema200.active = true,
                "bias_reversion" => config.outputs.bias_reversion.active = true,
                "neutral_revrsi" => config.outputs.neutral_revrsi.active = true,
                "bullish_revrsi" => config.outputs.bullish_revrsi.active = true,
                "bearish_revrsi" => config.outputs.bearish_revrsi.active = true,
                "atr_upperband" => config.outputs.atr_upperband.active = true,
                "atr_lowerband" => config.outputs.atr_lowerband.active = true,
                "rssi" => config.outputs.rssi.active = true,
                "rssi_ma" => config.outputs.rssi_ma.active = true,
                "structure_power" => config.outputs.structure_power.active = true,
                "structure_power_sma" => config.outputs.structure_power_sma.active = true,
                "atr_percent" => config.outputs.atr_percent.active = true,
                "atr_reversion_percent" => config.outputs.atr_reversion_percent.active = true,
                "band_reversion" => config.outputs.band_reversion.active = true,
                "sharpe" => config.outputs.sharpe.active = true,
                "body_ratio" => config.outputs.body_ratio.active = true,
                "is_atr_gap" => config.outputs.is_atr_gap.active = true,
                "leverage" => config.outputs.leverage.active = true,
                other => panic!("unexpected output {other}"),
            }
        }

        config
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

    fn varied_klines() -> Vec<Kline> {
        (0..300)
            .map(|index| {
                let trend = 100.0 + index as f64 * 0.8;
                let wave = (index as f64 * 0.37).sin() * 6.0;
                let open = trend + wave;
                Kline {
                    open,
                    high: open + 1.0 + (index % 5) as f64 * 0.4,
                    low: open - 0.5 - (index % 3) as f64 * 0.3,
                    close: open + (index as f64 * 0.71).cos(),
                    volume: 1_000.0 + (index % 11) as f64 * 37.0,
                    time: 1_700_000_000_000 + index * 60_000,
                    adjclose: None,
                }
            })
            .collect()
    }
}

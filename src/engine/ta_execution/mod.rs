use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroUsize;

use crate::engine::execution_strategy::ExecutionStrategy;
use crate::model::kline::Kline;
use crate::ta::experimental::{band_reversion, band_reversion_percent};
use crate::ta::gap_zones::is_atr_gap_with_atr;
use crate::ta::indicator::{
    IndicatorColumn, IndicatorFrame, IndicatorProjection, IndicatorSettings, booleans, numbers,
    validate_indicator_output,
};
use crate::ta::ma::{ema, rma, sma};
use crate::ta::metric::sharpe;
use crate::ta::ohlc::Ohlc;
use crate::ta::plan::{
    IndicatorPlan, PlanCompiler, PlanOutput, SeriesExpr, SeriesNode, SourceField, standard_plan,
};
use crate::ta::rsi::{reverse_rsi, rsi};
use crate::ta::{TaError, TaResult};

#[derive(Debug, Clone, Copy)]
enum Backend {
    Sequential,
    IntraSeries(NonZeroUsize),
}

impl Backend {
    fn from_strategy(strategy: ExecutionStrategy) -> Self {
        match strategy.resolve() {
            ExecutionStrategy::Auto => unreachable!("Auto resolves to sequential"),
            ExecutionStrategy::Sequential => Self::Sequential,
            ExecutionStrategy::IntraSeries(instructions) => {
                Self::IntraSeries(instructions.workers())
            }
        }
    }
}

pub(crate) fn execute_standard_plan(
    klines: &[Kline],
    settings: IndicatorSettings,
    projection: &IndicatorProjection,
    strategy: ExecutionStrategy,
) -> TaResult<IndicatorFrame> {
    let compiled = standard_plan(settings)?.project(projection).compile()?;
    execute_compiled_plan(&compiled, klines, strategy)
}

pub(crate) fn execute_compiled_plan(
    compiled: &PlanCompiler,
    klines: &[Kline],
    strategy: ExecutionStrategy,
) -> TaResult<IndicatorFrame> {
    execute_plan(&compiled.plan, klines, Backend::from_strategy(strategy))
}

fn execute_plan(
    plan: &IndicatorPlan,
    klines: &[Kline],
    backend: Backend,
) -> TaResult<IndicatorFrame> {
    if klines.is_empty() {
        return Err(TaError::validation("Kline slice is empty"));
    }
    validate_klines(klines)?;
    let source = Sources::from_klines(klines);
    let ohlc = Ohlc::new(&source.open, &source.high, &source.low, &source.close)?;
    let mut memo = HashMap::new();
    let mut columns = BTreeMap::new();
    for output in plan.outputs.iter() {
        let name = output.name().output();
        match output {
            PlanOutput::Number { expr, .. } => {
                let result = evaluate(expr, &source, ohlc, &mut memo, backend)?;
                validate_indicator_output(name.column_name(), &result)?;
                columns.insert(name, IndicatorColumn::Number(numbers(result)));
            }
            PlanOutput::Boolean { expr, .. } => {
                let atr = evaluate(&expr.atr, &source, ohlc, &mut memo, backend)?;
                columns.insert(
                    name,
                    IndicatorColumn::Boolean(booleans(is_atr_gap_with_atr(ohlc, &atr)?)),
                );
            }
        }
    }
    Ok(IndicatorFrame::from_columns(klines.len(), columns))
}

fn validate_klines(klines: &[Kline]) -> TaResult<()> {
    let mut previous = None;
    for (index, row) in klines.iter().enumerate() {
        for (name, value) in [
            ("open", row.open),
            ("high", row.high),
            ("low", row.low),
            ("close", row.close),
            ("volume", row.volume),
        ] {
            if !value.is_finite() {
                return Err(TaError::validation(format!(
                    "kline {index} has non-finite {name}"
                )));
            }
        }
        if previous.is_some_and(|time| row.time <= time) {
            return Err(TaError::validation(
                "klines must be in strictly increasing chronological order",
            ));
        }
        previous = Some(row.time);
    }
    Ok(())
}

fn evaluate(
    expr: &SeriesExpr,
    source: &Sources,
    ohlc: Ohlc<'_>,
    memo: &mut HashMap<*const SeriesNode, Vec<f64>>,
    backend: Backend,
) -> TaResult<Vec<f64>> {
    if let Some(value) = memo.get(&expr.key()) {
        return Ok(value.clone());
    }
    let value = match expr.0.as_ref() {
        SeriesNode::Source(field) => source.field(*field).to_vec(),
        SeriesNode::Sma(input, period) => {
            sma(&evaluate(input, source, ohlc, memo, backend)?, *period)?
        }
        SeriesNode::Ema(input, period) => smooth(
            &evaluate(input, source, ohlc, memo, backend)?,
            *period,
            false,
            backend,
        )?,
        SeriesNode::Rma(input, period) => smooth(
            &evaluate(input, source, ohlc, memo, backend)?,
            *period,
            true,
            backend,
        )?,
        SeriesNode::Atr(period) => {
            let values = ohlc
                .true_range()
                .map_err(|_| TaError::non_finite_indicator_output(0, "atr"))?;
            smooth(&values, *period, true, backend)
                .map_err(|_| TaError::non_finite_indicator_output(0, "atr"))?
        }
        SeriesNode::Rsi(input, period) => compute_rsi(
            &evaluate(input, source, ohlc, memo, backend)?,
            *period,
            backend,
        )?,
        SeriesNode::ReverseRsi(input, period, target) => compute_reverse_rsi(
            &evaluate(input, source, ohlc, memo, backend)?,
            *period,
            *target,
            backend,
        )?,
        SeriesNode::BarBias => ohlc.bar_bias()?,
        SeriesNode::BodyRatio => ohlc.body_ratio()?,
        SeriesNode::Sharpe(input, period) => {
            sharpe(&evaluate(input, source, ohlc, memo, backend)?, *period)?
        }
        SeriesNode::Add(left, right) => zip(
            &evaluate(left, source, ohlc, memo, backend)?,
            &evaluate(right, source, ohlc, memo, backend)?,
            |a, b| a + b,
        ),
        SeriesNode::Sub(left, right) => zip(
            &evaluate(left, source, ohlc, memo, backend)?,
            &evaluate(right, source, ohlc, memo, backend)?,
            |a, b| a - b,
        ),
        SeriesNode::Scale(input, factor) => evaluate(input, source, ohlc, memo, backend)?
            .into_iter()
            .map(|value| value * factor)
            .collect(),
        SeriesNode::AtrPercent(atr, open) => zip(
            &evaluate(atr, source, ohlc, memo, backend)?,
            &evaluate(open, source, ohlc, memo, backend)?,
            |atr, open| if open == 0.0 { 0.0 } else { atr / open },
        ),
        SeriesNode::BandReversion {
            atr,
            signal,
            percent,
        } => {
            let oscillation = evaluate(atr, source, ohlc, memo, backend)?
                .into_iter()
                .map(|value| value * 1.618)
                .collect::<Vec<_>>();
            let signal = evaluate(signal, source, ohlc, memo, backend)?;
            if *percent {
                band_reversion_percent(ohlc, &oscillation, &signal)?
            } else {
                band_reversion(ohlc, &oscillation, &signal)?
            }
        }
    };
    memo.insert(expr.key(), value.clone());
    Ok(value)
}

fn smooth(values: &[f64], period: usize, wilder: bool, backend: Backend) -> TaResult<Vec<f64>> {
    match backend {
        Backend::Sequential => {
            if wilder {
                rma(values, period)
            } else {
                ema(values, period)
            }
        }
        Backend::IntraSeries(workers) => smooth_with_workers(
            values,
            if wilder {
                1.0 / period as f64
            } else {
                2.0 / (period as f64 + 1.0)
            },
            workers.get(),
        ),
    }
}

fn compute_rsi(values: &[f64], period: usize, backend: Backend) -> TaResult<Vec<f64>> {
    match backend {
        Backend::Sequential => rsi(values, period),
        Backend::IntraSeries(workers) => rsi_with_workers(values, period, workers.get()),
    }
}

fn compute_reverse_rsi(
    values: &[f64],
    period: usize,
    target: f64,
    backend: Backend,
) -> TaResult<Vec<f64>> {
    match backend {
        Backend::Sequential => reverse_rsi(values, period, target),
        Backend::IntraSeries(workers) => {
            reverse_rsi_with_workers(values, period, target, workers.get())
        }
    }
}

fn smooth_with_workers(values: &[f64], alpha: f64, workers: usize) -> TaResult<Vec<f64>> {
    if workers <= 1 || values.len() < 16 {
        return Ok(smooth_sequential(values, alpha));
    }
    let chunk_size = values.len().div_ceil(workers.min(values.len()));
    let chunks = values.chunks(chunk_size).collect::<Vec<_>>();
    let summaries = chunks
        .iter()
        .map(|chunk| block_summary(chunk, alpha))
        .collect::<Vec<_>>();
    let mut seeds = Vec::with_capacity(chunks.len());
    let mut seed = values[0];
    seeds.push(seed);
    for &(scale, offset) in summaries.iter().take(chunks.len() - 1) {
        seed = scale * seed + offset;
        seeds.push(seed);
    }
    let mut result = vec![0.0; values.len()];
    std::thread::scope(|scope| {
        for ((output, chunk), seed) in result
            .chunks_mut(chunk_size)
            .zip(chunks.iter().copied())
            .zip(seeds.iter().copied())
        {
            scope.spawn(move || fill_block(output, chunk, alpha, seed));
        }
    });
    result[0] = values[0];
    Ok(result)
}

fn rsi_with_workers(values: &[f64], period: usize, workers: usize) -> TaResult<Vec<f64>> {
    let (gains, losses) = changes(values);
    let gains = smooth_with_workers(&gains, 1.0 / period as f64, workers)?;
    let losses = smooth_with_workers(&losses, 1.0 / period as f64, workers)?;
    Ok(rsi_values(gains, losses))
}

fn reverse_rsi_with_workers(
    values: &[f64],
    period: usize,
    target: f64,
    workers: usize,
) -> TaResult<Vec<f64>> {
    if !(0.0 < target && target < 100.0) {
        return Err(TaError::validation(
            "reverse RSI target must be strictly between 0 and 100",
        ));
    }
    let (gains, losses) = changes(values);
    let gains = smooth_with_workers(&gains, 1.0 / period as f64, workers)?;
    let losses = smooth_with_workers(&losses, 1.0 / period as f64, workers)?;
    Ok(values
        .iter()
        .zip(gains)
        .zip(losses)
        .map(|((&source, gain), loss)| {
            let x = (period - 1) as f64 * (loss * target / (100.0 - target) - gain);
            if x >= 0.0 {
                source + x
            } else {
                source + x * (100.0 - target) / target
            }
        })
        .collect())
}

fn changes(values: &[f64]) -> (Vec<f64>, Vec<f64>) {
    values
        .iter()
        .enumerate()
        .map(|(index, &value)| {
            let difference = index
                .checked_sub(1)
                .map(|previous| value - values[previous])
                .unwrap_or(0.0);
            (difference.max(0.0), (-difference).max(0.0))
        })
        .unzip()
}
fn rsi_values(gains: Vec<f64>, losses: Vec<f64>) -> Vec<f64> {
    gains
        .into_iter()
        .zip(losses)
        .map(|(gain, loss)| {
            if loss == 0.0 && gain == 0.0 {
                50.0
            } else if loss == 0.0 {
                100.0
            } else {
                100.0 - 100.0 / (1.0 + gain / loss)
            }
        })
        .collect()
}
fn smooth_sequential(values: &[f64], alpha: f64) -> Vec<f64> {
    let mut result = Vec::with_capacity(values.len());
    let mut previous = 0.0;
    for (index, &value) in values.iter().enumerate() {
        let current = if index == 0 {
            value
        } else {
            (1.0 - alpha) * previous + alpha * value
        };
        result.push(current);
        previous = current;
    }
    result
}
fn block_summary(values: &[f64], alpha: f64) -> (f64, f64) {
    let beta = 1.0 - alpha;
    values.iter().fold((1.0, 0.0), |(scale, offset), &value| {
        (scale * beta, beta * offset + alpha * value)
    })
}
fn fill_block(output: &mut [f64], values: &[f64], alpha: f64, seed: f64) {
    let mut previous = seed;
    for (slot, &value) in output.iter_mut().zip(values) {
        previous = (1.0 - alpha) * previous + alpha * value;
        *slot = previous;
    }
}
fn zip(left: &[f64], right: &[f64], map: impl Fn(f64, f64) -> f64) -> Vec<f64> {
    left.iter()
        .zip(right)
        .map(|(&left, &right)| map(left, right))
        .collect()
}

struct Sources {
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    volume: Vec<f64>,
}
impl Sources {
    fn from_klines(rows: &[Kline]) -> Self {
        Self {
            open: rows.iter().map(|row| row.open).collect(),
            high: rows.iter().map(|row| row.high).collect(),
            low: rows.iter().map(|row| row.low).collect(),
            close: rows.iter().map(|row| row.close).collect(),
            volume: rows.iter().map(|row| row.volume).collect(),
        }
    }
    fn field(&self, field: SourceField) -> &[f64] {
        match field {
            SourceField::Open => &self.open,
            SourceField::High => &self.high,
            SourceField::Low => &self.low,
            SourceField::Close => &self.close,
            SourceField::Volume => &self.volume,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::execution_strategy::ExecutionInstructions;
    use crate::ta::indicator::IndicatorOutput;
    use crate::ta::plan::{OutputName, PlanOutput, SeriesExpr};

    fn klines() -> Vec<Kline> {
        (0..128)
            .map(|index| {
                let open = 100.0 + index as f64;
                Kline {
                    open,
                    high: open + 2.0,
                    low: open - 1.0,
                    close: open + (index % 3) as f64,
                    volume: 10.0 + index as f64,
                    time: index,
                    adjclose: None,
                }
            })
            .collect()
    }

    fn intra_series(workers: usize) -> ExecutionStrategy {
        ExecutionStrategy::IntraSeries(ExecutionInstructions::new(
            NonZeroUsize::new(workers).unwrap(),
        ))
    }

    fn assert_frame_close(actual: &IndicatorFrame, expected: &IndicatorFrame) {
        assert_eq!(actual.len(), expected.len());
        assert_eq!(
            actual.column_names().collect::<Vec<_>>(),
            expected.column_names().collect::<Vec<_>>()
        );
        for name in actual.column_names() {
            match (actual.column(name).unwrap(), expected.column(name).unwrap()) {
                (IndicatorColumn::Number(actual), IndicatorColumn::Number(expected)) => {
                    for (&actual, &expected) in actual.iter().zip(expected) {
                        assert!((actual.unwrap() - expected.unwrap()).abs() <= 1e-12);
                    }
                }
                (IndicatorColumn::Boolean(actual), IndicatorColumn::Boolean(expected)) => {
                    assert_eq!(actual, expected)
                }
                _ => panic!("column type changed: {name}"),
            }
        }
    }

    #[test]
    fn explicit_backend_preserves_standard_projection_and_schema() {
        let projection = IndicatorProjection::selected([
            IndicatorOutput::Atr,
            IndicatorOutput::RssiMa,
            IndicatorOutput::IsAtrGap,
        ]);
        let sequential = execute_standard_plan(
            &klines(),
            IndicatorSettings::default(),
            &projection,
            ExecutionStrategy::Sequential,
        )
        .unwrap();
        for workers in [1, 2, 4, 8] {
            let actual = execute_standard_plan(
                &klines(),
                IndicatorSettings::default(),
                &projection,
                intra_series(workers),
            )
            .unwrap();
            assert_frame_close(&actual, &sequential);
            assert_eq!(
                actual.column_names().collect::<Vec<_>>(),
                vec!["atr", "is_atr_gap", "rssi_ma"]
            );
        }
    }

    #[test]
    fn explicit_backend_evaluates_one_compiled_custom_plan() {
        let shared = SeriesExpr::close().ema(3);
        let compiled = IndicatorPlan::builder()
            .output(PlanOutput::number(
                OutputName::standard(IndicatorOutput::Ema200),
                shared.clone().scale(2.0),
            ))
            .output(PlanOutput::number(
                OutputName::standard(IndicatorOutput::VolumeSma),
                shared.add(SeriesExpr::open()),
            ))
            .build()
            .unwrap()
            .compile()
            .unwrap();
        let sequential =
            execute_compiled_plan(&compiled, &klines(), ExecutionStrategy::Sequential).unwrap();
        for workers in [1, 2, 4, 8] {
            let actual =
                execute_compiled_plan(&compiled, &klines(), intra_series(workers)).unwrap();
            assert_frame_close(&actual, &sequential);
        }
    }

    #[test]
    fn ta_sources_contain_no_engine_execution_terms() {
        for source in [
            include_str!("../../ta/ma.rs"),
            include_str!("../../ta/rsi.rs"),
            include_str!("../../ta/experimental.rs"),
            include_str!("../../ta/ohlc.rs"),
            include_str!("../../ta/plan.rs"),
        ] {
            for forbidden in [
                "worker",
                "thread",
                "parallel",
                "ExecutionStrategy",
                "ExecutorBackend",
                "IntraSeries",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "TA source contains {forbidden}"
                );
            }
        }
    }
}

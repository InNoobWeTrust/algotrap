//! Typed, lazy technical-analysis plans and their sequential evaluator.
//!
//! Plans describe only the closed indicator domain.  They never execute while
//! being built, and contain no scheduling or data-engine concerns.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::model::kline::Kline;

use super::experimental::{band_reversion, band_reversion_percent};
use super::gap_zones::is_atr_gap_with_atr;
use super::indicator::{
    IndicatorColumn, IndicatorFrame, IndicatorOutput, IndicatorProjection, IndicatorSettings,
    booleans, numbers, validate_indicator_output,
};
use super::ma::{ema, rma, sma};
use super::metric::sharpe;
use super::ohlc::Ohlc;
use super::rsi::{reverse_rsi, rsi};
use super::{TaError, TaResult};

/// A chronological scalar field available from a Kline source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceField {
    Open,
    High,
    Low,
    Close,
    Volume,
}

/// A stable, typed public output alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutputName(pub(crate) IndicatorOutput);

impl OutputName {
    /// Creates the standard alias associated with an indicator output.
    pub const fn standard(output: IndicatorOutput) -> Self {
        Self(output)
    }
    /// Returns the stable materialized column name.
    pub fn as_str(self) -> &'static str {
        self.0.column_name()
    }
    pub(crate) const fn output(self) -> IndicatorOutput {
        self.0
    }
}

#[derive(Debug, Clone)]
pub(crate) enum SeriesNode {
    Source(SourceField),
    Sma(SeriesExpr, usize),
    Ema(SeriesExpr, usize),
    Rma(SeriesExpr, usize),
    Atr(usize),
    Rsi(SeriesExpr, usize),
    ReverseRsi(SeriesExpr, usize, f64),
    BarBias,
    BodyRatio,
    Sharpe(SeriesExpr, usize),
    Add(SeriesExpr, SeriesExpr),
    Sub(SeriesExpr, SeriesExpr),
    Scale(SeriesExpr, f64),
    AtrPercent(SeriesExpr, SeriesExpr),
    BandReversion {
        atr: SeriesExpr,
        signal: SeriesExpr,
        percent: bool,
    },
}

/// A lazy numeric series in the closed TA expression language.
#[derive(Debug, Clone)]
pub struct SeriesExpr(pub(crate) Arc<SeriesNode>);

impl SeriesExpr {
    /// Returns the source's open series.
    pub fn open() -> Self {
        Self::source(SourceField::Open)
    }
    /// Returns the source's high series.
    pub fn high() -> Self {
        Self::source(SourceField::High)
    }
    /// Returns the source's low series.
    pub fn low() -> Self {
        Self::source(SourceField::Low)
    }
    /// Returns the source's close series.
    pub fn close() -> Self {
        Self::source(SourceField::Close)
    }
    /// Returns the source's volume series.
    pub fn volume() -> Self {
        Self::source(SourceField::Volume)
    }
    /// Returns a simple moving-average expression.
    pub fn sma(self, period: usize) -> Self {
        Self(Arc::new(SeriesNode::Sma(self, period)))
    }
    /// Returns an exponential moving-average expression.
    pub fn ema(self, period: usize) -> Self {
        Self(Arc::new(SeriesNode::Ema(self, period)))
    }
    /// Returns a Wilder moving-average expression.
    pub fn rma(self, period: usize) -> Self {
        Self(Arc::new(SeriesNode::Rma(self, period)))
    }
    /// Returns an RSI expression.
    pub fn rsi(self, period: usize) -> Self {
        Self(Arc::new(SeriesNode::Rsi(self, period)))
    }
    /// Returns a reverse-RSI expression for a finite target strictly inside 0..100.
    pub fn reverse_rsi(self, period: usize, target: f64) -> Self {
        Self(Arc::new(SeriesNode::ReverseRsi(self, period, target)))
    }
    /// Returns the elementwise sum of two expressions.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, rhs: Self) -> Self {
        Self(Arc::new(SeriesNode::Add(self, rhs)))
    }
    /// Returns the elementwise difference of two expressions.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, rhs: Self) -> Self {
        Self(Arc::new(SeriesNode::Sub(self, rhs)))
    }
    /// Returns an elementwise scalar multiple.
    pub fn scale(self, factor: f64) -> Self {
        Self(Arc::new(SeriesNode::Scale(self, factor)))
    }
    fn source(field: SourceField) -> Self {
        Self(Arc::new(SeriesNode::Source(field)))
    }
    pub(crate) fn key(&self) -> *const SeriesNode {
        Arc::as_ptr(&self.0)
    }
}

/// Alias for an indicator expression, emphasizing public plan intent.
pub type IndicatorExpr = SeriesExpr;

/// A lazy boolean expression in the closed TA expression language.
#[derive(Debug, Clone)]
pub struct BooleanExpr {
    pub(crate) atr: SeriesExpr,
}

/// A typed, lazy public plan output.
#[derive(Debug, Clone)]
pub enum PlanOutput {
    Number {
        name: OutputName,
        expr: IndicatorExpr,
    },
    Boolean {
        name: OutputName,
        expr: BooleanExpr,
    },
}

impl PlanOutput {
    /// Assigns a stable standard alias to a numeric expression.
    pub fn number(name: OutputName, expr: IndicatorExpr) -> Self {
        Self::Number { name, expr }
    }
    /// Assigns a stable standard alias to a boolean expression.
    pub fn boolean(name: OutputName, expr: BooleanExpr) -> Self {
        Self::Boolean { name, expr }
    }
    pub(crate) fn name(&self) -> OutputName {
        match self {
            Self::Number { name, .. } | Self::Boolean { name, .. } => *name,
        }
    }
}

/// Immutable, reusable collection of lazy TA outputs.
#[derive(Debug, Clone, Default)]
pub struct IndicatorPlan {
    pub(crate) outputs: Arc<Vec<PlanOutput>>,
}

/// Builder for immutable plans. Building only allocates expression nodes.
#[derive(Debug, Default)]
pub struct IndicatorPlanBuilder {
    outputs: Vec<PlanOutput>,
}

impl IndicatorPlan {
    /// Starts an empty indicator plan.
    pub fn builder() -> IndicatorPlanBuilder {
        IndicatorPlanBuilder::default()
    }
    /// Combines two plans without calculating either one.
    pub fn union(&self, other: &Self) -> TaResult<Self> {
        Self::from_outputs(
            self.outputs
                .iter()
                .cloned()
                .chain(other.outputs.iter().cloned())
                .collect(),
        )
    }
    /// Adds one lazy output without calculating the plan.
    pub fn extend(&self, output: PlanOutput) -> TaResult<Self> {
        Self::from_outputs(
            self.outputs
                .iter()
                .cloned()
                .chain(std::iter::once(output))
                .collect(),
        )
    }
    /// Retains only requested public aliases.
    pub fn project(&self, projection: &IndicatorProjection) -> Self {
        Self {
            outputs: Arc::new(
                self.outputs
                    .iter()
                    .filter(|output| projection.contains(output.name().output()))
                    .cloned()
                    .collect(),
            ),
        }
    }
    /// Validates all aliases, source references, periods, scalars, and graph shape.
    pub fn compile(&self) -> TaResult<PlanCompiler> {
        PlanCompiler::new(self.clone())
    }
    fn from_outputs(outputs: Vec<PlanOutput>) -> TaResult<Self> {
        let plan = Self {
            outputs: Arc::new(outputs),
        };
        plan.compile()?;
        Ok(plan)
    }
}

impl IndicatorPlanBuilder {
    /// Adds a lazy typed output.
    pub fn output(mut self, output: PlanOutput) -> Self {
        self.outputs.push(output);
        self
    }
    /// Finishes and validates an immutable plan without calculating it.
    pub fn build(self) -> TaResult<IndicatorPlan> {
        IndicatorPlan::from_outputs(self.outputs)
    }
}

/// Returns a lazy ATR expression.
pub fn atr(period: usize) -> IndicatorExpr {
    SeriesExpr(Arc::new(SeriesNode::Atr(period)))
}
/// Returns a lazy bar-bias expression.
pub fn bar_bias() -> IndicatorExpr {
    SeriesExpr(Arc::new(SeriesNode::BarBias))
}
/// Returns a lazy body-ratio expression.
pub fn body_ratio() -> IndicatorExpr {
    SeriesExpr(Arc::new(SeriesNode::BodyRatio))
}
/// Returns a lazy Sharpe expression.
pub fn sharpe_expr(period: usize) -> IndicatorExpr {
    SeriesExpr::close().pipe_sharpe(period)
}
/// Returns lazy ATR gap flags from an ATR expression.
pub fn is_atr_gap(atr: IndicatorExpr) -> BooleanExpr {
    BooleanExpr { atr }
}

impl SeriesExpr {
    fn pipe_sharpe(self, period: usize) -> Self {
        Self(Arc::new(SeriesNode::Sharpe(self, period)))
    }
    pub fn atr_percent(self, open: Self) -> Self {
        Self(Arc::new(SeriesNode::AtrPercent(self, open)))
    }
    pub fn band_reversion(self, signal: Self) -> Self {
        Self(Arc::new(SeriesNode::BandReversion {
            atr: self,
            signal,
            percent: false,
        }))
    }
    pub fn band_reversion_percent(self, signal: Self) -> Self {
        Self(Arc::new(SeriesNode::BandReversion {
            atr: self,
            signal,
            percent: true,
        }))
    }
}

/// Validated plan ready for the sequential executor.
#[derive(Debug, Clone)]
pub struct PlanCompiler {
    pub(crate) plan: IndicatorPlan,
}

impl PlanCompiler {
    /// Validates and compiles a plan without accessing source rows.
    pub fn new(plan: IndicatorPlan) -> TaResult<Self> {
        validate_plan(&plan)?;
        Ok(Self { plan })
    }
    /// Evaluates this compiled plan sequentially against chronological Klines.
    pub fn execute(&self, klines: &[Kline]) -> TaResult<IndicatorFrame> {
        PlanExecutor::new().execute(&self.plan, klines)
    }
}

/// Sequential evaluator for compiled plans. It memoizes shared expression nodes.
#[derive(Debug)]
pub struct PlanExecutor {
    #[cfg(test)]
    evaluations: std::cell::Cell<usize>,
}

/// Normalizes arithmetic-overflow failures from the ATR chain into the stable
/// non-finite-output contract while preserving structural errors (invalid
/// period, alignment, validation) verbatim.
fn normalize_atr_overflow(error: TaError) -> TaError {
    if error.kind == crate::ta::TaErrorKind::Computation {
        TaError::non_finite_indicator_output(0, "atr")
    } else {
        error
    }
}

impl PlanExecutor {
    /// Creates a sequential executor.
    pub fn new() -> Self {
        Self {
            #[cfg(test)]
            evaluations: std::cell::Cell::new(0),
        }
    }
    /// Evaluates only the plan's projected transitive closure.
    pub fn execute(&self, plan: &IndicatorPlan, klines: &[Kline]) -> TaResult<IndicatorFrame> {
        self.execute_inner(plan, klines)
    }
    fn execute_inner(&self, plan: &IndicatorPlan, klines: &[Kline]) -> TaResult<IndicatorFrame> {
        if klines.is_empty() {
            return Err(TaError::validation("Kline slice is empty"));
        }
        validate_klines(klines)?;
        plan.compile()
            .map_err(|error| TaError::validation(error.to_string()))?;
        let source = Sources::from_klines(klines);
        let ohlc = Ohlc::new(&source.open, &source.high, &source.low, &source.close)?;
        let mut values = HashMap::new();
        let mut columns = BTreeMap::new();
        for output in plan.outputs.iter() {
            let name = output.name().output();
            match output {
                PlanOutput::Number { expr, .. } => {
                    let result = self.evaluate(expr, &source, ohlc, &mut values)?;
                    validate_indicator_output(name.column_name(), &result)?;
                    columns.insert(name, IndicatorColumn::Number(numbers(result)));
                }
                PlanOutput::Boolean { expr, .. } => {
                    let atr = self.evaluate(&expr.atr, &source, ohlc, &mut values)?;
                    columns.insert(
                        name,
                        IndicatorColumn::Boolean(booleans(is_atr_gap_with_atr(ohlc, &atr)?)),
                    );
                }
            }
        }
        Ok(IndicatorFrame::from_columns(klines.len(), columns))
    }
    fn evaluate(
        &self,
        expr: &SeriesExpr,
        source: &Sources,
        ohlc: Ohlc<'_>,
        memo: &mut HashMap<*const SeriesNode, Vec<f64>>,
    ) -> TaResult<Vec<f64>> {
        if let Some(value) = memo.get(&expr.key()) {
            return Ok(value.clone());
        }
        #[cfg(test)]
        self.evaluations.set(self.evaluations.get() + 1);
        let value = match expr.0.as_ref() {
            SeriesNode::Source(field) => source.field(*field).to_vec(),
            SeriesNode::Sma(input, p) => sma(&self.evaluate(input, source, ohlc, memo)?, *p)?,
            SeriesNode::Ema(input, p) => ema(&self.evaluate(input, source, ohlc, memo)?, *p)?,
            SeriesNode::Rma(input, p) => rma(&self.evaluate(input, source, ohlc, memo)?, *p)?,
            SeriesNode::Atr(p) => {
                let true_range = ohlc.true_range().map_err(normalize_atr_overflow)?;
                rma(&true_range, *p).map_err(normalize_atr_overflow)?
            }
            SeriesNode::Rsi(input, p) => rsi(&self.evaluate(input, source, ohlc, memo)?, *p)?,
            SeriesNode::ReverseRsi(input, p, target) => {
                reverse_rsi(&self.evaluate(input, source, ohlc, memo)?, *p, *target)?
            }
            SeriesNode::BarBias => ohlc.bar_bias()?,
            SeriesNode::BodyRatio => ohlc.body_ratio()?,
            SeriesNode::Sharpe(input, p) => sharpe(&self.evaluate(input, source, ohlc, memo)?, *p)?,
            SeriesNode::Add(left, right) => zip(
                &self.evaluate(left, source, ohlc, memo)?,
                &self.evaluate(right, source, ohlc, memo)?,
                |a, b| a + b,
            ),
            SeriesNode::Sub(left, right) => zip(
                &self.evaluate(left, source, ohlc, memo)?,
                &self.evaluate(right, source, ohlc, memo)?,
                |a, b| a - b,
            ),
            SeriesNode::Scale(input, factor) => self
                .evaluate(input, source, ohlc, memo)?
                .into_iter()
                .map(|v| v * factor)
                .collect(),
            SeriesNode::AtrPercent(atr, open) => zip(
                &self.evaluate(atr, source, ohlc, memo)?,
                &self.evaluate(open, source, ohlc, memo)?,
                |a, o| if o == 0.0 { 0.0 } else { a / o },
            ),
            SeriesNode::BandReversion {
                atr,
                signal,
                percent,
            } => {
                let oscillation = self
                    .evaluate(atr, source, ohlc, memo)?
                    .into_iter()
                    .map(|v| v * 1.618)
                    .collect::<Vec<_>>();
                let signal = self.evaluate(signal, source, ohlc, memo)?;
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
    #[cfg(test)]
    fn evaluation_count(&self) -> usize {
        self.evaluations.get()
    }
}

impl Default for PlanExecutor {
    fn default() -> Self {
        Self::new()
    }
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
            open: rows.iter().map(|r| r.open).collect(),
            high: rows.iter().map(|r| r.high).collect(),
            low: rows.iter().map(|r| r.low).collect(),
            close: rows.iter().map(|r| r.close).collect(),
            volume: rows.iter().map(|r| r.volume).collect(),
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
fn zip(left: &[f64], right: &[f64], map: impl Fn(f64, f64) -> f64) -> Vec<f64> {
    left.iter().zip(right).map(|(&a, &b)| map(a, b)).collect()
}
fn validate_plan(plan: &IndicatorPlan) -> TaResult<()> {
    let mut names = BTreeSet::new();
    for output in plan.outputs.iter() {
        if !names.insert(output.name()) {
            return Err(TaError::invalid_plan(format!(
                "duplicate plan output {}",
                output.name().as_str()
            )));
        }
        match output {
            PlanOutput::Number { expr, .. } => validate_expr(expr)?,
            PlanOutput::Boolean { expr, .. } => validate_expr(&expr.atr)?,
        }
    }
    Ok(())
}
fn validate_expr(expr: &SeriesExpr) -> TaResult<()> {
    match expr.0.as_ref() {
        SeriesNode::Source(_) | SeriesNode::BarBias | SeriesNode::BodyRatio => Ok(()),
        SeriesNode::Atr(p) => period("ATR", *p),
        SeriesNode::Sma(x, p) => {
            period("SMA", *p)?;
            validate_expr(x)
        }
        SeriesNode::Ema(x, p) => {
            period("EMA", *p)?;
            validate_expr(x)
        }
        SeriesNode::Rma(x, p) => {
            period("RMA", *p)?;
            validate_expr(x)
        }
        SeriesNode::Rsi(x, p) => {
            period("RSI", *p)?;
            validate_expr(x)
        }
        SeriesNode::Sharpe(x, p) => {
            period("Sharpe", *p)?;
            validate_expr(x)
        }
        SeriesNode::ReverseRsi(x, p, target) => {
            period("reverse RSI", *p)?;
            if !target.is_finite() || !(0.0 < *target && *target < 100.0) {
                return Err(TaError::invalid_plan(
                    "reverse RSI target must be finite and strictly between 0 and 100",
                ));
            }
            validate_expr(x)
        }
        SeriesNode::Add(a, b) | SeriesNode::Sub(a, b) | SeriesNode::AtrPercent(a, b) => {
            validate_expr(a)?;
            validate_expr(b)
        }
        SeriesNode::Scale(x, factor) => {
            if !factor.is_finite() {
                return Err(TaError::invalid_plan("scale must be finite"));
            }
            validate_expr(x)
        }
        SeriesNode::BandReversion { atr, signal, .. } => {
            validate_expr(atr)?;
            validate_expr(signal)
        }
    }
}
fn period(operator: &'static str, period: usize) -> TaResult<()> {
    if period == 0 {
        Err(TaError::invalid_period(format!(
            "{operator} period must be greater than zero (got {period})"
        )))
    } else {
        Ok(())
    }
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

/// Builds the complete standard indicator plan in `IndicatorOutput::all` order.
pub fn standard_plan(settings: IndicatorSettings) -> TaResult<IndicatorPlan> {
    let open = SeriesExpr::open();
    let high = SeriesExpr::high();
    let low = SeriesExpr::low();
    let close = SeriesExpr::close();
    let bias = bar_bias();
    let atr_expr = atr(settings.atr_period);
    let bias_reversion = open
        .clone()
        .sub(bias.clone().rma(settings.bias_period))
        .sma(settings.bias_period);
    let rssi = open.clone().add(bias.clone()).rsi(settings.rsi_period);
    let structure = bias.clone().rma(settings.structure_period);
    let bands = atr_expr.clone().band_reversion(bias_reversion.clone());
    let bands_percent = atr_expr
        .clone()
        .band_reversion_percent(bias_reversion.clone());
    IndicatorPlan::builder()
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::Atr),
            atr_expr.clone(),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::VolumeSma),
            SeriesExpr::volume().ema(settings.volume_ema_period),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::Ema200),
            close.clone().ema(settings.ema_period),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::BiasReversion),
            bias_reversion.clone(),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::NeutralReverseRsi),
            open.clone()
                .add(bias.clone())
                .reverse_rsi(settings.reverse_rsi_period, 50.0),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::BullishReverseRsi),
            high.reverse_rsi(settings.reverse_rsi_period, 70.0),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::BearishReverseRsi),
            low.reverse_rsi(settings.reverse_rsi_period, 30.0),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::AtrUpperBand),
            open.clone().add(atr_expr.clone().scale(1.618)),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::AtrLowerBand),
            open.clone().sub(atr_expr.clone().scale(1.618)),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::Rssi),
            rssi.clone(),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::RssiMa),
            rssi.ema(settings.rsi_smooth_period),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::StructurePower),
            structure.clone(),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::StructurePowerSma),
            structure.sma(settings.structure_sma_period),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::AtrPercent),
            atr_expr.clone().atr_percent(open.clone()),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::AtrReversionPercent),
            bands_percent,
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::BandReversion),
            bands,
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::Sharpe),
            sharpe_expr(settings.sharpe_period),
        ))
        .output(PlanOutput::number(
            OutputName::standard(IndicatorOutput::BodyRatio),
            body_ratio(),
        ))
        .output(PlanOutput::boolean(
            OutputName::standard(IndicatorOutput::IsAtrGap),
            is_atr_gap(atr_expr),
        ))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn klines() -> Vec<Kline> {
        (0..32)
            .map(|index| {
                let open = 100.0 + index as f64;
                Kline {
                    open,
                    high: open + 2.0,
                    low: open - 1.0,
                    close: open + (index % 3) as f64,
                    volume: 10.0 + index as f64,
                    time: index as i64,
                    adjclose: None,
                }
            })
            .collect()
    }

    #[test]
    fn construction_is_lazy_and_shared_nodes_are_memoized() {
        let shared = SeriesExpr::close().ema(3);
        let plan = IndicatorPlan::builder()
            .output(PlanOutput::number(
                OutputName::standard(IndicatorOutput::Ema200),
                shared.clone(),
            ))
            .output(PlanOutput::number(
                OutputName::standard(IndicatorOutput::VolumeSma),
                shared,
            ))
            .build()
            .unwrap();
        let executor = PlanExecutor::new();
        assert_eq!(executor.evaluation_count(), 0);
        executor.execute(&plan, &klines()).unwrap();
        assert_eq!(executor.evaluation_count(), 2);
    }

    #[test]
    fn union_extend_and_projection_reuse_lazy_outputs() {
        let atr = atr(3);
        let base = IndicatorPlan::builder()
            .output(PlanOutput::number(
                OutputName::standard(IndicatorOutput::Atr),
                atr.clone(),
            ))
            .build()
            .unwrap();
        let bands = IndicatorPlan::builder()
            .output(PlanOutput::number(
                OutputName::standard(IndicatorOutput::AtrUpperBand),
                SeriesExpr::open().add(atr.scale(1.618)),
            ))
            .build()
            .unwrap();
        let projection = IndicatorProjection::selected([IndicatorOutput::AtrUpperBand]);
        let frame = base
            .union(&bands)
            .unwrap()
            .project(&projection)
            .compile()
            .unwrap()
            .execute(&klines())
            .unwrap();
        assert!(frame.column("atr").is_none());
        assert!(frame.column("atr_upperband").is_some());
    }

    #[test]
    fn duplicate_aliases_and_invalid_periods_fail_before_evaluation() {
        let duplicate = IndicatorPlan::builder()
            .output(PlanOutput::number(
                OutputName::standard(IndicatorOutput::Atr),
                atr(2),
            ))
            .output(PlanOutput::number(
                OutputName::standard(IndicatorOutput::Atr),
                atr(3),
            ))
            .build();
        assert!(
            matches!(duplicate, Err(error) if error.kind == crate::ta::TaErrorKind::InvalidPlan)
        );
        let invalid = IndicatorPlan::builder()
            .output(PlanOutput::number(
                OutputName::standard(IndicatorOutput::Ema200),
                SeriesExpr::close().ema(0),
            ))
            .build();
        assert!(
            matches!(invalid, Err(error) if error.kind == crate::ta::TaErrorKind::InvalidPeriod)
        );
    }

    #[test]
    fn standard_complete_and_projection_match_legacy_sequential_frame() {
        let rows = klines();
        let settings = IndicatorSettings {
            ema_period: 3,
            volume_ema_period: 3,
            rsi_period: 3,
            rsi_smooth_period: 2,
            reverse_rsi_period: 3,
            atr_period: 3,
            bias_period: 3,
            structure_period: 3,
            structure_sma_period: 2,
            sharpe_period: 3,
        };
        let expected = IndicatorFrame::compute(&rows, settings).unwrap();
        let actual = standard_plan(settings)
            .unwrap()
            .compile()
            .unwrap()
            .execute(&rows)
            .unwrap();
        assert_eq!(actual, expected);
        let projection = IndicatorProjection::selected([
            IndicatorOutput::Atr,
            IndicatorOutput::RssiMa,
            IndicatorOutput::IsAtrGap,
        ]);
        let projected = standard_plan(settings)
            .unwrap()
            .project(&projection)
            .compile()
            .unwrap()
            .execute(&rows)
            .unwrap();
        for output in projection.outputs() {
            assert_eq!(
                projected.column(output.column_name()),
                expected.column(output.column_name())
            );
        }
        assert_eq!(projected.column_names().count(), 3);
    }
}

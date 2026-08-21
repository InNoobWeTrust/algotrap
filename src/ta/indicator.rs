//! Pure, lazy, composable technical-indicator plans.
//!
//! [`IndicatorPlan`] describes and validates ordered indicator dependencies
//! without evaluating them. Materializing a plan over one chronological candle
//! series produces an [`IndicatorFrame`] with the requested indicator columns.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::kline::Kline;

use super::{TaError, TaResult};

/// A consumer-visible output produced by the pure technical-analysis kernels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IndicatorOutput {
    Atr,
    VolumeSma,
    Ema200,
    BiasReversion,
    NeutralReverseRsi,
    BullishReverseRsi,
    BearishReverseRsi,
    AtrUpperBand,
    AtrLowerBand,
    Rssi,
    RssiMa,
    StructurePower,
    StructurePowerSma,
    AtrPercent,
    AtrReversionPercent,
    BandReversion,
    Sharpe,
    BodyRatio,
    IsAtrGap,
}

impl IndicatorOutput {
    const ALL: [Self; 19] = [
        Self::Atr,
        Self::VolumeSma,
        Self::Ema200,
        Self::BiasReversion,
        Self::NeutralReverseRsi,
        Self::BullishReverseRsi,
        Self::BearishReverseRsi,
        Self::AtrUpperBand,
        Self::AtrLowerBand,
        Self::Rssi,
        Self::RssiMa,
        Self::StructurePower,
        Self::StructurePowerSma,
        Self::AtrPercent,
        Self::AtrReversionPercent,
        Self::BandReversion,
        Self::Sharpe,
        Self::BodyRatio,
        Self::IsAtrGap,
    ];

    /// Returns every consumer-visible output in its complete-frame declaration order.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter()
    }

    /// Returns the stable materialized-column name for this output.
    pub fn column_name(self) -> &'static str {
        match self {
            Self::Atr => "atr",
            Self::VolumeSma => "volume_sma",
            Self::Ema200 => "ema200",
            Self::BiasReversion => "bias_reversion",
            Self::NeutralReverseRsi => "neutral_revrsi",
            Self::BullishReverseRsi => "bullish_revrsi",
            Self::BearishReverseRsi => "bearish_revrsi",
            Self::AtrUpperBand => "atr_upperband",
            Self::AtrLowerBand => "atr_lowerband",
            Self::Rssi => "rssi",
            Self::RssiMa => "rssi_ma",
            Self::StructurePower => "structure_power",
            Self::StructurePowerSma => "structure_power_sma",
            Self::AtrPercent => "atr_percent",
            Self::AtrReversionPercent => "atr_reversion_percent",
            Self::BandReversion => "band_reversion",
            Self::Sharpe => "sharpe",
            Self::BodyRatio => "body_ratio",
            Self::IsAtrGap => "is_atr_gap",
        }
    }
}

/// Typed request for all or a selected set of consumer-visible indicator outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndicatorProjection {
    Complete,
    Selected(BTreeSet<IndicatorOutput>),
}

impl IndicatorProjection {
    /// Creates a deduplicated projection of only the supplied outputs.
    pub fn selected(outputs: impl IntoIterator<Item = IndicatorOutput>) -> Self {
        Self::Selected(outputs.into_iter().collect())
    }

    /// Returns whether this projection requests an output.
    pub fn contains(&self, output: IndicatorOutput) -> bool {
        matches!(self, Self::Complete)
            || matches!(self, Self::Selected(outputs) if outputs.contains(&output))
    }

    /// Returns the requested outputs, with a complete request expanded deterministically.
    pub fn outputs(&self) -> Box<dyn Iterator<Item = IndicatorOutput> + '_> {
        match self {
            Self::Complete => Box::new(IndicatorOutput::all()),
            Self::Selected(outputs) => Box::new(outputs.iter().copied()),
        }
    }
}

/// Configuration for the ordered indicator kernels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndicatorSettings {
    pub volume_ema_period: usize,
    pub ema_period: usize,
    pub rsi_period: usize,
    pub rsi_smooth_period: usize,
    pub reverse_rsi_period: usize,
    pub atr_period: usize,
    pub bias_period: usize,
    pub structure_period: usize,
    pub structure_sma_period: usize,
    pub sharpe_period: usize,
}

impl Default for IndicatorSettings {
    fn default() -> Self {
        Self {
            volume_ema_period: 20,
            ema_period: 200,
            rsi_period: 14,
            rsi_smooth_period: 9,
            reverse_rsi_period: 14,
            atr_period: 42,
            bias_period: 9,
            structure_period: 9,
            structure_sma_period: 16,
            sharpe_period: 200,
        }
    }
}

impl IndicatorSettings {
    /// Validates that every smoothing/lookback period can be executed.
    pub fn validate(self) -> TaResult<Self> {
        for (name, value) in [
            ("volume_ema_period", self.volume_ema_period),
            ("ema_period", self.ema_period),
            ("rsi_period", self.rsi_period),
            ("rsi_smooth_period", self.rsi_smooth_period),
            ("reverse_rsi_period", self.reverse_rsi_period),
            ("atr_period", self.atr_period),
            ("bias_period", self.bias_period),
            ("structure_period", self.structure_period),
            ("structure_sma_period", self.structure_sma_period),
            ("sharpe_period", self.sharpe_period),
        ] {
            if value == 0 {
                return Err(TaError::invalid_period(format!(
                    "{name} must be greater than zero"
                )));
            }
        }
        Ok(self)
    }
}

/// A nullable numeric or boolean output column from an indicator kernel.
#[derive(Debug, Clone, PartialEq)]
pub enum IndicatorColumn {
    Number(Vec<Option<f64>>),
    Boolean(Vec<Option<bool>>),
}

impl IndicatorColumn {
    #[allow(dead_code)]
    fn len(&self) -> usize {
        match self {
            Self::Number(values) => values.len(),
            Self::Boolean(values) => values.len(),
        }
    }
}

/// A columnar indicator result whose rows remain in chronological order.
#[derive(Debug, Clone, PartialEq)]
pub struct IndicatorFrame {
    len: usize,
    columns: BTreeMap<IndicatorOutput, IndicatorColumn>,
}

impl IndicatorFrame {
    pub(crate) fn from_columns(
        len: usize,
        columns: BTreeMap<IndicatorOutput, IndicatorColumn>,
    ) -> Self {
        Self { len, columns }
    }
    /// Computes the complete shared cryptobot/Telegram numeric indicator set.
    pub fn compute(klines: &[Kline], settings: IndicatorSettings) -> TaResult<Self> {
        Self::compute_projected(klines, settings, &IndicatorProjection::Complete)
    }

    /// Computes only requested final outputs and their transitive pure-kernel dependencies.
    pub fn compute_projected(
        klines: &[Kline],
        settings: IndicatorSettings,
        projection: &IndicatorProjection,
    ) -> TaResult<Self> {
        let plan = super::plan::standard_plan(settings)?.project(projection);
        plan.compile()?.execute(klines)
    }

    /// Returns the number of source candles represented by every output.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns whether the source series had no candle rows.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns a declared output column by its static name.
    pub fn column(&self, name: &str) -> Option<&IndicatorColumn> {
        IndicatorOutput::all()
            .find(|output| output.column_name() == name)
            .and_then(|output| self.columns.get(&output))
    }

    /// Returns output column names in deterministic order.
    pub fn column_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        let mut names = self
            .columns
            .keys()
            .map(|output| output.column_name())
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.into_iter()
    }
}

pub(crate) fn numbers(values: Vec<f64>) -> Vec<Option<f64>> {
    values
        .into_iter()
        .map(|value| value.is_finite().then_some(value))
        .collect()
}

/// Enforces the kernel postcondition before nullable frame materialization.
pub(crate) fn validate_indicator_output(column: &'static str, values: &[f64]) -> TaResult<()> {
    if let Some(row) = values.iter().position(|value| !value.is_finite()) {
        return Err(TaError::non_finite_indicator_output(row, column));
    }
    Ok(())
}
pub(crate) fn booleans(values: Vec<bool>) -> Vec<Option<bool>> {
    values.into_iter().map(Some).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};

    fn klines() -> Vec<Kline> {
        (0..4)
            .map(|index| {
                let open = 100.0 + index as f64;
                Kline {
                    open,
                    high: open + 3.0,
                    low: open - 2.0,
                    close: open + if index % 2 == 0 { 2.0 } else { -1.0 },
                    volume: 1_000.0 + index as f64,
                    time: 1_700_000_000_000 + index * 60_000,
                    adjclose: None,
                }
            })
            .collect()
    }

    #[test]
    fn computes_aligned_finite_columns() {
        let frame = IndicatorFrame::compute(
            &klines(),
            IndicatorSettings {
                atr_period: 1,
                ..IndicatorSettings::default()
            },
        )
        .unwrap();
        assert_eq!(frame.len(), 4);
        assert_eq!(frame.column("atr").unwrap().len(), frame.len());
        assert_eq!(
            frame.column("rssi").unwrap(),
            &IndicatorColumn::Number(vec![
                Some(50.0),
                Some(0.0),
                Some(68.29268292682927),
                Some(49.93141289437586),
            ])
        );
        assert_eq!(frame.column("body_ratio").unwrap().len(), frame.len());
    }

    #[test]
    fn projected_leaf_contains_only_the_requested_output() {
        let projection = IndicatorProjection::selected([IndicatorOutput::Sharpe]);
        let frame =
            IndicatorFrame::compute_projected(&klines(), IndicatorSettings::default(), &projection)
                .unwrap();

        assert_eq!(frame.column_names().collect::<Vec<_>>(), vec!["sharpe"]);
    }

    #[test]
    fn projected_outputs_match_complete_without_unrelated_columns() {
        let settings = IndicatorSettings {
            volume_ema_period: 2,
            ema_period: 2,
            rsi_period: 2,
            rsi_smooth_period: 2,
            reverse_rsi_period: 2,
            atr_period: 2,
            bias_period: 2,
            structure_period: 2,
            structure_sma_period: 2,
            sharpe_period: 2,
        };
        let complete = IndicatorFrame::compute(&klines(), settings).unwrap();
        let requested = [
            IndicatorOutput::RssiMa,
            IndicatorOutput::StructurePowerSma,
            IndicatorOutput::AtrUpperBand,
            IndicatorOutput::AtrPercent,
            IndicatorOutput::BandReversion,
            IndicatorOutput::AtrReversionPercent,
            IndicatorOutput::IsAtrGap,
            IndicatorOutput::NeutralReverseRsi,
            IndicatorOutput::BullishReverseRsi,
            IndicatorOutput::BearishReverseRsi,
        ];
        let projection = IndicatorProjection::selected(requested);
        let projected =
            IndicatorFrame::compute_projected(&klines(), settings, &projection).unwrap();

        assert_eq!(projected.len(), complete.len());
        assert_eq!(
            projection.outputs().collect::<Vec<_>>().len(),
            requested.len()
        );
        assert_eq!(
            projected.column_names().collect::<Vec<_>>(),
            vec![
                "atr_percent",
                "atr_reversion_percent",
                "atr_upperband",
                "band_reversion",
                "bearish_revrsi",
                "bullish_revrsi",
                "is_atr_gap",
                "neutral_revrsi",
                "rssi_ma",
                "structure_power_sma",
            ]
        );
        assert_frame_close(&projected, &complete, 1e-12);
        assert!(projected.column("atr").is_none());
        assert!(projected.column("bias_reversion").is_none());
        assert!(projected.column("rssi").is_none());
        assert!(projected.column("structure_power").is_none());
    }

    #[test]
    fn selected_projection_deduplicates_unions_and_preserves_leaves() {
        let projection = IndicatorProjection::selected([
            IndicatorOutput::BodyRatio,
            IndicatorOutput::Sharpe,
            IndicatorOutput::BodyRatio,
            IndicatorOutput::Ema200,
        ]);
        assert!(projection.contains(IndicatorOutput::BodyRatio));
        assert!(!projection.contains(IndicatorOutput::Atr));
        assert_eq!(projection.outputs().collect::<Vec<_>>().len(), 3);

        let frame =
            IndicatorFrame::compute_projected(&klines(), IndicatorSettings::default(), &projection)
                .unwrap();
        assert_eq!(
            frame.column_names().collect::<Vec<_>>(),
            vec!["body_ratio", "ema200", "sharpe"]
        );
    }

    #[test]
    fn projection_preserves_validation_and_chronology_failures() {
        let projection = IndicatorProjection::selected([IndicatorOutput::Sharpe]);
        assert!(
            IndicatorFrame::compute_projected(&[], IndicatorSettings::default(), &projection)
                .is_err()
        );
        assert!(
            IndicatorFrame::compute_projected(
                &klines(),
                IndicatorSettings {
                    atr_period: 0,
                    ..IndicatorSettings::default()
                },
                &projection,
            )
            .is_err()
        );

        let mut unordered = klines();
        unordered[1].time = unordered[0].time;
        assert!(
            IndicatorFrame::compute_projected(
                &unordered,
                IndicatorSettings::default(),
                &projection
            )
            .is_err()
        );
    }

    #[test]
    fn compute_preserves_complete_fixed_fixture() {
        let frame = IndicatorFrame::compute(
            &klines(),
            IndicatorSettings {
                volume_ema_period: 2,
                ema_period: 2,
                rsi_period: 2,
                rsi_smooth_period: 2,
                reverse_rsi_period: 2,
                atr_period: 2,
                bias_period: 2,
                structure_period: 2,
                structure_sma_period: 2,
                sharpe_period: 2,
            },
        )
        .unwrap();
        let expected_numbers = [
            ("atr", vec![5.0, 5.0, 5.0, 5.0]),
            ("atr_lowerband", vec![91.91, 92.91, 93.91, 94.91]),
            (
                "atr_percent",
                vec![
                    0.05,
                    0.04950495049504951,
                    0.049019607843137254,
                    0.04854368932038835,
                ],
            ),
            ("atr_reversion_percent", vec![0.0; 4]),
            ("atr_upperband", vec![108.09, 109.09, 110.09, 111.09]),
            ("band_reversion", vec![0.0; 4]),
            (
                "bearish_revrsi",
                vec![98.0, 97.83333333333333, 98.25, 98.95833333333333],
            ),
            ("bias_reversion", vec![97.0, 98.25, 99.625, 100.8125]),
            ("body_ratio", vec![0.4, 0.2, 0.4, 0.2]),
            (
                "bullish_revrsi",
                vec![103.0, 103.78571428571429, 104.67857142857143, 105.625],
            ),
            (
                "ema200",
                vec![
                    102.0,
                    100.66666666666666,
                    102.88888888888889,
                    102.2962962962963,
                ],
            ),
            ("neutral_revrsi", vec![103.0, 102.0, 103.5, 103.25]),
            ("rssi", vec![50.0, 0.0, 80.0, 44.44444444444444]),
            (
                "rssi_ma",
                vec![
                    50.0,
                    16.666666666666668,
                    58.888888888888886,
                    49.25925925925925,
                ],
            ),
            (
                "sharpe",
                vec![
                    0.0,
                    -0.35355339059327373,
                    0.17677669529663687,
                    0.35355339059327373,
                ],
            ),
            ("structure_power", vec![3.0, 1.5, 2.25, 1.125]),
            ("structure_power_sma", vec![3.0, 2.25, 1.875, 1.6875]),
            (
                "volume_sma",
                vec![
                    1000.0,
                    1000.6666666666666,
                    1001.5555555555557,
                    1002.5185185185185,
                ],
            ),
        ];
        let mut expected = expected_numbers
            .into_iter()
            .map(|(name, values)| {
                let output = IndicatorOutput::all()
                    .find(|output| output.column_name() == name)
                    .unwrap();
                (output, IndicatorColumn::Number(numbers(values)))
            })
            .collect::<BTreeMap<_, _>>();
        expected.insert(
            IndicatorOutput::IsAtrGap,
            IndicatorColumn::Boolean(vec![Some(false); 4]),
        );
        assert_eq!(
            frame,
            IndicatorFrame {
                len: 4,
                columns: expected
            }
        );
    }

    #[test]
    fn rejects_invalid_input_and_periods() {
        assert!(
            IndicatorFrame::compute(
                &klines(),
                IndicatorSettings {
                    atr_period: 0,
                    ..IndicatorSettings::default()
                }
            )
            .is_err()
        );
        let mut invalid = klines();
        invalid[1].time = invalid[0].time;
        assert!(IndicatorFrame::compute(&invalid, IndicatorSettings::default()).is_err());
    }

    #[test]
    fn rejects_finite_inputs_that_overflow_an_indicator_output() {
        let extreme = vec![Kline {
            open: f64::MAX,
            high: f64::MAX,
            low: -f64::MAX,
            close: f64::MAX,
            volume: 1.0,
            time: 1_700_000_000_000,
            adjclose: None,
        }];

        let error = IndicatorFrame::compute(&extreme, IndicatorSettings::default())
            .expect_err("finite inputs with overflowing indicator output must fail");

        assert_eq!(error.kind, crate::ta::TaErrorKind::NonFiniteIndicatorOutput);
        assert_eq!(error.context.as_deref(), Some("row 0, column atr"));
    }

    #[allow(dead_code)]
    fn seeded_klines(len: usize, seed: u64) -> Vec<Kline> {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        (0..len)
            .map(|index| {
                let open: f64 = 100.0 + (rng.random_range(-1.0..1.0) * 12.0);
                let close: f64 = open + rng.random_range(-2.5..2.5);
                let high: f64 = open.max(close) + rng.random_range(0.0..1.8);
                let low: f64 = open.min(close) - rng.random_range(0.0..1.8);
                Kline {
                    open,
                    high,
                    low,
                    close,
                    volume: 1_000.0 + rng.random_range(-100.0..100.0),
                    time: 1_700_000_000_000 + index as i64 * 60_000,
                    adjclose: None,
                }
            })
            .collect()
    }

    fn assert_frame_close(actual: &IndicatorFrame, expected: &IndicatorFrame, tolerance: f64) {
        assert_eq!(actual.len(), expected.len());
        for name in actual.column_names() {
            match (actual.column(name).unwrap(), expected.column(name).unwrap()) {
                (IndicatorColumn::Number(actual), IndicatorColumn::Number(expected)) => {
                    assert_eq!(actual.len(), expected.len());
                    for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
                        assert!(
                            actual.is_some_and(f64::is_finite),
                            "column {name} row {index} is not finite"
                        );
                        assert!(
                            (actual.unwrap() - expected.unwrap()).abs() <= tolerance,
                            "column {name} row {index} differs: {} vs {}",
                            actual.unwrap(),
                            expected.unwrap()
                        );
                    }
                }
                (IndicatorColumn::Boolean(actual), IndicatorColumn::Boolean(expected)) => {
                    assert_eq!(actual, expected);
                }
                _ => unreachable!("column {name} changed type"),
            }
        }
    }
}

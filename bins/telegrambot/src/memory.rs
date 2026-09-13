//! Persistent memory — per-ticker JSON files with predictions, weights, and
//! last-notified state.
//!
//! Storage layout:
//!   {MEMORY_DIR}/{SYMBOL}.json  — e.g. /data/memory/BTC-USDT.json

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Canonical indicator keys persisted in every prediction and notification snapshot.
pub const CANONICAL_TRACKED_INDICATOR_KEYS: &[&str] = &[
    "rssi",
    "structure_power",
    "band_reversion",
    "atr_percent",
    "sharpe",
    "close",
];

// ─── Data Types ──────────────────────────────────────────────────────────────

/// Terminal outcome kind for a single trade plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradePlanOutcomeKind {
    /// Outcome where the take-profit level was reached.
    TakeProfit,
    /// Outcome where the stop-loss level was reached.
    StopLoss,
    /// Outcome that could not be classified unambiguously.
    Ambiguous,
}

/// Settled outcome metadata for a single trade plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradePlanOutcome {
    /// Outcome classification.
    pub kind: TradePlanOutcomeKind,
    /// Time the entry level was first reached, if known.
    pub entry_hit_at: Option<DateTime<Utc>>,
    /// Time the outcome was resolved.
    pub resolved_at: DateTime<Utc>,
    /// Timeframe used to resolve the outcome.
    pub resolution_timeframe: algotrap::prelude::Timeframe,
    /// Lowest price reached during the plan.
    pub lowest_reached: f64,
    /// Highest price reached during the plan.
    pub highest_reached: f64,
}

/// A single trade plan option (A, B, or C).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradePlan {
    /// Human-readable option label (for example, "A").
    pub label: String,     // "A", "B", "C"
    /// Planned direction: "LONG", "SHORT", or "WAIT".
    pub direction: String, // "LONG" | "SHORT" | "WAIT"
    /// Entry price, if specified.
    pub entry: Option<f64>,
    /// Target price, if specified.
    pub target: Option<f64>,
    /// Stop-loss price, if specified.
    pub stop: Option<f64>,
    /// Explanation for the plan.
    pub rationale: String,
    /// Timeframe used to evaluate the plan, if specified.
    #[serde(default)]
    pub timeframe: Option<algotrap::prelude::Timeframe>,
    /// Settled outcome, if validation has completed.
    #[serde(default)]
    pub outcome: Option<TradePlanOutcome>,
}

/// A stored prediction from a single scan cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    /// Time the prediction was generated.
    pub timestamp: DateTime<Utc>,
    /// Prediction confidence, expressed as a percentage.
    pub confidence: f64,
    /// Predicted market direction.
    pub direction: algotrap::prelude::Direction,
    /// Human-readable prediction summary.
    pub summary: String,
    /// Candidate trade plans accompanying the prediction.
    #[serde(default)]
    pub trade_plans: Vec<TradePlan>,
    /// Canonical tracked indicator snapshot at prediction time.
    ///
    /// Every tracked key is persisted on every cycle. `None` means the output
    /// was inactive or unavailable for that frame, not that the value was zero.
    #[serde(
        default = "canonical_indicator_snapshot",
        deserialize_with = "deserialize_indicator_snapshot"
    )]
    pub indicators: HashMap<String, Option<f64>>,
    /// Outcome score set after validation (None = not yet validated).
    #[serde(default)]
    pub outcome_score: Option<f64>,
}

/// Per-indicator weights tuned by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weights {
    /// Indicator weights keyed by canonical indicator name.
    pub values: HashMap<String, f64>,
    /// LLM-tuned significance threshold for change detection (seeded at 0.25).
    #[serde(default = "default_significance_threshold")]
    pub significance_threshold: f64,
}

fn default_significance_threshold() -> f64 {
    0.25
}

fn canonical_indicator_snapshot() -> HashMap<String, Option<f64>> {
    CANONICAL_TRACKED_INDICATOR_KEYS
        .iter()
        .map(|key| ((*key).to_string(), None))
        .collect()
}

fn deserialize_indicator_snapshot<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, Option<f64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = HashMap::<String, Option<f64>>::deserialize(deserializer)?;
    let mut canonical = canonical_indicator_snapshot();
    canonical.extend(raw);
    Ok(canonical)
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            significance_threshold: 0.25,
        }
    }
}

/// A single tunable parameter with bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    /// Current parameter value.
    pub value: f64,
    /// Inclusive lower bound.
    pub min: f64,
    /// Inclusive upper bound.
    pub max: f64,
}

impl ParamSpec {
    /// Creates a tunable parameter with the supplied value and bounds.
    pub fn new(value: f64, min: f64, max: f64) -> Self {
        Self { value, min, max }
    }

    /// Clamp to bounds.
    pub fn clamped(&self) -> f64 {
        self.value.clamp(self.min, self.max)
    }
}

/// Legacy grouped indicator parameter wire retained only for JSON migration.
#[derive(Debug, Clone, Deserialize)]
struct LegacyIndicatorParams {
    /// Optional period parameter (e.g., RSI period, ATR period).
    pub period: Option<ParamSpec>,
    /// Optional smoothing parameter (e.g., EMA smooth window).
    pub smooth: Option<ParamSpec>,
    /// Whether this indicator is currently active.
    #[serde(default = "default_true")]
    pub active: bool,
    /// Cycles since this indicator was deactivated (0 if active).
    #[serde(default)]
    pub inactive_cycles: u32,
}

fn default_true() -> bool {
    true
}

/// Strongly typed shared period names accepted from LLM tuning proposals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeriodName {
    /// Volume EMA period.
    VolumeEma,
    /// EMA period.
    Ema,
    /// RSI period.
    Rsi,
    /// Smoothed RSI period.
    RsiSmooth,
    /// Reverse RSI period.
    ReverseRsi,
    /// ATR period.
    Atr,
    /// Bias smoothing period.
    Bias,
    /// Structure-power period.
    Structure,
    /// Structure SMA period.
    StructureSma,
    /// Sharpe lookback period.
    Sharpe,
}

impl PeriodName {
    /// All supported period names.
    pub const ALL: [Self; 10] = [
        Self::VolumeEma,
        Self::Ema,
        Self::Rsi,
        Self::RsiSmooth,
        Self::ReverseRsi,
        Self::Atr,
        Self::Bias,
        Self::Structure,
        Self::StructureSma,
        Self::Sharpe,
    ];

    /// Returns the stable snake_case name used in persisted configuration.
    pub const fn persisted_name(self) -> &'static str {
        match self {
            Self::VolumeEma => "volume_ema",
            Self::Ema => "ema",
            Self::Rsi => "rsi",
            Self::RsiSmooth => "rsi_smooth",
            Self::ReverseRsi => "reverse_rsi",
            Self::Atr => "atr",
            Self::Bias => "bias",
            Self::Structure => "structure",
            Self::StructureSma => "structure_sma",
            Self::Sharpe => "sharpe",
        }
    }

    pub(crate) fn spec(self, periods: &IndicatorPeriods) -> &ParamSpec {
        match self {
            Self::VolumeEma => &periods.volume_ema,
            Self::Ema => &periods.ema,
            Self::Rsi => &periods.rsi,
            Self::RsiSmooth => &periods.rsi_smooth,
            Self::ReverseRsi => &periods.reverse_rsi,
            Self::Atr => &periods.atr,
            Self::Bias => &periods.bias,
            Self::Structure => &periods.structure,
            Self::StructureSma => &periods.structure_sma,
            Self::Sharpe => &periods.sharpe,
        }
    }

    fn spec_mut(self, periods: &mut IndicatorPeriods) -> &mut ParamSpec {
        match self {
            Self::VolumeEma => &mut periods.volume_ema,
            Self::Ema => &mut periods.ema,
            Self::Rsi => &mut periods.rsi,
            Self::RsiSmooth => &mut periods.rsi_smooth,
            Self::ReverseRsi => &mut periods.reverse_rsi,
            Self::Atr => &mut periods.atr,
            Self::Bias => &mut periods.bias,
            Self::Structure => &mut periods.structure,
            Self::StructureSma => &mut periods.structure_sma,
            Self::Sharpe => &mut periods.sharpe,
        }
    }
}

/// Shared tunable period windows for Telegram indicator computation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IndicatorPeriods {
    /// Volume EMA window used for the `volume_sma` output.
    ///
    /// This is intentionally fixed at 20 (`min == max == 20`) to preserve the
    /// current Telegram presentation contract.
    pub volume_ema: ParamSpec,
    /// EMA window used for the `ema200` output.
    pub ema: ParamSpec,
    /// RSI window used for the `rssi` output family.
    pub rsi: ParamSpec,
    /// Smoothing window used for the `rssi_ma` output.
    pub rsi_smooth: ParamSpec,
    /// Reverse RSI window used for the RevRSI output family.
    pub reverse_rsi: ParamSpec,
    /// Shared ATR window used by ATR-derived outputs and gap zones.
    pub atr: ParamSpec,
    /// Bias smoothing window used for `bias_reversion`.
    pub bias: ParamSpec,
    /// Structure power window used for `structure_power`.
    pub structure: ParamSpec,
    /// Structure SMA window used for `structure_power_sma`.
    ///
    /// This is intentionally fixed at 16 (`min == max == 16`) to preserve the
    /// current Telegram presentation contract.
    pub structure_sma: ParamSpec,
    /// Sharpe lookback window used for `sharpe`.
    pub sharpe: ParamSpec,
}

impl Default for IndicatorPeriods {
    fn default() -> Self {
        Self {
            volume_ema: ParamSpec::new(20.0, 20.0, 20.0),
            ema: ParamSpec::new(200.0, 50.0, 500.0),
            rsi: ParamSpec::new(14.0, 5.0, 50.0),
            rsi_smooth: ParamSpec::new(9.0, 3.0, 30.0),
            reverse_rsi: ParamSpec::new(14.0, 5.0, 50.0),
            atr: ParamSpec::new(42.0, 10.0, 100.0),
            bias: ParamSpec::new(9.0, 3.0, 30.0),
            structure: ParamSpec::new(9.0, 3.0, 30.0),
            structure_sma: ParamSpec::new(16.0, 16.0, 16.0),
            sharpe: ParamSpec::new(200.0, 50.0, 500.0),
        }
    }
}

/// Per-output activation state persisted independently from period tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputState {
    /// Whether the output is currently visible to downstream Telegram consumers.
    #[serde(default = "default_true")]
    pub active: bool,
    /// Cycles since this output was deactivated.
    #[serde(default)]
    pub inactive_cycles: u32,
}

impl Default for OutputState {
    fn default() -> Self {
        Self {
            active: true,
            inactive_cycles: 0,
        }
    }
}

/// Telegram graph outputs with independently persisted activation state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TelegramOutputConfig {
    /// Activation state for the ATR output.
    pub atr: OutputState,
    /// Activation state for the volume SMA output.
    pub volume_sma: OutputState,
    /// Activation state for the EMA 200 output.
    pub ema200: OutputState,
    /// Activation state for the bias-reversion output.
    pub bias_reversion: OutputState,
    /// Activation state for the neutral RevRSI output.
    pub neutral_revrsi: OutputState,
    /// Activation state for the bullish RevRSI output.
    pub bullish_revrsi: OutputState,
    /// Activation state for the bearish RevRSI output.
    pub bearish_revrsi: OutputState,
    /// Activation state for the upper ATR band output.
    pub atr_upperband: OutputState,
    /// Activation state for the lower ATR band output.
    pub atr_lowerband: OutputState,
    /// Activation state for the RSSI output.
    pub rssi: OutputState,
    /// Activation state for the smoothed RSSI output.
    pub rssi_ma: OutputState,
    /// Activation state for the structure-power output.
    pub structure_power: OutputState,
    /// Activation state for the structure-power SMA output.
    pub structure_power_sma: OutputState,
    /// Activation state for the ATR percentage output.
    pub atr_percent: OutputState,
    /// Activation state for the ATR-reversion percentage output.
    pub atr_reversion_percent: OutputState,
    /// Activation state for the band-reversion output.
    pub band_reversion: OutputState,
    /// Activation state for the Sharpe output.
    pub sharpe: OutputState,
    /// Activation state for the body-ratio output.
    pub body_ratio: OutputState,
    /// Activation state for the ATR gap-zone output.
    pub is_atr_gap: OutputState,
    /// Activation state for the leverage output.
    pub leverage: OutputState,
}

/// Strongly typed Telegram output names accepted from LLM tuning proposals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramOutputName {
    /// ATR output.
    Atr,
    /// Volume SMA output.
    VolumeSma,
    /// EMA 200 output.
    Ema200,
    /// Bias-reversion output.
    BiasReversion,
    /// Neutral RevRSI output.
    NeutralRevrsi,
    /// Bullish RevRSI output.
    BullishRevrsi,
    /// Bearish RevRSI output.
    BearishRevrsi,
    /// Upper ATR band output.
    AtrUpperband,
    /// Lower ATR band output.
    AtrLowerband,
    /// RSSI output.
    Rssi,
    /// Smoothed RSSI output.
    RssiMa,
    /// Structure-power output.
    StructurePower,
    /// Structure-power SMA output.
    StructurePowerSma,
    /// ATR percentage output.
    AtrPercent,
    /// ATR-reversion percentage output.
    AtrReversionPercent,
    /// Band-reversion output.
    BandReversion,
    /// Sharpe output.
    Sharpe,
    /// Body-ratio output.
    BodyRatio,
    /// ATR gap-zone output.
    IsAtrGap,
    /// Leverage output.
    Leverage,
}

impl TelegramOutputName {
    /// All supported Telegram output names.
    pub const ALL: [Self; 20] = [
        Self::Atr,
        Self::VolumeSma,
        Self::Ema200,
        Self::BiasReversion,
        Self::NeutralRevrsi,
        Self::BullishRevrsi,
        Self::BearishRevrsi,
        Self::AtrUpperband,
        Self::AtrLowerband,
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
        Self::Leverage,
    ];

    /// Returns the stable snake_case name used in persisted configuration.
    pub const fn persisted_name(self) -> &'static str {
        match self {
            Self::Atr => "atr",
            Self::VolumeSma => "volume_sma",
            Self::Ema200 => "ema200",
            Self::BiasReversion => "bias_reversion",
            Self::NeutralRevrsi => "neutral_revrsi",
            Self::BullishRevrsi => "bullish_revrsi",
            Self::BearishRevrsi => "bearish_revrsi",
            Self::AtrUpperband => "atr_upperband",
            Self::AtrLowerband => "atr_lowerband",
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
            Self::Leverage => "leverage",
        }
    }

    fn from_persisted_name(name: &str) -> Option<Self> {
        match name {
            "atr" => Some(Self::Atr),
            "volume_sma" => Some(Self::VolumeSma),
            "ema200" => Some(Self::Ema200),
            "bias_reversion" => Some(Self::BiasReversion),
            "neutral_revrsi" => Some(Self::NeutralRevrsi),
            "bullish_revrsi" => Some(Self::BullishRevrsi),
            "bearish_revrsi" => Some(Self::BearishRevrsi),
            "atr_upperband" => Some(Self::AtrUpperband),
            "atr_lowerband" => Some(Self::AtrLowerband),
            "rssi" => Some(Self::Rssi),
            "rssi_ma" => Some(Self::RssiMa),
            "structure_power" => Some(Self::StructurePower),
            "structure_power_sma" => Some(Self::StructurePowerSma),
            "atr_percent" => Some(Self::AtrPercent),
            "atr_reversion_percent" => Some(Self::AtrReversionPercent),
            "band_reversion" => Some(Self::BandReversion),
            "sharpe" => Some(Self::Sharpe),
            "body_ratio" => Some(Self::BodyRatio),
            "is_atr_gap" => Some(Self::IsAtrGap),
            "leverage" => Some(Self::Leverage),
            _ => None,
        }
    }

    pub(crate) fn state(self, outputs: &TelegramOutputConfig) -> &OutputState {
        match self {
            Self::Atr => &outputs.atr,
            Self::VolumeSma => &outputs.volume_sma,
            Self::Ema200 => &outputs.ema200,
            Self::BiasReversion => &outputs.bias_reversion,
            Self::NeutralRevrsi => &outputs.neutral_revrsi,
            Self::BullishRevrsi => &outputs.bullish_revrsi,
            Self::BearishRevrsi => &outputs.bearish_revrsi,
            Self::AtrUpperband => &outputs.atr_upperband,
            Self::AtrLowerband => &outputs.atr_lowerband,
            Self::Rssi => &outputs.rssi,
            Self::RssiMa => &outputs.rssi_ma,
            Self::StructurePower => &outputs.structure_power,
            Self::StructurePowerSma => &outputs.structure_power_sma,
            Self::AtrPercent => &outputs.atr_percent,
            Self::AtrReversionPercent => &outputs.atr_reversion_percent,
            Self::BandReversion => &outputs.band_reversion,
            Self::Sharpe => &outputs.sharpe,
            Self::BodyRatio => &outputs.body_ratio,
            Self::IsAtrGap => &outputs.is_atr_gap,
            Self::Leverage => &outputs.leverage,
        }
    }

    pub(crate) fn state_mut(self, outputs: &mut TelegramOutputConfig) -> &mut OutputState {
        match self {
            Self::Atr => &mut outputs.atr,
            Self::VolumeSma => &mut outputs.volume_sma,
            Self::Ema200 => &mut outputs.ema200,
            Self::BiasReversion => &mut outputs.bias_reversion,
            Self::NeutralRevrsi => &mut outputs.neutral_revrsi,
            Self::BullishRevrsi => &mut outputs.bullish_revrsi,
            Self::BearishRevrsi => &mut outputs.bearish_revrsi,
            Self::AtrUpperband => &mut outputs.atr_upperband,
            Self::AtrLowerband => &mut outputs.atr_lowerband,
            Self::Rssi => &mut outputs.rssi,
            Self::RssiMa => &mut outputs.rssi_ma,
            Self::StructurePower => &mut outputs.structure_power,
            Self::StructurePowerSma => &mut outputs.structure_power_sma,
            Self::AtrPercent => &mut outputs.atr_percent,
            Self::AtrReversionPercent => &mut outputs.atr_reversion_percent,
            Self::BandReversion => &mut outputs.band_reversion,
            Self::Sharpe => &mut outputs.sharpe,
            Self::BodyRatio => &mut outputs.body_ratio,
            Self::IsAtrGap => &mut outputs.is_atr_gap,
            Self::Leverage => &mut outputs.leverage,
        }
    }
}

impl TelegramOutputConfig {
    fn state_mut(&mut self, name: &str) -> Option<&mut OutputState> {
        TelegramOutputName::from_persisted_name(name).map(|output| output.state_mut(self))
    }

    fn active_count(&self) -> usize {
        TelegramOutputName::ALL
            .into_iter()
            .filter(|name| name.state(self).active)
            .count()
    }

    fn dormant_roster(&self) -> Vec<(&'static str, u32)> {
        TelegramOutputName::ALL
            .into_iter()
            .filter_map(|name| {
                let state = name.state(self);
                (!state.active).then_some((name.persisted_name(), state.inactive_cycles))
            })
            .collect()
    }
}

/// Gap-zone tuning persisted separately from output activation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GapZoneConfig {
    /// Per-timeframe recent gap-zone budget for LLM/chart consumption.
    pub max_zones: ParamSpec,
    /// Minimum candle body-ratio required for gap-zone qualification.
    #[serde(default = "default_body_ratio_threshold")]
    pub body_ratio_threshold: ParamSpec,
    /// Tunable ATR band-width multiplier for gap-zone bands.
    #[serde(default = "default_atr_band_multiplier")]
    pub atr_band_multiplier: ParamSpec,
    /// Tunable ATR gap-size multiplier for gap-zone qualification.
    #[serde(default = "default_atr_gap_multiplier")]
    pub atr_gap_multiplier: ParamSpec,
}

fn default_body_ratio_threshold() -> ParamSpec {
    ParamSpec::new(0.618, 0.0, 1.0)
}

fn default_atr_band_multiplier() -> ParamSpec {
    ParamSpec::new(1.618, 0.5, 5.0)
}

fn default_atr_gap_multiplier() -> ParamSpec {
    ParamSpec::new(1.0, 0.5, 5.0)
}

impl Default for GapZoneConfig {
    fn default() -> Self {
        Self {
            max_zones: ParamSpec::new(16.0, 1.0, 32.0),
            body_ratio_threshold: ParamSpec::new(0.618, 0.0, 1.0),
            atr_band_multiplier: ParamSpec::new(1.618, 0.5, 5.0),
            atr_gap_multiplier: ParamSpec::new(1.0, 0.5, 5.0),
        }
    }
}

/// Typed LLM proposal DTO for indicator tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum IndicatorProposal {
    /// Tune one shared period window.
    Period {
        /// Period parameter to tune.
        name: PeriodName,
        /// Proposed period value.
        value: f64,
    },
    /// Toggle one exact Telegram output.
    Output {
        /// Output to toggle.
        name: TelegramOutputName,
        /// Desired output activation state.
        active: bool,
    },
    /// Tune gap-zone extraction parameters.
    GapZones {
        /// Proposed per-timeframe recent gap-zone budget.
        #[serde(default)]
        max_zones: Option<f64>,
        /// Proposed minimum candle body-ratio for gap-zone qualification.
        #[serde(default)]
        body_ratio_threshold: Option<f64>,
        /// Proposed ATR band-width multiplier.
        #[serde(default)]
        atr_band_multiplier: Option<f64>,
        /// Proposed ATR gap-size multiplier.
        #[serde(default)]
        atr_gap_multiplier: Option<f64>,
    },
}

const LEGACY_RSSI_OUTPUTS: &[&str] = &["rssi", "rssi_ma"];
const LEGACY_STRUCTURE_OUTPUTS: &[&str] = &["structure_power", "structure_power_sma"];
const LEGACY_ATR_OUTPUTS: &[&str] = &["atr", "atr_upperband", "atr_lowerband", "atr_percent"];
const LEGACY_REVRSI_OUTPUTS: &[&str] = &["neutral_revrsi", "bullish_revrsi", "bearish_revrsi"];
const LEGACY_GAP_ZONE_OUTPUTS: &[&str] = &["is_atr_gap", "body_ratio"];
const LEGACY_EMA200_OUTPUTS: &[&str] = &["ema200"];
const LEGACY_SHARPE_OUTPUTS: &[&str] = &["sharpe"];
const LEGACY_BIAS_REV_OUTPUTS: &[&str] = &["bias_reversion"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewIndicatorConfigWire {
    #[serde(default)]
    periods: IndicatorPeriods,
    #[serde(default)]
    outputs: TelegramOutputConfig,
    #[serde(default)]
    gap_zones: GapZoneConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyIndicatorConfigWire {
    #[serde(default)]
    indicators: HashMap<String, LegacyIndicatorParams>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum IndicatorConfigWire {
    New(Box<NewIndicatorConfigWire>),
    Legacy(LegacyIndicatorConfigWire),
}

/// Per-ticker indicator configuration, persisted and LLM-tunable.
///
/// Period tuning is shared across indicator families, output activation is
/// independent per emitted Telegram column, and gap-zone extraction parameters
/// are stored separately from visibility state.
#[derive(Debug, Clone, Serialize, Default)]
pub struct IndicatorConfig {
    /// Shared period windows used during indicator computation.
    pub periods: IndicatorPeriods,
    /// Per-output activation state for Telegram-visible graph columns.
    pub outputs: TelegramOutputConfig,
    /// Gap-zone extraction parameters unrelated to output visibility.
    pub gap_zones: GapZoneConfig,
}

impl<'de> Deserialize<'de> for IndicatorConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let wire = serde_json::from_value::<IndicatorConfigWire>(value).map_err(|error| {
            serde::de::Error::custom(format!(
                "invalid indicator_config: expected typed {{periods, outputs, gap_zones}} or legacy {{indicators}} schema: {error}"
            ))
        })?;
        let config = match wire {
            IndicatorConfigWire::New(wire) => Self {
                periods: wire.periods,
                outputs: wire.outputs,
                gap_zones: wire.gap_zones,
            },
            IndicatorConfigWire::Legacy(wire) => Self::from_legacy_indicators(wire.indicators),
        };

        Ok(config)
    }
}

impl IndicatorConfig {
    fn from_legacy_indicators(indicators: HashMap<String, LegacyIndicatorParams>) -> Self {
        let mut config = Self {
            periods: IndicatorPeriods::default(),
            outputs: TelegramOutputConfig::default(),
            gap_zones: GapZoneConfig::default(),
        };
        let mut gap_zone_atr_fallback = None;
        let mut atr_period_migrated = false;

        for (name, params) in indicators {
            match canonical_legacy_name(&name) {
                Some("rssi") => {
                    migrate_spec(&mut config.periods.rsi, params.period.clone());
                    migrate_spec(&mut config.periods.rsi_smooth, params.smooth.clone());
                    config.migrate_legacy_state(LEGACY_RSSI_OUTPUTS, &params);
                }
                Some("structure_power") => {
                    migrate_spec(&mut config.periods.structure, params.smooth.clone());
                    config.migrate_legacy_state(LEGACY_STRUCTURE_OUTPUTS, &params);
                }
                Some("atr") => {
                    atr_period_migrated = params.period.is_some();
                    migrate_spec(&mut config.periods.atr, params.period.clone());
                    config.migrate_legacy_state(LEGACY_ATR_OUTPUTS, &params);
                }
                Some("ema200") => {
                    migrate_spec(&mut config.periods.ema, params.period.clone());
                    config.migrate_legacy_state(LEGACY_EMA200_OUTPUTS, &params);
                }
                Some("sharpe") => {
                    migrate_spec(&mut config.periods.sharpe, params.period.clone());
                    config.migrate_legacy_state(LEGACY_SHARPE_OUTPUTS, &params);
                }
                Some("bias_reversion") => {
                    migrate_spec(&mut config.periods.bias, params.smooth.clone());
                    config.migrate_legacy_state(LEGACY_BIAS_REV_OUTPUTS, &params);
                }
                Some("revrsi") => {
                    migrate_spec(&mut config.periods.reverse_rsi, params.period.clone());
                    config.migrate_legacy_state(LEGACY_REVRSI_OUTPUTS, &params);
                }
                Some("gap_zones") => {
                    gap_zone_atr_fallback = params.period.clone().or(gap_zone_atr_fallback);
                    if let Some(spec) = params.smooth.clone() {
                        config.gap_zones.max_zones.value =
                            spec.value.clamp(config.gap_zones.max_zones.min, config.gap_zones.max_zones.max);
                    }
                    config.migrate_legacy_state(LEGACY_GAP_ZONE_OUTPUTS, &params);
                }
                _ => {}
            }
        }

        if !atr_period_migrated {
            migrate_spec(&mut config.periods.atr, gap_zone_atr_fallback);
        }

        config
    }

    fn migrate_legacy_state(&mut self, outputs: &[&str], params: &LegacyIndicatorParams) {
        for output in outputs {
            if let Some(state) = self.outputs.state_mut(output) {
                state.active = params.active;
                state.inactive_cycles = params.inactive_cycles;
            }
        }
    }

    /// Count active Telegram outputs (excludes the OHLC base tier).
    pub fn active_count(&self) -> usize {
        self.outputs.active_count()
    }

    /// Get the dormant roster: inactive outputs with their cycle counts.
    pub fn dormant_roster(&self) -> Vec<(&'static str, u32)> {
        self.outputs.dormant_roster()
    }

    /// Increment inactive_cycles for all dormant outputs.
    pub fn tick_dormant(&mut self) {
        for output in TelegramOutputName::ALL {
            let state = output.state_mut(&mut self.outputs);
            if !state.active {
                state.inactive_cycles += 1;
            }
        }
    }

    /// Apply LLM-proposed param changes with guardrails.
    ///
    /// - Range clamping: values clamped to [min, max]
    /// - Rate limiting: ±30% change per cycle (except exempt fields)
    /// - Min-2-active: cannot deactivate below 2 active derived indicators
    pub fn apply_proposals(&mut self, proposed: &[IndicatorProposal]) {
        const RATE_LIMIT: f64 = 0.30;
        const MIN_ACTIVE: usize = 2;

        // Pre-compute active count to avoid borrow conflicts
        let mut active_count = self.outputs.active_count();

        for proposal in proposed {
            match proposal {
                IndicatorProposal::Period { name, value } => {
                    if !value.is_finite() {
                        warn!(proposal = ?proposal, "Ignoring non-finite period proposal");
                        continue;
                    }
                    apply_rate_limited(
                        name.persisted_name(),
                        name.spec_mut(&mut self.periods),
                        *value,
                        RATE_LIMIT,
                    );
                }
                IndicatorProposal::Output { name, active } => {
                    self.apply_output_toggle(*name, *active, &mut active_count, MIN_ACTIVE);
                }
                IndicatorProposal::GapZones {
                    max_zones,
                    body_ratio_threshold,
                    atr_band_multiplier,
                    atr_gap_multiplier,
                } => {
                    if let Some(value) = max_zones {
                        if value.is_finite() {
                            apply_rate_limited(
                                "gap_zones.max_zones",
                                &mut self.gap_zones.max_zones,
                                *value,
                                RATE_LIMIT,
                            );
                        } else {
                            warn!(proposal = ?proposal, "Ignoring non-finite gap-zone max_zones proposal");
                        }
                    }
                    if let Some(value) = body_ratio_threshold {
                        if value.is_finite() {
                            apply_rate_limited(
                                "gap_zones.body_ratio_threshold",
                                &mut self.gap_zones.body_ratio_threshold,
                                *value,
                                RATE_LIMIT,
                            );
                        } else {
                            warn!(proposal = ?proposal, "Ignoring non-finite gap-zone body_ratio_threshold proposal");
                        }
                    }
                    if let Some(value) = atr_band_multiplier {
                        if value.is_finite() {
                            apply_rate_limited(
                                "gap_zones.atr_band_multiplier",
                                &mut self.gap_zones.atr_band_multiplier,
                                *value,
                                RATE_LIMIT,
                            );
                        } else {
                            warn!(proposal = ?proposal, "Ignoring non-finite gap-zone atr_band_multiplier proposal");
                        }
                    }
                    if let Some(value) = atr_gap_multiplier {
                        if value.is_finite() {
                            apply_rate_limited(
                                "gap_zones.atr_gap_multiplier",
                                &mut self.gap_zones.atr_gap_multiplier,
                                *value,
                                RATE_LIMIT,
                            );
                        } else {
                            warn!(proposal = ?proposal, "Ignoring non-finite gap-zone atr_gap_multiplier proposal");
                        }
                    }
                }
            }
        }
    }

    fn apply_output_toggle(
        &mut self,
        output: TelegramOutputName,
        active: bool,
        active_count: &mut usize,
        min_active: usize,
    ) {
        self.set_single_output_active(output, active, active_count, min_active);
    }

    fn set_single_output_active(
        &mut self,
        output: TelegramOutputName,
        active: bool,
        active_count: &mut usize,
        min_active: usize,
    ) {
        let currently_active = output.state(&self.outputs).active;
        if !active && currently_active && active_count.saturating_sub(1) < min_active {
            warn!(
                target = output.persisted_name(),
                proposed_active = active,
                applied_active = currently_active,
                active_count = *active_count,
                min_active,
                "Indicator output deactivation blocked by min-active guardrail"
            );
            return;
        }

        self.set_output_state(output, active);
        *active_count = self.outputs.active_count();
    }

    fn set_output_state(&mut self, output: TelegramOutputName, active: bool) {
        let state = output.state_mut(&mut self.outputs);
        if state.active != active {
            state.active = active;
            state.inactive_cycles = 0;
        } else if active {
            state.inactive_cycles = 0;
        }
    }
}

fn canonical_legacy_name(name: &str) -> Option<&'static str> {
    match name {
        "rsi" | "rssi" => Some("rssi"),
        "structure_power" => Some("structure_power"),
        "atr" => Some("atr"),
        "ema200" => Some("ema200"),
        "sharpe" => Some("sharpe"),
        "bias_reversion" => Some("bias_reversion"),
        "revrsi" => Some("revrsi"),
        "gap_zones" => Some("gap_zones"),
        _ => None,
    }
}

fn migrate_spec(target: &mut ParamSpec, source: Option<ParamSpec>) {
    if let Some(spec) = source {
        *target = spec;
    }
}

fn apply_rate_limited(target: &str, spec: &mut ParamSpec, new_val: f64, rate_limit: f64) {
    if !new_val.is_finite() {
        return;
    }
    const ADJUSTMENT_TOLERANCE: f64 = 1e-9;
    let old = spec.value;
    let max_change = old * rate_limit;
    let delta = (new_val - old).clamp(-max_change, max_change);
    let rate_limited = old + delta;
    let applied = rate_limited.clamp(spec.min, spec.max);

    let rate_limited_changed = (rate_limited - new_val).abs() > ADJUSTMENT_TOLERANCE;
    let range_clamped = (applied - rate_limited).abs() > ADJUSTMENT_TOLERANCE;

    if rate_limited_changed || range_clamped {
        let adjustment = match (rate_limited_changed, range_clamped) {
            (true, true) => "rate_limited_and_clamped",
            (true, false) => "rate_limited",
            (false, true) => "clamped",
            (false, false) => unreachable!(),
        };
        warn!(
            target,
            proposed = new_val,
            applied,
            previous = old,
            min = spec.min,
            max = spec.max,
            adjustment,
            "Indicator proposal adjusted by guardrails"
        );
    }

    spec.value = applied;
}

/// Snapshot of indicator values from the last notification (for delta detection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifiedSnapshot {
    /// Canonical tracked indicator snapshot from the last notification.
    ///
    /// Every tracked key is present. `None` means unavailable/inactive.
    #[serde(
        default = "canonical_indicator_snapshot",
        deserialize_with = "deserialize_indicator_snapshot"
    )]
    pub indicators: HashMap<String, Option<f64>>,
    /// Time of the last notification, if available.
    pub timestamp: Option<DateTime<Utc>>,
    /// Notification tier used for the last notification, if set.
    pub tier: Option<String>,
}

impl Default for NotifiedSnapshot {
    fn default() -> Self {
        Self {
            indicators: canonical_indicator_snapshot(),
            timestamp: None,
            tier: None,
        }
    }
}

/// Full per-ticker memory file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerMemory {
    /// Ticker symbol associated with this memory file.
    pub symbol: String,
    /// Retained prediction history.
    pub predictions: Vec<Prediction>,
    /// Persisted indicator weights and thresholds.
    pub weights: Weights,
    /// Snapshot from the most recent notification.
    pub last_notified: NotifiedSnapshot,
    /// Per-indicator tunable parameters — LLM-adjustable each cycle.
    #[serde(default)]
    pub indicator_config: IndicatorConfig,
}

impl TickerMemory {
    /// Create a fresh memory for a ticker with no history.
    pub fn new(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            predictions: Vec::new(),
            weights: Weights::default(),
            last_notified: NotifiedSnapshot::default(),
            indicator_config: IndicatorConfig::default(),
        }
    }
}

// ─── File I/O ────────────────────────────────────────────────────────────────

/// Build the file path for a ticker's memory file.
fn memory_path(memory_dir: &str, symbol: &str) -> PathBuf {
    Path::new(memory_dir).join(format!("{symbol}.json"))
}

/// Load ticker memory from disk. Returns a fresh default if the file doesn't
/// exist or is corrupted (cold start / corruption recovery).
pub fn load_memory(memory_dir: &str, symbol: &str) -> TickerMemory {
    let path = memory_path(memory_dir, symbol);

    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str::<TickerMemory>(&contents) {
            Ok(mem) => {
                info!(symbol, predictions = mem.predictions.len(), "Loaded memory");
                mem
            }
            Err(e) => {
                warn!(symbol, error = %e, "Corrupt memory file — starting fresh");
                TickerMemory::new(symbol)
            }
        },
        Err(_) => {
            info!(symbol, "No memory file — cold start");
            TickerMemory::new(symbol)
        }
    }
}

/// Save ticker memory atomically (write to temp file, then rename).
pub fn save_memory(
    memory_dir: &str,
    mem: &TickerMemory,
) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    let dir = Path::new(memory_dir);
    std::fs::create_dir_all(dir)?;

    let path = memory_path(memory_dir, &mem.symbol);
    let tmp_path = path.with_extension("json.tmp");

    let json = serde_json::to_string_pretty(mem)?;
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &path)?;

    info!(symbol = %mem.symbol, "Saved memory");
    Ok(())
}

/// Check if the stored indicator schema matches the current pipeline.
///
/// Compares the canonical tracked key set from the most recent prediction's
/// snapshot against the provided `current_keys`. Nullable values do not affect
/// this check: inactive/unavailable outputs must remain present as stable keys.
/// If the key set itself differs (indicator added/removed), clears predictions
/// and weights (KB is retained — it's ticker personality, not schema-dependent).
///
/// Returns `true` if a reset was performed, `false` if matching or empty.
pub fn check_stored_schema(mem: &mut TickerMemory, current_keys: &[&str]) -> bool {
    // No predictions → nothing to compare against, skip
    let stored_keys = match mem.predictions.last() {
        Some(pred) => {
            let mut keys: Vec<String> = pred.indicators.keys().cloned().collect();
            keys.sort();
            keys
        }
        None => return false,
    };

    let mut current_sorted: Vec<String> = current_keys.iter().map(|s| s.to_string()).collect();
    current_sorted.sort();

    if stored_keys == current_sorted {
        return false; // Matching schema
    }

    // Schema mismatch — clear predictions and weights
    info!(
        symbol = %mem.symbol,
        stored = ?stored_keys,
        current = ?current_sorted,
        "Schema mismatch: indicator key set changed. Clearing predictions and weights."
    );
    mem.predictions.clear();
    mem.weights.values.clear();
    true
}

/// Returns `true` when a trade plan is structurally eligible for directional
/// scoring: `LONG`/`SHORT` direction, timeframe `>= 4h`, finite positive
/// `entry`/`target`/`stop` with `LONG: stop < entry < target` or
/// `SHORT: target < entry < stop` and reward distance `>=` risk distance.
///
/// Mirrors `scoring::structural_levels` without depending on scoring so
/// retention stays self-contained. Stored `outcome` is ignored here; callers
/// combine this with `outcome.is_none()` to detect unresolved work.
fn trade_plan_structurally_eligible(plan: &TradePlan) -> bool {
    let direction: algotrap::prelude::Direction = match plan.direction.parse() {
        Ok(direction) => direction,
        Err(_) => return false,
    };
    let is_long = match direction {
        algotrap::prelude::Direction::Long => true,
        algotrap::prelude::Direction::Short => false,
        algotrap::prelude::Direction::None => return false,
    };
    let timeframe = match plan.timeframe {
        Some(timeframe) => timeframe,
        None => return false,
    };
    if timeframe.weight() < algotrap::prelude::Timeframe::H4.weight() {
        return false;
    }
    let (entry, target, stop) = match (plan.entry, plan.target, plan.stop) {
        (Some(entry), Some(target), Some(stop)) => (entry, target, stop),
        _ => return false,
    };
    for value in [entry, target, stop] {
        if !value.is_finite() || value <= 0.0 {
            return false;
        }
    }
    if is_long {
        if !(stop < entry && entry < target) {
            return false;
        }
        if (target - entry) < (entry - stop) {
            return false;
        }
    } else {
        if !(target < entry && entry < stop) {
            return false;
        }
        if (entry - target) < (stop - entry) {
            return false;
        }
    }
    true
}

/// Returns `true` when a prediction still has pending directional work: at
/// least one structurally eligible plan whose `outcome` is `None`.
///
/// Legacy plans with missing timeframe, `WAIT` direction, sub-4h timeframes,
/// or malformed levels are not eligible and therefore never protect a record.
fn prediction_has_unresolved_eligible_plan(pred: &Prediction) -> bool {
    pred.trade_plans
        .iter()
        .any(|plan| plan.outcome.is_none() && trade_plan_structurally_eligible(plan))
}

/// Append a prediction to memory, enforcing the sliding window limit while
/// protecting unresolved directional work.
///
/// Retention rules (deterministic):
/// - Chronological (insertion) order is always preserved; eviction only
///   removes entries, never reorders.
/// - When over `max_predictions`, evict the oldest terminal/unscorable record
///   first (a record without any unresolved eligible plan). This includes
///   scored history, all-terminal plans (even all-`Ambiguous` unscored), and
///   unscorable records such as `WAIT`, sub-4h, malformed, or legacy
///   missing-timeframe plans.
/// - Predictions with at least one structurally eligible unresolved plan
///   (`LONG`/`SHORT`, timeframe `>= 4h`, valid levels, `outcome: None`) are
///   never evicted merely to satisfy `max_predictions`.
/// - If every retained record is unresolved, all records are retained even
///   when that exceeds `max_predictions` (no data loss for pending work).
///   Settled history itself is therefore bounded to `max_predictions` whenever
///   any evictable record exists.
pub fn append_prediction(mem: &mut TickerMemory, pred: Prediction, max_predictions: usize) {
    mem.predictions.push(pred);
    while mem.predictions.len() > max_predictions {
        let Some(evictable) = mem
            .predictions
            .iter()
            .position(|existing| !prediction_has_unresolved_eligible_plan(existing))
        else {
            // All remaining records are unresolved eligible — retain them.
            break;
        };
        mem.predictions.remove(evictable);
    }
}

/// Apply weight guardrails: clamp values to [min, max] and rate-limit change.
pub fn apply_weight_guardrails(
    current: &HashMap<String, f64>,
    proposed: &HashMap<String, f64>,
    weight_min: f64,
    weight_max: f64,
    rate_limit: f64,
) -> HashMap<String, f64> {
    proposed
        .iter()
        .map(|(key, &new_val)| {
            let old_val = current.get(key).copied().unwrap_or(new_val);
            // Rate-limit: cap the change per cycle
            let clamped_delta = (new_val - old_val).clamp(-rate_limit, rate_limit);
            let adjusted = (old_val + clamped_delta).clamp(weight_min, weight_max);
            (key.clone(), adjusted)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT_NAMES: &[TelegramOutputName] = &[
        TelegramOutputName::Atr,
        TelegramOutputName::VolumeSma,
        TelegramOutputName::Ema200,
        TelegramOutputName::BiasReversion,
        TelegramOutputName::NeutralRevrsi,
        TelegramOutputName::BullishRevrsi,
        TelegramOutputName::BearishRevrsi,
        TelegramOutputName::AtrUpperband,
        TelegramOutputName::AtrLowerband,
        TelegramOutputName::Rssi,
        TelegramOutputName::RssiMa,
        TelegramOutputName::StructurePower,
        TelegramOutputName::StructurePowerSma,
        TelegramOutputName::AtrPercent,
        TelegramOutputName::AtrReversionPercent,
        TelegramOutputName::BandReversion,
        TelegramOutputName::Sharpe,
        TelegramOutputName::BodyRatio,
        TelegramOutputName::IsAtrGap,
        TelegramOutputName::Leverage,
    ];

    fn sample_prediction(summary: &str) -> Prediction {
        let mut indicators = canonical_indicator_snapshot();
        indicators.insert("rssi".into(), Some(55.0));
        indicators.insert("close".into(), Some(100.0));

        Prediction {
            timestamp: Utc::now(),
            confidence: 65.0,
            direction: algotrap::prelude::Direction::Long,
            summary: summary.into(),
            trade_plans: vec![],
            indicators,
            outcome_score: Some(0.8),
        }
    }

    fn sample_memory(symbol: &str) -> TickerMemory {
        let mut mem = TickerMemory::new(symbol);
        mem.weights.values.insert("rssi".into(), 0.30);
        let mut notified = canonical_indicator_snapshot();
        notified.insert("rssi".into(), Some(48.0));
        mem.last_notified = NotifiedSnapshot {
            indicators: notified,
            timestamp: Some(
                DateTime::parse_from_rfc3339("2026-03-15T12:34:56Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            tier: Some("tier-1".into()),
        };
        append_prediction(&mut mem, sample_prediction("fixture"), 8);
        mem
    }

    #[test]
    fn test_cold_start_memory() {
        let mem = TickerMemory::new("BTC-USDT");
        assert_eq!(mem.symbol, "BTC-USDT");
        assert!(mem.predictions.is_empty());
        assert!(mem.weights.values.is_empty());
        assert!((mem.weights.significance_threshold - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn test_append_prediction_sliding_window() {
        let mut mem = TickerMemory::new("BTC-USDT");
        for i in 0..10 {
            let pred = Prediction {
                timestamp: Utc::now(),
                confidence: i as f64 * 10.0,
                direction: algotrap::prelude::Direction::Long,
                summary: format!("pred {i}"),
                trade_plans: vec![],
                indicators: HashMap::new(),
                outcome_score: None,
            };
            append_prediction(&mut mem, pred, 8);
        }
        assert_eq!(mem.predictions.len(), 8);
        // Oldest should have been evicted — first remaining is pred 2
        assert_eq!(mem.predictions[0].summary, "pred 2");
    }

    fn retention_base_time() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-03-15T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn unresolved_long_4h(summary: &str, timestamp: DateTime<Utc>) -> Prediction {
        Prediction {
            timestamp,
            confidence: 65.0,
            direction: algotrap::prelude::Direction::Long,
            summary: summary.into(),
            trade_plans: vec![TradePlan {
                label: "A".into(),
                direction: "LONG".into(),
                entry: Some(100.0),
                target: Some(110.0),
                stop: Some(95.0),
                rationale: "retention fixture".into(),
                timeframe: Some(algotrap::prelude::Timeframe::H4),
                outcome: None,
            }],
            indicators: HashMap::new(),
            outcome_score: None,
        }
    }

    fn terminal_long_4h(summary: &str, timestamp: DateTime<Utc>) -> Prediction {
        Prediction {
            timestamp,
            confidence: 65.0,
            direction: algotrap::prelude::Direction::Long,
            summary: summary.into(),
            trade_plans: vec![TradePlan {
                label: "A".into(),
                direction: "LONG".into(),
                entry: Some(100.0),
                target: Some(110.0),
                stop: Some(95.0),
                rationale: "retention fixture".into(),
                timeframe: Some(algotrap::prelude::Timeframe::H4),
                outcome: Some(TradePlanOutcome {
                    kind: TradePlanOutcomeKind::TakeProfit,
                    entry_hit_at: Some(timestamp),
                    resolved_at: timestamp,
                    resolution_timeframe: algotrap::prelude::Timeframe::H4,
                    lowest_reached: 95.0,
                    highest_reached: 112.0,
                }),
            }],
            indicators: HashMap::new(),
            outcome_score: Some(1.0),
        }
    }

    fn legacy_missing_timeframe(summary: &str, timestamp: DateTime<Utc>) -> Prediction {
        Prediction {
            timestamp,
            confidence: 65.0,
            direction: algotrap::prelude::Direction::Long,
            summary: summary.into(),
            trade_plans: vec![TradePlan {
                label: "A".into(),
                direction: "LONG".into(),
                entry: Some(100.0),
                target: Some(110.0),
                stop: Some(95.0),
                rationale: "legacy without timeframe".into(),
                timeframe: None,
                outcome: None,
            }],
            indicators: HashMap::new(),
            outcome_score: None,
        }
    }

    #[test]
    fn test_append_preserves_unresolved_4h_beyond_cap() {
        let mut mem = TickerMemory::new("BTC-USDT");
        let base = retention_base_time();
        append_prediction(&mut mem, unresolved_long_4h("protected-0", base), 8);
        for i in 1..=10 {
            let timestamp = base + chrono::Duration::seconds(i64::from(i) * 60);
            append_prediction(
                &mut mem,
                terminal_long_4h(&format!("settled-{i}"), timestamp),
                8,
            );
        }
        assert!(
            mem.predictions.iter().any(|p| p.summary == "protected-0"),
            "unresolved 4h prediction must survive more than 8 appends"
        );
        assert_eq!(mem.predictions.len(), 8);
    }

    #[test]
    fn test_append_evicts_oldest_terminal_first() {
        let mut mem = TickerMemory::new("BTC-USDT");
        let base = retention_base_time();
        for i in 0..8 {
            let timestamp = base + chrono::Duration::seconds(i64::from(i) * 60);
            append_prediction(
                &mut mem,
                terminal_long_4h(&format!("settled-{i}"), timestamp),
                8,
            );
        }
        let overflow_at = base + chrono::Duration::seconds(8 * 60);
        append_prediction(&mut mem, terminal_long_4h("settled-8", overflow_at), 8);
        assert_eq!(mem.predictions.len(), 8);
        assert!(
            !mem.predictions.iter().any(|p| p.summary == "settled-0"),
            "oldest terminal record must be evicted first"
        );
        let summaries: Vec<&str> = mem.predictions.iter().map(|p| p.summary.as_str()).collect();
        assert_eq!(
            summaries,
            vec![
                "settled-1",
                "settled-2",
                "settled-3",
                "settled-4",
                "settled-5",
                "settled-6",
                "settled-7",
                "settled-8",
            ]
        );
    }

    #[test]
    fn test_append_legacy_missing_timeframe_is_cap_eligible() {
        let mut mem = TickerMemory::new("BTC-USDT");
        let base = retention_base_time();
        append_prediction(&mut mem, unresolved_long_4h("protected", base), 8);
        let legacy_at = base + chrono::Duration::seconds(60);
        append_prediction(&mut mem, legacy_missing_timeframe("legacy", legacy_at), 8);
        for i in 2..=9 {
            let timestamp = base + chrono::Duration::seconds(i64::from(i) * 60);
            append_prediction(
                &mut mem,
                terminal_long_4h(&format!("settled-{i}"), timestamp),
                8,
            );
        }
        assert!(
            mem.predictions.iter().any(|p| p.summary == "protected"),
            "unresolved 4h record must be retained"
        );
        assert!(
            !mem.predictions.iter().any(|p| p.summary == "legacy"),
            "legacy record without timeframe must remain cap-eligible even with outcome None"
        );
    }

    #[test]
    fn test_append_preserves_chronological_order_with_mixed_history() {
        let mut mem = TickerMemory::new("BTC-USDT");
        let base = retention_base_time();
        append_prediction(&mut mem, unresolved_long_4h("protected-0", base), 8);
        for i in 1..=10 {
            let timestamp = base + chrono::Duration::seconds(i64::from(i) * 60);
            append_prediction(
                &mut mem,
                terminal_long_4h(&format!("settled-{i}"), timestamp),
                8,
            );
        }
        assert_eq!(mem.predictions.len(), 8);
        let mut previous = None;
        for pred in &mem.predictions {
            if let Some(previous_at) = previous {
                assert!(
                    pred.timestamp > previous_at,
                    "chronological order must be preserved after protected retention"
                );
            }
            previous = Some(pred.timestamp);
        }
        assert_eq!(mem.predictions[0].summary, "protected-0");
        assert_eq!(mem.predictions[1].summary, "settled-4");
        assert_eq!(mem.predictions[7].summary, "settled-10");
    }

    #[test]
    fn test_append_retains_all_when_every_record_is_unresolved() {
        let mut mem = TickerMemory::new("BTC-USDT");
        let base = retention_base_time();
        for i in 0..10 {
            let timestamp = base + chrono::Duration::seconds(i64::from(i) * 60);
            append_prediction(
                &mut mem,
                unresolved_long_4h(&format!("pending-{i}"), timestamp),
                8,
            );
        }
        assert_eq!(
            mem.predictions.len(),
            10,
            "all-unresolved history must be retained without data loss even beyond cap"
        );
        let summaries: Vec<&str> = mem.predictions.iter().map(|p| p.summary.as_str()).collect();
        assert_eq!(
            summaries,
            vec![
                "pending-0",
                "pending-1",
                "pending-2",
                "pending-3",
                "pending-4",
                "pending-5",
                "pending-6",
                "pending-7",
                "pending-8",
                "pending-9",
            ]
        );
    }

    #[test]
    fn test_weight_guardrails() {
        let current = HashMap::from([("rssi".into(), 0.30), ("structure_power".into(), 0.20)]);
        let proposed = HashMap::from([
            ("rssi".into(), 0.50),            // wants +0.20, should be limited
            ("structure_power".into(), 0.10), // wants -0.10, should be limited
        ]);

        let result = apply_weight_guardrails(&current, &proposed, 0.05, 0.50, 0.05);

        // rssi: 0.30 + 0.05 = 0.35 (rate limited)
        assert!((result["rssi"] - 0.35).abs() < f64::EPSILON);
        // structure_power: 0.20 - 0.05 = 0.15 (rate limited)
        assert!((result["structure_power"] - 0.15).abs() < f64::EPSILON);
    }

    #[test]
    fn test_weight_guardrails_clamps_bounds() {
        let current = HashMap::from([("rssi".into(), 0.04)]);
        let proposed = HashMap::from([("rssi".into(), 0.01)]);

        let result = apply_weight_guardrails(&current, &proposed, 0.05, 0.50, 0.05);

        // 0.04 - 0.03 → clamped delta to -0.05 → 0.04 + (-0.03 clamped to -0.03) = but
        // also the result must be >= 0.05
        assert!(result["rssi"] >= 0.05);
    }

    #[test]
    fn test_save_load_roundtrip() {
        let dir = std::env::temp_dir().join("telegrambot_test_memory");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap();

        let mut mem = TickerMemory::new("TEST-USDT");
        mem.weights.values.insert("rssi".into(), 0.30);
        append_prediction(
            &mut mem,
            Prediction {
                timestamp: Utc::now(),
                confidence: 65.0,
                direction: algotrap::prelude::Direction::Long,
                summary: "test".into(),
                trade_plans: vec![],
                indicators: HashMap::from([("rssi".into(), Some(55.0))]),
                outcome_score: None,
            },
            8,
        );

        save_memory(dir_str, &mem).unwrap();
        let loaded = load_memory(dir_str, "TEST-USDT");

        assert_eq!(loaded.predictions.len(), 1);
        assert!((loaded.weights.values["rssi"] - 0.30).abs() < f64::EPSILON);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_stored_schema_keys_match() {
        let mut mem = TickerMemory::new("TEST");
        append_prediction(
            &mut mem,
            Prediction {
                timestamp: Utc::now(),
                confidence: 50.0,
                direction: algotrap::prelude::Direction::Long,
                summary: "t".into(),
                trade_plans: vec![],
                indicators: HashMap::from([
                    ("rssi".into(), Some(50.0)),
                    ("close".into(), Some(100.0)),
                ]),
                outcome_score: None,
            },
            8,
        );
        let reset = check_stored_schema(&mut mem, &["rssi", "close"]);
        assert!(!reset);
        assert_eq!(mem.predictions.len(), 1); // retained
    }

    #[test]
    fn test_stored_schema_key_added() {
        let mut mem = TickerMemory::new("TEST");
        mem.weights.values.insert("rssi".into(), 0.5);
        append_prediction(
            &mut mem,
            Prediction {
                timestamp: Utc::now(),
                confidence: 50.0,
                direction: algotrap::prelude::Direction::Long,
                summary: "t".into(),
                trade_plans: vec![],
                indicators: HashMap::from([("rssi".into(), Some(50.0))]),
                outcome_score: Some(0.8),
            },
            8,
        );
        let reset = check_stored_schema(&mut mem, &["rssi", "sharpe"]);
        assert!(reset);
        assert!(mem.predictions.is_empty()); // cleared
        assert!(mem.weights.values.is_empty()); // cleared
    }

    #[test]
    fn test_stored_schema_key_removed() {
        let mut mem = TickerMemory::new("TEST");
        append_prediction(
            &mut mem,
            Prediction {
                timestamp: Utc::now(),
                confidence: 50.0,
                direction: algotrap::prelude::Direction::Long,
                summary: "t".into(),
                trade_plans: vec![],
                indicators: HashMap::from([
                    ("rssi".into(), Some(50.0)),
                    ("deprecated_indicator".into(), Some(1.0)),
                ]),
                outcome_score: None,
            },
            8,
        );
        // deprecated_indicator removed from pipeline
        let reset = check_stored_schema(&mut mem, &["rssi"]);
        assert!(reset);
        assert!(mem.predictions.is_empty());
    }

    #[test]
    fn test_stored_schema_empty_predictions() {
        let mut mem = TickerMemory::new("TEST");
        let reset = check_stored_schema(&mut mem, &["rssi", "close"]);
        assert!(!reset); // No predictions = nothing to compare, skip
    }

    #[test]
    fn test_old_numeric_snapshot_json_deserializes_to_some_values() {
        let payload = serde_json::json!({
            "symbol": "BTC-USDT",
            "predictions": [{
                "timestamp": "2026-03-15T12:34:56Z",
                "confidence": 65.0,
                "direction": "LONG",
                "summary": "legacy",
                "trade_plans": [],
                "indicators": {
                    "rssi": 55.0,
                    "close": 100.0
                },
                "outcome_score": null
            }],
            "weights": {
                "values": {},
                "significance_threshold": 0.25
            },
            "last_notified": {
                "indicators": {
                    "rssi": 48.0,
                    "close": 99.0
                },
                "timestamp": null,
                "tier": null
            },
            "indicator_config": {
                "periods": {},
                "outputs": {},
                "gap_zones": {}
            }
        });

        let mem: TickerMemory = serde_json::from_value(payload).unwrap();

        assert_eq!(mem.predictions[0].indicators["rssi"], Some(55.0));
        assert_eq!(mem.predictions[0].indicators["close"], Some(100.0));
        assert_eq!(mem.last_notified.indicators["rssi"], Some(48.0));
        assert_eq!(mem.last_notified.indicators["close"], Some(99.0));
        for key in CANONICAL_TRACKED_INDICATOR_KEYS {
            assert!(mem.predictions[0].indicators.contains_key(*key));
            assert!(mem.last_notified.indicators.contains_key(*key));
        }
    }

    #[test]
    fn test_nullable_snapshot_roundtrip_preserves_nulls() {
        let mut mem = TickerMemory::new("BTC-USDT");
        mem.last_notified.indicators.insert("rssi".into(), None);
        mem.last_notified
            .indicators
            .insert("close".into(), Some(100.0));
        let mut indicators = canonical_indicator_snapshot();
        indicators.insert("rssi".into(), None);
        indicators.insert("close".into(), Some(100.0));
        mem.predictions.push(Prediction {
            timestamp: Utc::now(),
            confidence: 65.0,
            direction: algotrap::prelude::Direction::Long,
            summary: "nullable".into(),
            trade_plans: vec![],
            indicators,
            outcome_score: None,
        });

        let json = serde_json::to_value(&mem).unwrap();
        assert!(json["predictions"][0]["indicators"]["rssi"].is_null());
        assert!(json["last_notified"]["indicators"]["rssi"].is_null());

        let roundtrip: TickerMemory = serde_json::from_value(json).unwrap();
        assert_eq!(roundtrip.predictions[0].indicators["rssi"], None);
        assert_eq!(roundtrip.predictions[0].indicators["close"], Some(100.0));
        assert_eq!(roundtrip.last_notified.indicators["rssi"], None);
    }

    #[test]
    fn test_stored_schema_keys_match_with_unavailable_outputs() {
        let mut mem = TickerMemory::new("TEST");
        let mut indicators = canonical_indicator_snapshot();
        indicators.insert("rssi".into(), Some(50.0));
        indicators.insert("structure_power".into(), Some(1.0));
        indicators.insert("band_reversion".into(), Some(0.1));
        indicators.insert("atr_percent".into(), Some(0.005));
        indicators.insert("sharpe".into(), None);
        indicators.insert("close".into(), Some(100.0));
        append_prediction(
            &mut mem,
            Prediction {
                timestamp: Utc::now(),
                confidence: 50.0,
                direction: algotrap::prelude::Direction::Long,
                summary: "t".into(),
                trade_plans: vec![],
                indicators,
                outcome_score: None,
            },
            8,
        );

        let reset = check_stored_schema(
            &mut mem,
            &[
                "rssi",
                "structure_power",
                "band_reversion",
                "atr_percent",
                "sharpe",
                "close",
            ],
        );

        assert!(!reset);
        assert_eq!(mem.predictions.len(), 1);
    }

    #[test]
    fn test_indicator_config_defaults() {
        let ic = IndicatorConfig::default();
        assert_eq!(ic.periods.volume_ema.clamped() as usize, 20);
        assert_eq!(ic.periods.ema.clamped() as usize, 200);
        assert_eq!(ic.periods.rsi.clamped() as usize, 14);
        assert_eq!(ic.periods.reverse_rsi.clamped() as usize, 14);
        assert_eq!(ic.periods.atr.clamped() as usize, 42);
        assert_eq!(ic.periods.rsi_smooth.clamped() as usize, 9);
        assert_eq!(ic.periods.structure.clamped() as usize, 9);
        assert_eq!(ic.gap_zones.max_zones.value, 16.0);
        assert_eq!(ic.gap_zones.max_zones.min, 1.0);
        assert_eq!(ic.gap_zones.max_zones.max, 32.0);
        assert_eq!(ic.gap_zones.max_zones.clamped() as usize, 16);
        assert_eq!(ic.gap_zones.body_ratio_threshold.value, 0.618);
        assert_eq!(ic.gap_zones.body_ratio_threshold.min, 0.0);
        assert_eq!(ic.gap_zones.body_ratio_threshold.max, 1.0);
        assert_eq!(ic.gap_zones.atr_band_multiplier.value, 1.618);
        assert_eq!(ic.gap_zones.atr_band_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_band_multiplier.max, 5.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, 1.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.max, 5.0);
        for output in OUTPUT_NAMES {
            assert!(
                output.state(&ic.outputs).active,
                "expected {} active by default",
                output.persisted_name()
            );
        }
        assert_eq!(ic.active_count(), OUTPUT_NAMES.len());
        assert!(ic.dormant_roster().is_empty());
    }

    #[test]
    fn test_indicator_config_new_schema_serde_roundtrip() {
        let mut ic = IndicatorConfig::default();
        ic.periods.atr.value = 55.0;
        ic.periods.rsi.value = 18.0;
        ic.gap_zones.max_zones.value = 20.0;
        ic.gap_zones.body_ratio_threshold.value = 0.7;
        ic.gap_zones.atr_band_multiplier.value = 2.0;
        ic.gap_zones.atr_gap_multiplier.value = 1.2;
        ic.outputs.rssi = OutputState {
            active: false,
            inactive_cycles: 4,
        };
        ic.outputs.rssi_ma = OutputState {
            active: false,
            inactive_cycles: 4,
        };
        ic.outputs.volume_sma = OutputState {
            active: false,
            inactive_cycles: 2,
        };

        let json = serde_json::to_string(&ic).unwrap();
        assert!(json.contains("\"periods\""));
        assert!(json.contains("\"outputs\""));
        assert!(json.contains("\"gap_zones\""));
        assert!(!json.contains("\"indicators\""));

        let decoded: IndicatorConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.periods, ic.periods);
        assert_eq!(decoded.outputs, ic.outputs);
        assert_eq!(decoded.gap_zones, ic.gap_zones);
        assert!(!decoded.outputs.rssi.active);
        assert_eq!(decoded.outputs.rssi.inactive_cycles, 4);
    }

    #[test]
    fn test_indicator_config_migrates_old_grouped_schema() {
        let legacy = serde_json::json!({
            "indicators": {
                "rssi": {
                    "period": {"value": 21.0, "min": 5.0, "max": 50.0},
                    "smooth": {"value": 12.0, "min": 3.0, "max": 30.0},
                    "active": false,
                    "inactive_cycles": 4
                },
                "structure_power": {
                    "smooth": {"value": 11.0, "min": 3.0, "max": 30.0},
                    "active": false,
                    "inactive_cycles": 2
                },
                "atr": {
                    "period": {"value": 30.0, "min": 10.0, "max": 100.0},
                    "active": false,
                    "inactive_cycles": 7
                },
                "revrsi": {
                    "period": {"value": 18.0, "min": 5.0, "max": 50.0},
                    "active": false,
                    "inactive_cycles": 5
                },
                "bias_reversion": {
                    "smooth": {"value": 6.0, "min": 3.0, "max": 30.0},
                    "active": false,
                    "inactive_cycles": 1
                },
                "sharpe": {
                    "period": {"value": 250.0, "min": 50.0, "max": 500.0},
                    "active": false,
                    "inactive_cycles": 8
                },
                "ema200": {
                    "period": {"value": 210.0, "min": 50.0, "max": 500.0},
                    "active": false,
                    "inactive_cycles": 9
                },
                "gap_zones": {
                    "period": {"value": 28.0, "min": 14.0, "max": 56.0},
                    "smooth": {"value": 35.0, "min": 10.0, "max": 100.0},
                    "min_trust": {"value": 0.55, "min": 0.0, "max": 0.9},
                    "active": false,
                    "inactive_cycles": 6
                }
                // NOTE: legacy `min_trust` above is intentionally retained in the
                // fixture to prove it deserializes silently and is dropped.
            }
        });

        let ic: IndicatorConfig = serde_json::from_value(legacy).unwrap();

        assert_eq!(ic.periods.rsi.value, 21.0);
        assert_eq!(ic.periods.rsi_smooth.value, 12.0);
        assert_eq!(ic.periods.structure.value, 11.0);
        assert_eq!(ic.periods.atr.value, 30.0);
        assert_eq!(ic.periods.reverse_rsi.value, 18.0);
        assert_eq!(ic.periods.bias.value, 6.0);
        assert_eq!(ic.periods.sharpe.value, 250.0);
        assert_eq!(ic.periods.ema.value, 210.0);
        assert_eq!(ic.gap_zones.max_zones.value, 32.0);
        assert_eq!(ic.gap_zones.max_zones.min, 1.0);
        assert_eq!(ic.gap_zones.max_zones.max, 32.0);
        assert_eq!(ic.gap_zones.body_ratio_threshold.value, 0.618);
        assert_eq!(ic.gap_zones.body_ratio_threshold.min, 0.0);
        assert_eq!(ic.gap_zones.body_ratio_threshold.max, 1.0);
        assert_eq!(ic.gap_zones.atr_band_multiplier.value, 1.618);
        assert_eq!(ic.gap_zones.atr_band_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_band_multiplier.max, 5.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, 1.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.max, 5.0);
        assert_eq!(ic.periods.atr.clamped() as usize, 30);

        for output in LEGACY_RSSI_OUTPUTS {
            let state = TelegramOutputName::from_persisted_name(output)
                .unwrap()
                .state(&ic.outputs);
            assert!(!state.active);
            assert_eq!(state.inactive_cycles, 4);
        }
        for output in LEGACY_STRUCTURE_OUTPUTS {
            let state = TelegramOutputName::from_persisted_name(output)
                .unwrap()
                .state(&ic.outputs);
            assert!(!state.active);
            assert_eq!(state.inactive_cycles, 2);
        }
        for output in LEGACY_ATR_OUTPUTS {
            let state = TelegramOutputName::from_persisted_name(output)
                .unwrap()
                .state(&ic.outputs);
            assert!(!state.active);
            assert_eq!(state.inactive_cycles, 7);
        }
        for output in LEGACY_REVRSI_OUTPUTS {
            let state = TelegramOutputName::from_persisted_name(output)
                .unwrap()
                .state(&ic.outputs);
            assert!(!state.active);
            assert_eq!(state.inactive_cycles, 5);
        }
        for output in LEGACY_GAP_ZONE_OUTPUTS {
            let state = TelegramOutputName::from_persisted_name(output)
                .unwrap()
                .state(&ic.outputs);
            assert!(!state.active);
            assert_eq!(state.inactive_cycles, 6);
        }
        assert!(!ic.outputs.bias_reversion.active);
        assert!(!ic.outputs.sharpe.active);
        assert!(!ic.outputs.ema200.active);
        assert!(ic.outputs.volume_sma.active);
        assert!(ic.outputs.atr_reversion_percent.active);
        assert!(ic.outputs.band_reversion.active);
        assert!(ic.outputs.leverage.active);
    }

    #[test]
    fn test_old_ticker_memory_migration_preserves_unrelated_state() {
        let original = sample_memory("BTC-USDT");
        let mut stored = serde_json::to_value(&original).unwrap();
        stored["indicator_config"] = serde_json::json!({
            "indicators": {
                "rssi": {
                    "period": {"value": 17.0, "min": 5.0, "max": 50.0},
                    "smooth": {"value": 10.0, "min": 3.0, "max": 30.0},
                    "active": false,
                    "inactive_cycles": 3
                },
                "atr": {
                    "period": {"value": 33.0, "min": 10.0, "max": 100.0},
                    "active": true,
                    "inactive_cycles": 0
                }
            }
        });

        let migrated: TickerMemory = serde_json::from_value(stored).unwrap();

        assert_eq!(migrated.symbol, original.symbol);
        assert_eq!(migrated.predictions.len(), original.predictions.len());
        assert_eq!(
            migrated.predictions[0].summary,
            original.predictions[0].summary
        );
        assert_eq!(migrated.weights.values, original.weights.values);
        assert_eq!(
            migrated.weights.significance_threshold,
            original.weights.significance_threshold
        );
        assert_eq!(
            migrated.last_notified.indicators,
            original.last_notified.indicators
        );
        assert_eq!(
            migrated.last_notified.timestamp,
            original.last_notified.timestamp
        );
        assert_eq!(migrated.last_notified.tier, original.last_notified.tier);
        assert_eq!(migrated.indicator_config.periods.rsi.clamped() as usize, 17);
        assert!(!migrated.indicator_config.outputs.rssi.active);
        assert!(!migrated.indicator_config.outputs.rssi_ma.active);

        let saved = serde_json::to_value(&migrated).unwrap();
        assert!(saved["indicator_config"].get("periods").is_some());
        assert!(saved["indicator_config"].get("outputs").is_some());
        assert!(saved["indicator_config"].get("gap_zones").is_some());
        assert!(saved["indicator_config"].get("indicators").is_none());
    }

    #[test]
    fn test_indicator_config_missing_fields_default_safely() {
        let ic: IndicatorConfig = serde_json::from_value(serde_json::json!({
            "periods": {
                "atr": {"value": 64.0, "min": 10.0, "max": 100.0}
            }
        }))
        .unwrap();

        assert_eq!(ic.periods.atr.clamped() as usize, 64);
        assert_eq!(ic.periods.rsi.clamped() as usize, 14);
        assert_eq!(ic.gap_zones.max_zones.clamped() as usize, 16);
        assert_eq!(ic.gap_zones.body_ratio_threshold.value, 0.618);
        assert_eq!(ic.gap_zones.atr_band_multiplier.value, 1.618);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, 1.0);
        for output in OUTPUT_NAMES {
            assert!(
                output.state(&ic.outputs).active,
                "expected {} active from default fill",
                output.persisted_name()
            );
        }
    }

    #[test]
    fn test_indicator_proposal_serde_parses_typed_variants() {
        let period: IndicatorProposal = serde_json::from_value(serde_json::json!({
            "target": "period",
            "name": "rsi",
            "value": 18.0
        }))
        .unwrap();
        assert_eq!(
            period,
            IndicatorProposal::Period {
                name: PeriodName::Rsi,
                value: 18.0,
            }
        );

        let output: IndicatorProposal = serde_json::from_value(serde_json::json!({
            "target": "output",
            "name": "rssi",
            "active": false
        }))
        .unwrap();
        assert_eq!(
            output,
            IndicatorProposal::Output {
                name: TelegramOutputName::Rssi,
                active: false,
            }
        );

        let gap_zones: IndicatorProposal = serde_json::from_value(serde_json::json!({
            "target": "gap_zones",
            "max_zones": 40.0
        }))
        .unwrap();
        assert_eq!(
            gap_zones,
            IndicatorProposal::GapZones {
                max_zones: Some(40.0),
                body_ratio_threshold: None,
                atr_band_multiplier: None,
                atr_gap_multiplier: None,
            }
        );

        let gap_threshold: IndicatorProposal = serde_json::from_value(serde_json::json!({
            "target": "gap_zones",
            "body_ratio_threshold": 0.7
        }))
        .unwrap();
        assert_eq!(
            gap_threshold,
            IndicatorProposal::GapZones {
                max_zones: None,
                body_ratio_threshold: Some(0.7),
                atr_band_multiplier: None,
                atr_gap_multiplier: None,
            }
        );

        let gap_band: IndicatorProposal = serde_json::from_value(serde_json::json!({
            "target": "gap_zones",
            "atr_band_multiplier": 2.0
        }))
        .unwrap();
        assert_eq!(
            gap_band,
            IndicatorProposal::GapZones {
                max_zones: None,
                body_ratio_threshold: None,
                atr_band_multiplier: Some(2.0),
                atr_gap_multiplier: None,
            }
        );

        let gap_multiplier: IndicatorProposal = serde_json::from_value(serde_json::json!({
            "target": "gap_zones",
            "atr_gap_multiplier": 1.2
        }))
        .unwrap();
        assert_eq!(
            gap_multiplier,
            IndicatorProposal::GapZones {
                max_zones: None,
                body_ratio_threshold: None,
                atr_band_multiplier: None,
                atr_gap_multiplier: Some(1.2),
            }
        );
    }

    #[test]
    fn test_indicator_proposal_serde_rejects_unknown_target_or_name() {
        let unknown_target = serde_json::from_value::<IndicatorProposal>(serde_json::json!({
            "target": "group",
            "name": "rsi",
            "value": 14.0
        }))
        .unwrap_err()
        .to_string();
        assert!(unknown_target.contains("unknown variant"));

        let unknown_period_name = serde_json::from_value::<IndicatorProposal>(serde_json::json!({
            "target": "period",
            "name": "made_up_period",
            "value": 14.0
        }))
        .unwrap_err()
        .to_string();
        assert!(unknown_period_name.contains("unknown variant"));

        let unknown_output_name = serde_json::from_value::<IndicatorProposal>(serde_json::json!({
            "target": "output",
            "name": "made_up_output",
            "active": true
        }))
        .unwrap_err()
        .to_string();
        assert!(unknown_output_name.contains("unknown variant"));
    }

    #[test]
    fn test_indicator_config_rate_limited_period_proposal() {
        let mut ic = IndicatorConfig::default();
        // RSSI period is 14.0. Requesting 100 should be rate-limited to +30% = 14 * 1.3 = 18.2
        let proposed = [IndicatorProposal::Period {
            name: PeriodName::Rsi,
            value: 100.0,
        }];
        ic.apply_proposals(&proposed);
        assert!((ic.periods.rsi.value - 18.2).abs() < 1e-10);
    }

    #[test]
    fn test_indicator_config_independent_output_activation_and_min_active_guardrail() {
        let mut ic = IndicatorConfig::default();
        let proposed = [IndicatorProposal::Output {
            name: TelegramOutputName::Rssi,
            active: false,
        }];
        ic.apply_proposals(&proposed);
        assert!(!ic.outputs.rssi.active);
        assert!(ic.outputs.rssi_ma.active);

        let all_off: Vec<IndicatorProposal> = TelegramOutputName::ALL
            .into_iter()
            .map(|name| IndicatorProposal::Output {
                name,
                active: false,
            })
            .collect();
        ic.apply_proposals(&all_off);
        assert_eq!(ic.active_count(), 2, "Min-2-active guardrail failed");
    }

    #[test]
    fn test_indicator_config_blocks_deactivation_when_only_minimum_active_remain() {
        let mut ic = IndicatorConfig::default();
        for output in TelegramOutputName::ALL {
            ic.set_output_state(output, false);
        }
        ic.outputs.atr.active = true;
        ic.outputs.rssi.active = true;

        ic.apply_proposals(&[IndicatorProposal::Output {
            name: TelegramOutputName::Rssi,
            active: false,
        }]);

        assert!(ic.outputs.rssi.active);
        assert_eq!(ic.active_count(), 2);
    }

    #[test]
    fn test_indicator_config_active_count_and_dormancy_typed_outputs() {
        let mut ic = IndicatorConfig::default();
        let proposed = vec![
            IndicatorProposal::Output {
                name: TelegramOutputName::VolumeSma,
                active: false,
            },
            IndicatorProposal::Output {
                name: TelegramOutputName::Leverage,
                active: false,
            },
            IndicatorProposal::Output {
                name: TelegramOutputName::Rssi,
                active: false,
            },
            IndicatorProposal::Output {
                name: TelegramOutputName::RssiMa,
                active: false,
            },
        ];
        ic.apply_proposals(&proposed);
        assert_eq!(ic.active_count(), OUTPUT_NAMES.len() - 4);
        assert!(!ic.outputs.volume_sma.active);
        assert!(!ic.outputs.leverage.active);
        assert!(!ic.outputs.rssi.active);
        assert!(!ic.outputs.rssi_ma.active);
        ic.tick_dormant();
        ic.tick_dormant();
        let dormant = ic.dormant_roster();
        for output in ["volume_sma", "leverage", "rssi", "rssi_ma"] {
            let entry = dormant.iter().find(|(name, _)| *name == output).unwrap();
            assert_eq!(entry.1, 2);
        }
    }

    #[test]
    fn test_indicator_config_non_finite_proposals_are_ignored() {
        let mut ic = IndicatorConfig::default();
        let original_rsi = ic.periods.rsi.value;
        let original_max_zones = ic.gap_zones.max_zones.value;
        let original_threshold = ic.gap_zones.body_ratio_threshold.value;
        let original_band = ic.gap_zones.atr_band_multiplier.value;
        let original_gap = ic.gap_zones.atr_gap_multiplier.value;

        let proposed = vec![
            IndicatorProposal::Period {
                name: PeriodName::Rsi,
                value: f64::NAN,
            },
            IndicatorProposal::GapZones {
                max_zones: Some(f64::INFINITY),
                body_ratio_threshold: Some(f64::NAN),
                atr_band_multiplier: Some(f64::INFINITY),
                atr_gap_multiplier: Some(f64::NAN),
            },
        ];

        ic.apply_proposals(&proposed);

        assert_eq!(ic.periods.rsi.value, original_rsi);
        assert_eq!(ic.gap_zones.max_zones.value, original_max_zones);
        assert_eq!(ic.gap_zones.body_ratio_threshold.value, original_threshold);
        assert_eq!(ic.gap_zones.atr_band_multiplier.value, original_band);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, original_gap);
    }

    #[test]
    fn test_legacy_prediction_missing_trade_plans_outcome_score_timeframe_outcome_loads() {
        let payload = serde_json::json!({
            "symbol": "BTC-USDT",
            "predictions": [
                {
                    "timestamp": "2026-03-15T12:34:56Z",
                    "confidence": 65.0,
                    "direction": "LONG",
                    "summary": "legacy without new fields",
                    "indicators": {
                        "rssi": 55.0,
                        "close": 100.0
                    }
                },
                {
                    "timestamp": "2026-03-16T12:34:56Z",
                    "confidence": 42.0,
                    "direction": "SHORT",
                    "summary": "legacy plan without metadata",
                    "trade_plans": [
                        {
                            "label": "A",
                            "direction": "LONG",
                            "entry": 100.0,
                            "target": 110.0,
                            "stop": 95.0,
                            "rationale": "legacy plan"
                        }
                    ],
                    "indicators": {
                        "rssi": 48.0,
                        "close": 99.0
                    },
                    "outcome_score": 0.8
                }
            ],
            "weights": {
                "values": {},
                "significance_threshold": 0.25
            },
            "last_notified": {
                "indicators": {
                    "rssi": 48.0,
                    "close": 99.0
                },
                "timestamp": null,
                "tier": null
            },
            "indicator_config": {
                "periods": {},
                "outputs": {},
                "gap_zones": {}
            }
        });

        let mem: TickerMemory = serde_json::from_value(payload).unwrap();

        assert!(mem.predictions[0].trade_plans.is_empty());
        assert_eq!(mem.predictions[0].outcome_score, None);
        assert_eq!(mem.predictions[1].outcome_score, Some(0.8));
        assert_eq!(mem.predictions[1].trade_plans.len(), 1);
        assert_eq!(mem.predictions[1].trade_plans[0].timeframe, None);
        assert_eq!(mem.predictions[1].trade_plans[0].outcome, None);
    }

    #[test]
    fn test_trade_plan_timeframe_outcome_roundtrip() {
        let outcome = TradePlanOutcome {
            kind: TradePlanOutcomeKind::TakeProfit,
            entry_hit_at: Some(
                DateTime::parse_from_rfc3339("2026-03-16T10:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            resolved_at: DateTime::parse_from_rfc3339("2026-03-16T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            resolution_timeframe: algotrap::prelude::Timeframe::H4,
            lowest_reached: 99.0,
            highest_reached: 112.0,
        };
        let plan = TradePlan {
            label: "A".into(),
            direction: "LONG".into(),
            entry: Some(100.0),
            target: Some(110.0),
            stop: Some(95.0),
            rationale: "roundtrip".into(),
            timeframe: Some(algotrap::prelude::Timeframe::H4),
            outcome: Some(outcome),
        };
        let mut indicators = canonical_indicator_snapshot();
        indicators.insert("rssi".into(), Some(55.0));
        let pred = Prediction {
            timestamp: DateTime::parse_from_rfc3339("2026-03-15T12:34:56Z")
                .unwrap()
                .with_timezone(&Utc),
            confidence: 70.0,
            direction: algotrap::prelude::Direction::Long,
            summary: "roundtrip".into(),
            trade_plans: vec![plan],
            indicators,
            outcome_score: Some(1.0),
        };

        let json = serde_json::to_value(&pred).unwrap();
        let decoded: Prediction = serde_json::from_value(json).unwrap();

        assert_eq!(decoded.trade_plans.len(), 1);
        assert_eq!(
            decoded.trade_plans[0].timeframe,
            Some(algotrap::prelude::Timeframe::H4)
        );
        assert_eq!(decoded.trade_plans[0].outcome, pred.trade_plans[0].outcome);
        assert_eq!(decoded.outcome_score, Some(1.0));
    }

    #[test]
    fn test_existing_outcome_scores_retained_through_migration() {
        let payload = serde_json::json!({
            "timestamp": "2026-03-15T12:34:56Z",
            "confidence": 65.0,
            "direction": "LONG",
            "summary": "scored",
            "trade_plans": [],
            "indicators": {
                "rssi": 55.0,
                "close": 100.0
            },
            "outcome_score": 0.8
        });

        let pred: Prediction = serde_json::from_value(payload).unwrap();
        assert_eq!(pred.outcome_score, Some(0.8));

        let json = serde_json::to_value(&pred).unwrap();
        let decoded: Prediction = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.outcome_score, Some(0.8));
    }

    #[test]
    fn test_outcome_kind_snake_case_and_canonical_timeframe() {
        let outcome = TradePlanOutcome {
            kind: TradePlanOutcomeKind::StopLoss,
            entry_hit_at: None,
            resolved_at: DateTime::parse_from_rfc3339("2026-03-16T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            resolution_timeframe: algotrap::prelude::Timeframe::H4,
            lowest_reached: 90.0,
            highest_reached: 105.0,
        };
        let plan = TradePlan {
            label: "B".into(),
            direction: "SHORT".into(),
            entry: Some(100.0),
            target: Some(90.0),
            stop: Some(105.0),
            rationale: "wire".into(),
            timeframe: Some(algotrap::prelude::Timeframe::H4),
            outcome: Some(outcome),
        };

        let json = serde_json::to_value(&plan).unwrap();
        assert_eq!(json["timeframe"], serde_json::json!("4h"));
        assert_eq!(json["outcome"]["kind"], serde_json::json!("stop_loss"));
        assert_eq!(
            json["outcome"]["resolution_timeframe"],
            serde_json::json!("4h")
        );

        assert_eq!(
            serde_json::to_value(TradePlanOutcomeKind::TakeProfit).unwrap(),
            serde_json::json!("take_profit")
        );
        assert_eq!(
            serde_json::to_value(TradePlanOutcomeKind::Ambiguous).unwrap(),
            serde_json::json!("ambiguous")
        );

        let decoded: TradePlan = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.timeframe, Some(algotrap::prelude::Timeframe::H4));
        assert_eq!(
            decoded.outcome.as_ref().unwrap().kind,
            TradePlanOutcomeKind::StopLoss
        );
    }

    #[test]
    fn test_gap_zone_budget_default_and_bounds() {
        let ic = IndicatorConfig::default();
        assert_eq!(ic.gap_zones.max_zones.value, 16.0);
        assert_eq!(ic.gap_zones.max_zones.min, 1.0);
        assert_eq!(ic.gap_zones.max_zones.max, 32.0);
        let mut probe = ic.gap_zones.max_zones.clone();
        probe.value = 0.0;
        assert_eq!(probe.clamped(), 1.0);
        probe.value = 100.0;
        assert_eq!(probe.clamped(), 32.0);
        probe.value = 20.0;
        assert_eq!(probe.clamped(), 20.0);
    }

    #[test]
    fn test_gap_zone_body_ratio_threshold_default_and_bounds() {
        let ic = IndicatorConfig::default();
        assert_eq!(ic.gap_zones.body_ratio_threshold.value, 0.618);
        assert_eq!(ic.gap_zones.body_ratio_threshold.min, 0.0);
        assert_eq!(ic.gap_zones.body_ratio_threshold.max, 1.0);
        let mut probe = ic.gap_zones.body_ratio_threshold.clone();
        probe.value = -0.5;
        assert_eq!(probe.clamped(), 0.0);
        probe.value = 1.5;
        assert_eq!(probe.clamped(), 1.0);
        probe.value = 0.7;
        assert_eq!(probe.clamped(), 0.7);
    }

    #[test]
    fn test_gap_zone_legacy_migration_clamps_to_new_budget() {
        for (persisted, expected) in [(50.0, 32.0), (5.0, 5.0), (100.0, 32.0)] {
            let legacy = serde_json::json!({
                "indicators": {
                    "gap_zones": {
                        "smooth": {"value": persisted, "min": 10.0, "max": 100.0},
                        "min_trust": {"value": 0.3, "min": 0.0, "max": 0.9},
                        "active": true,
                        "inactive_cycles": 0
                    }
                }
            });
            let ic: IndicatorConfig = serde_json::from_value(legacy).unwrap();
            assert_eq!(
                ic.gap_zones.max_zones.value, expected,
                "persisted {persisted} must clamp to new [1,32] as {expected}"
            );
            assert_eq!(ic.gap_zones.max_zones.min, 1.0);
            assert_eq!(ic.gap_zones.max_zones.max, 32.0);
            assert_eq!(ic.gap_zones.body_ratio_threshold.value, 0.618);
            assert_eq!(ic.gap_zones.body_ratio_threshold.min, 0.0);
            assert_eq!(ic.gap_zones.body_ratio_threshold.max, 1.0);
            assert_eq!(ic.gap_zones.atr_band_multiplier.value, 1.618);
            assert_eq!(ic.gap_zones.atr_band_multiplier.min, 0.5);
            assert_eq!(ic.gap_zones.atr_band_multiplier.max, 5.0);
            assert_eq!(ic.gap_zones.atr_gap_multiplier.value, 1.0);
            assert_eq!(ic.gap_zones.atr_gap_multiplier.min, 0.5);
            assert_eq!(ic.gap_zones.atr_gap_multiplier.max, 5.0);
        }
    }

    #[test]
    fn test_gap_zone_new_schema_ignores_legacy_min_trust_key() {
        let ic: IndicatorConfig = serde_json::from_value(serde_json::json!({
            "periods": {},
            "outputs": {},
            "gap_zones": {
                "max_zones": {"value": 20.0, "min": 1.0, "max": 32.0},
                "min_trust": {"value": 0.3, "min": 0.0, "max": 0.9}
            }
        }))
        .unwrap();
        assert_eq!(ic.gap_zones.max_zones.clamped() as usize, 20);
        assert_eq!(ic.gap_zones.body_ratio_threshold.value, 0.618);
        assert_eq!(ic.gap_zones.atr_band_multiplier.value, 1.618);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, 1.0);
    }

    #[test]
    fn test_gap_zone_missing_threshold_defaults_to_0618() {
        let ic: IndicatorConfig = serde_json::from_value(serde_json::json!({
            "periods": {},
            "outputs": {},
            "gap_zones": {
                "max_zones": {"value": 20.0, "min": 1.0, "max": 32.0}
            }
        }))
        .unwrap();
        assert_eq!(ic.gap_zones.body_ratio_threshold.value, 0.618);
        assert_eq!(ic.gap_zones.body_ratio_threshold.min, 0.0);
        assert_eq!(ic.gap_zones.body_ratio_threshold.max, 1.0);
        assert_eq!(ic.gap_zones.atr_band_multiplier.value, 1.618);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, 1.0);

        let roundtrip: IndicatorConfig =
            serde_json::from_value(serde_json::to_value(&ic).unwrap()).unwrap();
        assert_eq!(
            roundtrip.gap_zones.body_ratio_threshold,
            ic.gap_zones.body_ratio_threshold
        );
        assert_eq!(
            roundtrip.gap_zones.atr_band_multiplier,
            ic.gap_zones.atr_band_multiplier
        );
        assert_eq!(
            roundtrip.gap_zones.atr_gap_multiplier,
            ic.gap_zones.atr_gap_multiplier
        );
    }

    #[test]
    fn test_gap_zone_proposal_applies_and_clamps_without_min_trust() {
        let mut ic = IndicatorConfig::default();
        ic.apply_proposals(&[IndicatorProposal::GapZones {
            max_zones: Some(20.0),
            body_ratio_threshold: None,
            atr_band_multiplier: None,
            atr_gap_multiplier: None,
        }]);
        assert_eq!(ic.gap_zones.max_zones.value, 20.0);

        let mut high = IndicatorConfig::default();
        high.gap_zones.max_zones.value = 30.0;
        high.apply_proposals(&[IndicatorProposal::GapZones {
            max_zones: Some(100.0),
            body_ratio_threshold: None,
            atr_band_multiplier: None,
            atr_gap_multiplier: None,
        }]);
        assert_eq!(high.gap_zones.max_zones.value, 32.0);

        let mut low = IndicatorConfig::default();
        low.gap_zones.max_zones.value = 1.0;
        low.apply_proposals(&[IndicatorProposal::GapZones {
            max_zones: Some(0.0),
            body_ratio_threshold: None,
            atr_band_multiplier: None,
            atr_gap_multiplier: None,
        }]);
        assert_eq!(low.gap_zones.max_zones.value, 1.0);
    }

    #[test]
    fn test_gap_zone_threshold_proposal_rate_limited_like_max_zones() {
        let mut ic = IndicatorConfig::default();
        ic.apply_proposals(&[IndicatorProposal::GapZones {
            max_zones: None,
            body_ratio_threshold: Some(2.0),
            atr_band_multiplier: None,
            atr_gap_multiplier: None,
        }]);
        let expected = 0.618 * 1.3;
        assert!((ic.gap_zones.body_ratio_threshold.value - expected).abs() < 1e-10);

        let mut high = IndicatorConfig::default();
        high.gap_zones.body_ratio_threshold.value = 0.9;
        high.apply_proposals(&[IndicatorProposal::GapZones {
            max_zones: None,
            body_ratio_threshold: Some(2.0),
            atr_band_multiplier: None,
            atr_gap_multiplier: None,
        }]);
        assert_eq!(high.gap_zones.body_ratio_threshold.value, 1.0);

        let mut low = IndicatorConfig::default();
        low.gap_zones.body_ratio_threshold.value = 0.2;
        low.apply_proposals(&[IndicatorProposal::GapZones {
            max_zones: None,
            body_ratio_threshold: Some(0.0),
            atr_band_multiplier: None,
            atr_gap_multiplier: None,
        }]);
        assert!((low.gap_zones.body_ratio_threshold.value - 0.14).abs() < 1e-10);
    }

    #[test]
    fn test_gap_zone_proposal_ignores_legacy_min_trust_key() {
        let proposal: IndicatorProposal = serde_json::from_value(serde_json::json!({
            "target": "gap_zones",
            "max_zones": 20.0,
            "min_trust": 0.45
        }))
        .unwrap();
        assert_eq!(
            proposal,
            IndicatorProposal::GapZones {
                max_zones: Some(20.0),
                body_ratio_threshold: None,
                atr_band_multiplier: None,
                atr_gap_multiplier: None,
            }
        );
    }

    #[test]
    fn test_gap_zone_atr_multipliers_default_and_bounds() {
        let ic = IndicatorConfig::default();
        assert_eq!(ic.gap_zones.atr_band_multiplier.value, 1.618);
        assert_eq!(ic.gap_zones.atr_band_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_band_multiplier.max, 5.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, 1.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.max, 5.0);

        let mut band_probe = ic.gap_zones.atr_band_multiplier.clone();
        band_probe.value = 0.1;
        assert_eq!(band_probe.clamped(), 0.5);
        band_probe.value = 10.0;
        assert_eq!(band_probe.clamped(), 5.0);
        band_probe.value = 2.0;
        assert_eq!(band_probe.clamped(), 2.0);

        let mut gap_probe = ic.gap_zones.atr_gap_multiplier.clone();
        gap_probe.value = 0.1;
        assert_eq!(gap_probe.clamped(), 0.5);
        gap_probe.value = 10.0;
        assert_eq!(gap_probe.clamped(), 5.0);
        gap_probe.value = 1.2;
        assert_eq!(gap_probe.clamped(), 1.2);
    }

    #[test]
    fn test_gap_zone_atr_multipliers_missing_fields_default() {
        let ic: IndicatorConfig = serde_json::from_value(serde_json::json!({
            "periods": {},
            "outputs": {},
            "gap_zones": {}
        }))
        .unwrap();
        assert_eq!(ic.gap_zones.atr_band_multiplier.value, 1.618);
        assert_eq!(ic.gap_zones.atr_band_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_band_multiplier.max, 5.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, 1.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.max, 5.0);
    }

    #[test]
    fn test_old_gap_config_with_only_existing_fields_defaults_new_multipliers() {
        let ic: IndicatorConfig = serde_json::from_value(serde_json::json!({
            "periods": {},
            "outputs": {},
            "gap_zones": {
                "max_zones": {"value": 20.0, "min": 1.0, "max": 32.0},
                "body_ratio_threshold": {"value": 0.7, "min": 0.0, "max": 1.0}
            }
        }))
        .unwrap();
        assert_eq!(ic.gap_zones.max_zones.value, 20.0);
        assert_eq!(ic.gap_zones.body_ratio_threshold.value, 0.7);
        assert_eq!(ic.gap_zones.atr_band_multiplier.value, 1.618);
        assert_eq!(ic.gap_zones.atr_band_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_band_multiplier.max, 5.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, 1.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.max, 5.0);
    }

    #[test]
    fn test_gap_zone_atr_multipliers_serde_roundtrip() {
        let mut ic = IndicatorConfig::default();
        ic.gap_zones.atr_band_multiplier.value = 2.5;
        ic.gap_zones.atr_gap_multiplier.value = 0.8;

        let json = serde_json::to_value(&ic).unwrap();
        assert_eq!(
            json["gap_zones"]["atr_band_multiplier"]["value"].as_f64(),
            Some(2.5)
        );
        assert_eq!(
            json["gap_zones"]["atr_gap_multiplier"]["value"].as_f64(),
            Some(0.8)
        );

        let decoded: IndicatorConfig = serde_json::from_value(json).unwrap();
        assert_eq!(
            decoded.gap_zones.atr_band_multiplier,
            ic.gap_zones.atr_band_multiplier
        );
        assert_eq!(
            decoded.gap_zones.atr_gap_multiplier,
            ic.gap_zones.atr_gap_multiplier
        );
    }

    #[test]
    fn test_gap_zone_atr_multipliers_proposal_rate_limited_high_low() {
        let mut ic = IndicatorConfig::default();
        ic.apply_proposals(&[IndicatorProposal::GapZones {
            max_zones: None,
            body_ratio_threshold: None,
            atr_band_multiplier: Some(5.0),
            atr_gap_multiplier: Some(5.0),
        }]);
        assert!((ic.gap_zones.atr_band_multiplier.value - 1.618 * 1.3).abs() < 1e-10);
        assert!((ic.gap_zones.atr_gap_multiplier.value - 1.3).abs() < 1e-10);

        let mut high = IndicatorConfig::default();
        high.gap_zones.atr_band_multiplier.value = 4.5;
        high.gap_zones.atr_gap_multiplier.value = 4.5;
        high.apply_proposals(&[IndicatorProposal::GapZones {
            max_zones: None,
            body_ratio_threshold: None,
            atr_band_multiplier: Some(10.0),
            atr_gap_multiplier: Some(10.0),
        }]);
        assert_eq!(high.gap_zones.atr_band_multiplier.value, 5.0);
        assert_eq!(high.gap_zones.atr_gap_multiplier.value, 5.0);

        let mut low = IndicatorConfig::default();
        low.gap_zones.atr_band_multiplier.value = 0.6;
        low.gap_zones.atr_gap_multiplier.value = 0.6;
        low.apply_proposals(&[IndicatorProposal::GapZones {
            max_zones: None,
            body_ratio_threshold: None,
            atr_band_multiplier: Some(0.0),
            atr_gap_multiplier: Some(0.0),
        }]);
        assert_eq!(low.gap_zones.atr_band_multiplier.value, 0.5);
        assert_eq!(low.gap_zones.atr_gap_multiplier.value, 0.5);
    }

    #[test]
    fn test_gap_zone_atr_multipliers_non_finite_ignored() {
        let mut ic = IndicatorConfig::default();
        let original_band = ic.gap_zones.atr_band_multiplier.value;
        let original_gap = ic.gap_zones.atr_gap_multiplier.value;

        ic.apply_proposals(&[IndicatorProposal::GapZones {
            max_zones: None,
            body_ratio_threshold: None,
            atr_band_multiplier: Some(f64::NAN),
            atr_gap_multiplier: Some(f64::INFINITY),
        }]);

        assert_eq!(ic.gap_zones.atr_band_multiplier.value, original_band);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, original_gap);
    }

    #[test]
    fn test_gap_zone_atr_multipliers_legacy_has_no_mapping() {
        let legacy = serde_json::json!({
            "indicators": {
                "gap_zones": {
                    "period": {"value": 28.0, "min": 14.0, "max": 56.0},
                    "smooth": {"value": 20.0, "min": 1.0, "max": 32.0},
                    "min_trust": {"value": 0.55, "min": 0.0, "max": 0.9},
                    "active": true,
                    "inactive_cycles": 0
                }
            }
        });
        let ic: IndicatorConfig = serde_json::from_value(legacy).unwrap();
        assert_eq!(ic.gap_zones.atr_band_multiplier.value, 1.618);
        assert_eq!(ic.gap_zones.atr_band_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_band_multiplier.max, 5.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.value, 1.0);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.min, 0.5);
        assert_eq!(ic.gap_zones.atr_gap_multiplier.max, 5.0);
    }

    #[test]
    fn test_malformed_indicator_config_fails_clearly_without_touching_other_memory_fields() {
        let mem = sample_memory("ETH-USDT");
        let mut payload = serde_json::to_value(&mem).unwrap();
        payload["indicator_config"] = serde_json::json!({
            "outputs": {
                "rssi": 7
            }
        });
        let json = serde_json::to_string(&payload).unwrap();
        let source: serde_json::Value = serde_json::from_str(&json).unwrap();

        let error = serde_json::from_str::<TickerMemory>(&json)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("invalid indicator_config"),
            "unexpected error: {error}"
        );
        assert!(
            error.contains("indicator_config"),
            "unexpected error: {error}"
        );
        assert_eq!(source["predictions"].as_array().unwrap().len(), 1);
        assert_eq!(source["weights"]["values"]["rssi"].as_f64(), Some(0.30));
        assert_eq!(source["last_notified"]["tier"].as_str(), Some("tier-1"));
    }
}

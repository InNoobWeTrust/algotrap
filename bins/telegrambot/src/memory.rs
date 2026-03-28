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

// ─── Data Types ──────────────────────────────────────────────────────────────

/// A single trade plan option (A, B, or C).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradePlan {
    pub label: String,          // "A", "B", "C"
    pub direction: String,      // "LONG" | "SHORT" | "WAIT"
    pub entry: Option<f64>,
    pub target: Option<f64>,
    pub stop: Option<f64>,
    pub rationale: String,
}

/// A stored prediction from a single scan cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub timestamp: DateTime<Utc>,
    pub confidence: f64,
    pub direction: String,
    pub summary: String,
    pub trade_plans: Vec<TradePlan>,
    /// Indicator snapshot at time of prediction (for change detection).
    pub indicators: HashMap<String, f64>,
    /// Outcome score set after validation (None = not yet validated).
    pub outcome_score: Option<f64>,
}

/// Per-indicator weights tuned by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weights {
    pub values: HashMap<String, f64>,
    /// LLM-tuned significance threshold for change detection (seeded at 0.25).
    #[serde(default = "default_significance_threshold")]
    pub significance_threshold: f64,
}

fn default_significance_threshold() -> f64 {
    0.25
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSpec {
    pub value: f64,
    pub min: f64,
    pub max: f64,
}

impl ParamSpec {
    pub fn new(value: f64, min: f64, max: f64) -> Self {
        Self { value, min, max }
    }

    /// Clamp to bounds.
    pub fn clamped(&self) -> f64 {
        self.value.clamp(self.min, self.max)
    }
}

/// Tunable parameters for a single indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorParams {
    /// Optional period parameter (e.g., RSI period, ATR period).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<ParamSpec>,
    /// Optional smoothing parameter (e.g., EMA smooth window).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smooth: Option<ParamSpec>,
    /// Quality-filter threshold (e.g., min_trust for gap zones). Exempt from rate limiting.
    #[serde(default)]
    pub min_trust: Option<ParamSpec>,
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

/// Per-ticker indicator configuration, persisted and LLM-tunable.
///
/// Keys are indicator group names (e.g., "rssi", "atr", "structure_power").
/// OHLC is always-on and not part of this config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorConfig {
    pub indicators: HashMap<String, IndicatorParams>,
}

impl Default for IndicatorConfig {
    fn default() -> Self {
        let mut indicators = HashMap::new();

        indicators.insert("rssi".into(), IndicatorParams {
            period: Some(ParamSpec::new(14.0, 5.0, 50.0)),
            smooth: Some(ParamSpec::new(9.0, 3.0, 30.0)),
            min_trust: None,
            active: true,
            inactive_cycles: 0,
        });
        indicators.insert("structure_power".into(), IndicatorParams {
            period: None,
            smooth: Some(ParamSpec::new(9.0, 3.0, 30.0)),
            min_trust: None,
            active: true,
            inactive_cycles: 0,
        });
        indicators.insert("atr".into(), IndicatorParams {
            period: Some(ParamSpec::new(42.0, 10.0, 100.0)),
            smooth: None,
            min_trust: None,
            active: true,
            inactive_cycles: 0,
        });
        indicators.insert("ema200".into(), IndicatorParams {
            period: Some(ParamSpec::new(200.0, 50.0, 500.0)),
            smooth: None,
            min_trust: None,
            active: true,
            inactive_cycles: 0,
        });
        indicators.insert("sharpe".into(), IndicatorParams {
            period: Some(ParamSpec::new(200.0, 50.0, 500.0)),
            smooth: None,
            min_trust: None,
            active: true,
            inactive_cycles: 0,
        });
        // bias_reversion is a dependency of atr_reversion_percent, always active
        indicators.insert("bias_reversion".into(), IndicatorParams {
            period: None,
            smooth: Some(ParamSpec::new(9.0, 3.0, 30.0)),
            min_trust: None,
            active: true,
            inactive_cycles: 0,
        });
        // revrsi group inherits period from rssi config
        indicators.insert("revrsi".into(), IndicatorParams {
            period: Some(ParamSpec::new(14.0, 5.0, 50.0)),
            smooth: None,
            min_trust: None,
            active: true,
            inactive_cycles: 0,
        });
        // ATR Gap Zones — stateful indicator
        // period = atr_period, smooth = max_zones, min_trust = quality filter
        indicators.insert("gap_zones".into(), IndicatorParams {
            period: Some(ParamSpec::new(42.0, 14.0, 56.0)),
            smooth: Some(ParamSpec::new(50.0, 10.0, 100.0)), // max_zones
            min_trust: Some(ParamSpec::new(0.3, 0.0, 0.9)),
            active: true,
            inactive_cycles: 0,
        });

        Self { indicators }
    }
}

impl IndicatorConfig {
    /// Get a tunable period value for an indicator, falling back to default.
    pub fn period(&self, name: &str, default: usize) -> usize {
        self.indicators
            .get(name)
            .and_then(|p| p.period.as_ref())
            .map(|s| s.clamped() as usize)
            .unwrap_or(default)
    }

    /// Get a tunable smooth value for an indicator, falling back to default.
    pub fn smooth(&self, name: &str, default: usize) -> usize {
        self.indicators
            .get(name)
            .and_then(|p| p.smooth.as_ref())
            .map(|s| s.clamped() as usize)
            .unwrap_or(default)
    }

    /// Check if an indicator is active.
    pub fn is_active(&self, name: &str) -> bool {
        self.indicators
            .get(name)
            .map(|p| p.active)
            .unwrap_or(true) // Unknown indicators default to active
    }

    /// Count active derived indicators (excludes OHLC base tier).
    pub fn active_count(&self) -> usize {
        self.indicators.values().filter(|p| p.active).count()
    }

    /// Get the dormant roster: inactive indicators with their cycle counts.
    pub fn dormant_roster(&self) -> Vec<(&str, u32)> {
        self.indicators
            .iter()
            .filter(|(_, p)| !p.active)
            .map(|(name, p)| (name.as_str(), p.inactive_cycles))
            .collect()
    }

    /// Construct GapZoneParams from the gap_zones indicator config.
    pub fn gap_zone_params(&self) -> algotrap::ta::gap_zones::GapZoneParams {
        let defaults = algotrap::ta::gap_zones::GapZoneParams::default();
        algotrap::ta::gap_zones::GapZoneParams {
            atr_period: self.period("gap_zones", defaults.atr_period),
            max_zones: self.smooth("gap_zones", defaults.max_zones), // max_zones stored in smooth
            min_trust: self
                .indicators
                .get("gap_zones")
                .and_then(|p| p.min_trust.as_ref())
                .map(|s| s.clamped())
                .unwrap_or(defaults.min_trust),
        }
    }

    /// Get a tunable min_trust value for an indicator, falling back to default.
    pub fn min_trust(&self, name: &str, default: f64) -> f64 {
        self.indicators
            .get(name)
            .and_then(|p| p.min_trust.as_ref())
            .map(|s| s.clamped())
            .unwrap_or(default)
    }

    /// Increment inactive_cycles for all dormant indicators.
    pub fn tick_dormant(&mut self) {
        for params in self.indicators.values_mut() {
            if !params.active {
                params.inactive_cycles += 1;
            }
        }
    }

    /// Apply LLM-proposed param changes with guardrails.
    ///
    /// - Range clamping: values clamped to [min, max]
    /// - Rate limiting: ±30% change per cycle (except exempt fields)
    /// - Min-2-active: cannot deactivate below 2 active derived indicators
    pub fn apply_proposed(
        &mut self,
        proposed: &HashMap<String, serde_json::Value>,
    ) {
        const RATE_LIMIT: f64 = 0.30;
        const MIN_ACTIVE: usize = 2;

        // Pre-compute active count to avoid borrow conflicts
        let mut active_count = self.indicators.values().filter(|p| p.active).count();

        for (name, value) in proposed {
            let Some(params) = self.indicators.get_mut(name) else {
                continue; // Unknown indicator, skip
            };

            // Handle active toggle
            if let Some(active) = value.get("active").and_then(|v| v.as_bool()) {
                if !active && params.active {
                    // Trying to deactivate — check min-2-active guardrail
                    if active_count > MIN_ACTIVE {
                        params.active = false;
                        params.inactive_cycles = 0;
                        active_count -= 1;
                    }
                    // else: silently reject (can't go below MIN_ACTIVE)
                } else if active && !params.active {
                    params.active = true;
                    params.inactive_cycles = 0;
                    active_count += 1;
                }
            }

            // Handle period tuning
            if let Some(new_val) = value.get("period").and_then(|v| v.as_f64()) {
                if let Some(ref mut spec) = params.period {
                    let old = spec.value;
                    let max_change = old * RATE_LIMIT;
                    let delta = (new_val - old).clamp(-max_change, max_change);
                    spec.value = (old + delta).clamp(spec.min, spec.max);
                }
            }

            // Handle smooth tuning
            if let Some(new_val) = value.get("smooth").and_then(|v| v.as_f64()) {
                if let Some(ref mut spec) = params.smooth {
                    let old = spec.value;
                    let max_change = old * RATE_LIMIT;
                    let delta = (new_val - old).clamp(-max_change, max_change);
                    spec.value = (old + delta).clamp(spec.min, spec.max);
                }
            }

            // Handle min_trust tuning (EXEMPT from rate limiting — quality filter)
            if let Some(new_val) = value.get("min_trust").and_then(|v| v.as_f64()) {
                if let Some(ref mut spec) = params.min_trust {
                    spec.value = new_val.clamp(spec.min, spec.max);
                }
            }
        }
    }
}

/// Snapshot of indicator values from the last notification (for delta detection).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotifiedSnapshot {
    pub indicators: HashMap<String, f64>,
    pub timestamp: Option<DateTime<Utc>>,
    pub tier: Option<String>,
}

/// Full per-ticker memory file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerMemory {
    pub symbol: String,
    pub predictions: Vec<Prediction>,
    pub weights: Weights,
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
/// Compares the indicator key set from the most recent prediction's snapshot
/// against the provided `current_keys`. If they differ (indicator added/removed),
/// clears predictions and weights (KB is retained — it's ticker personality, not
/// schema-dependent).
///
/// Returns `true` if a reset was performed, `false` if compatible or empty.
pub fn check_schema_compatibility(
    mem: &mut TickerMemory,
    current_keys: &[&str],
) -> bool {
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
        return false; // Compatible
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

/// Append a prediction to memory, enforcing the sliding window limit.
pub fn append_prediction(mem: &mut TickerMemory, pred: Prediction, max_predictions: usize) {
    mem.predictions.push(pred);
    // Evict oldest if over limit
    while mem.predictions.len() > max_predictions {
        mem.predictions.remove(0);
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
                direction: "LONG".into(),
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

    #[test]
    fn test_weight_guardrails() {
        let current = HashMap::from([("rssi".into(), 0.30), ("climax".into(), 0.20)]);
        let proposed = HashMap::from([
            ("rssi".into(), 0.50),   // wants +0.20, should be limited
            ("climax".into(), 0.10), // wants -0.10, should be limited
        ]);

        let result = apply_weight_guardrails(&current, &proposed, 0.05, 0.50, 0.05);

        // rssi: 0.30 + 0.05 = 0.35 (rate limited)
        assert!((result["rssi"] - 0.35).abs() < f64::EPSILON);
        // climax: 0.20 - 0.05 = 0.15 (rate limited)
        assert!((result["climax"] - 0.15).abs() < f64::EPSILON);
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
                direction: "LONG".into(),
                summary: "test".into(),
                trade_plans: vec![],
                indicators: HashMap::from([("rssi".into(), 55.0)]),
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
    fn test_schema_compat_keys_match() {
        let mut mem = TickerMemory::new("TEST");
        append_prediction(
            &mut mem,
            Prediction {
                timestamp: Utc::now(),
                confidence: 50.0,
                direction: "LONG".into(),
                summary: "t".into(),
                trade_plans: vec![],
                indicators: HashMap::from([
                    ("rssi".into(), 50.0),
                    ("close".into(), 100.0),
                ]),
                outcome_score: None,
            },
            8,
        );
        let reset = check_schema_compatibility(&mut mem, &["rssi", "close"]);
        assert!(!reset);
        assert_eq!(mem.predictions.len(), 1); // retained
    }

    #[test]
    fn test_schema_compat_key_added() {
        let mut mem = TickerMemory::new("TEST");
        mem.weights.values.insert("rssi".into(), 0.5);
        append_prediction(
            &mut mem,
            Prediction {
                timestamp: Utc::now(),
                confidence: 50.0,
                direction: "LONG".into(),
                summary: "t".into(),
                trade_plans: vec![],
                indicators: HashMap::from([("rssi".into(), 50.0)]),
                outcome_score: Some(0.8),
            },
            8,
        );
        let reset = check_schema_compatibility(&mut mem, &["rssi", "sharpe"]);
        assert!(reset);
        assert!(mem.predictions.is_empty()); // cleared
        assert!(mem.weights.values.is_empty()); // cleared
    }

    #[test]
    fn test_schema_compat_key_removed() {
        let mut mem = TickerMemory::new("TEST");
        append_prediction(
            &mut mem,
            Prediction {
                timestamp: Utc::now(),
                confidence: 50.0,
                direction: "LONG".into(),
                summary: "t".into(),
                trade_plans: vec![],
                indicators: HashMap::from([
                    ("rssi".into(), 50.0),
                    ("climax_signal".into(), 1.0),
                ]),
                outcome_score: None,
            },
            8,
        );
        // climax_signal removed from pipeline
        let reset = check_schema_compatibility(&mut mem, &["rssi"]);
        assert!(reset);
        assert!(mem.predictions.is_empty());
    }

    #[test]
    fn test_schema_compat_empty_predictions() {
        let mut mem = TickerMemory::new("TEST");
        let reset = check_schema_compatibility(&mut mem, &["rssi", "close"]);
        assert!(!reset); // No predictions = nothing to compare, skip
    }

    #[test]
    fn test_indicator_config_defaults() {
        let ic = IndicatorConfig::default();
        assert_eq!(ic.period("rssi", 999), 14);
        assert_eq!(ic.period("atr", 999), 42);
        assert_eq!(ic.smooth("rssi", 999), 9);
        assert!(ic.is_active("rssi"));
        assert_eq!(ic.active_count(), 8);
        assert!(ic.dormant_roster().is_empty());
    }

    #[test]
    fn test_indicator_config_rate_limited_tuning() {
        let mut ic = IndicatorConfig::default();
        // RSSI period is 14.0. Requesting 100 should be rate-limited to +30% = 14 * 1.3 = 18.2
        let proposed = HashMap::from([
            ("rssi".to_string(), serde_json::json!({"period": 100})),
        ]);
        ic.apply_proposed(&proposed);
        let new_period = ic.period("rssi", 999);
        assert!(new_period <= 19, "Rate limit should cap at ~18, got {new_period}");
        assert!(new_period >= 17, "Should increase, got {new_period}");
    }

    #[test]
    fn test_indicator_config_min_active_guardrail() {
        let mut ic = IndicatorConfig::default();
        // Try to deactivate all 7 indicators — should stop at 2 active
        let mut proposed = HashMap::new();
        for name in ["rssi", "structure_power", "atr", "ema200", "sharpe", "bias_reversion", "revrsi"] {
            proposed.insert(name.to_string(), serde_json::json!({"active": false}));
        }
        ic.apply_proposed(&proposed);
        assert!(ic.active_count() >= 2, "Min-2-active guardrail failed: {}", ic.active_count());
    }

    #[test]
    fn test_indicator_config_tick_dormant() {
        let mut ic = IndicatorConfig::default();
        // Deactivate one
        let proposed = HashMap::from([
            ("sharpe".to_string(), serde_json::json!({"active": false})),
        ]);
        ic.apply_proposed(&proposed);
        assert!(!ic.is_active("sharpe"));
        ic.tick_dormant();
        ic.tick_dormant();
        let dormant = ic.dormant_roster();
        let sharpe_entry = dormant.iter().find(|(n, _)| *n == "sharpe");
        assert_eq!(sharpe_entry.unwrap().1, 2);
    }
}

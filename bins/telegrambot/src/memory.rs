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
}

impl TickerMemory {
    /// Create a fresh memory for a ticker with no history.
    pub fn new(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            predictions: Vec::new(),
            weights: Weights::default(),
            last_notified: NotifiedSnapshot::default(),
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
}

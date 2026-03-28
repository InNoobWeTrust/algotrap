//! Scoring — tier engine, significant-change detection, and outcome validation.

use std::collections::HashMap;

// ─── Tier System ─────────────────────────────────────────────────────────────

/// Response tier based on confidence thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Alert,
    Watch,
    Silent,
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tier::Alert => write!(f, "ALERT"),
            Tier::Watch => write!(f, "WATCH"),
            Tier::Silent => write!(f, "SILENT"),
        }
    }
}

/// Determine the tier for a given confidence score.
pub fn classify_tier(confidence: f64, alert_threshold: f64, watch_threshold: f64) -> Tier {
    if confidence >= alert_threshold {
        Tier::Alert
    } else if confidence >= watch_threshold {
        Tier::Watch
    } else {
        Tier::Silent
    }
}

// ─── Significant-Change Detection ────────────────────────────────────────────

/// Compute the symmetric percentage delta between two values.
///
/// Formula: |new - old| / max(|old|, |new|, 1.0)
///
/// The floor of 1.0 prevents division-by-zero and handles zero-crossing
/// gracefully (e.g., structure_power from +1 to -1 = 100%).
pub fn symmetric_delta(old: f64, new: f64) -> f64 {
    let denominator = old.abs().max(new.abs()).max(1.0);
    (new - old).abs() / denominator
}

/// Check whether any key indicator has changed significantly.
///
/// Returns `(has_significant_change, max_delta)`.
pub fn detect_significant_change(
    old_indicators: &HashMap<String, f64>,
    new_indicators: &HashMap<String, f64>,
    key_indicators: &[String],
    threshold: f64,
) -> (bool, f64) {
    let mut max_delta: f64 = 0.0;

    for key in key_indicators {
        if let (Some(&old_val), Some(&new_val)) = (old_indicators.get(key), new_indicators.get(key))
        {
            let delta = symmetric_delta(old_val, new_val);
            max_delta = max_delta.max(delta);
        }
    }

    (max_delta >= threshold, max_delta)
}

/// Determine whether a notification should be sent based on tier change,
/// significant-change detection, time-based cooldown, and direction filter.
pub fn should_notify(
    current_tier: Tier,
    previous_tier: Option<&str>,
    has_significant_change: bool,
    last_notified_at: Option<chrono::DateTime<chrono::Utc>>,
    cooldown_secs: u64,
    direction: &str,
) -> bool {
    // Filter: Watch/Silent with direction NONE is never actionable
    if current_tier != Tier::Alert && direction.eq_ignore_ascii_case("NONE") {
        return false;
    }

    // Silent tier never notifies
    if current_tier == Tier::Silent {
        return false;
    }

    // Check cooldown: has enough time passed since last notification?
    let cooldown_expired = match last_notified_at {
        Some(ts) => {
            let elapsed = chrono::Utc::now() - ts;
            elapsed.num_seconds() >= cooldown_secs as i64
        }
        None => true, // Cold start — no prior notification
    };

    // Tier change (or cold start) bypasses cooldown
    let tier_str = current_tier.to_string();
    let tier_changed = previous_tier.map_or(true, |prev| prev != tier_str);
    if tier_changed {
        return true;
    }

    // Alert tier always notifies if cooldown expired
    if current_tier == Tier::Alert && cooldown_expired {
        return true;
    }

    // Watch tier: significant change AND cooldown must both pass
    if current_tier == Tier::Watch && has_significant_change && cooldown_expired {
        return true;
    }

    false
}

/// Parse the comma-separated change detection indicators config string.
pub fn parse_indicator_keys(config_str: &str) -> Vec<String> {
    config_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ─── Outcome Validation ─────────────────────────────────────────────────────

/// Compute outcome score for a prediction using direction-based composite
/// scoring.
///
/// Formula (LONG/SHORT):
///   `direction_match × (0.6 + magnitude_factor × 0.4)`
///   where `magnitude_factor = min(1.0, |Δprice| / atr)`
///
/// Wrong direction always scores 0.0 — magnitude only amplifies correct calls.
///
/// NONE direction:
///   - With ATR: 1.0 if `|Δprice| < 0.5 × atr` (market stayed flat), 0.0
///     otherwise
///   - Without ATR: 0.0 (can't verify flatness)
///
/// Falls back to binary scoring (1.0 or 0.0) when ATR is unavailable.
pub fn compute_outcome_score(
    direction: &str,
    prediction_price: f64,
    current_price: f64,
    atr: Option<f64>,
) -> f64 {
    let delta = current_price - prediction_price;
    let abs_delta = delta.abs();
    let dir = direction.to_uppercase();

    // NONE direction: scored conditionally against ATR
    if dir == "NONE" {
        return match atr {
            Some(atr_val) if atr_val > 0.0 => {
                if abs_delta < 0.5 * atr_val {
                    1.0 // Correctly identified no-trade
                } else {
                    0.0 // Missed a significant move
                }
            }
            _ => 0.0, // Can't verify without ATR
        };
    }

    // Determine if the prediction direction was correct
    let direction_correct = match dir.as_str() {
        "LONG" => delta > 0.0,
        "SHORT" => delta < 0.0,
        _ => false,
    };

    if !direction_correct {
        return 0.0; // Wrong direction always scores 0.0
    }

    // Direction was correct — compute composite with magnitude bonus
    match atr {
        Some(atr_val) if atr_val > 0.0 => {
            let magnitude_factor = (abs_delta / atr_val).min(1.0);
            0.6 + magnitude_factor * 0.4 // Range: [0.6, 1.0]
        }
        _ => 1.0, // Binary fallback — correct direction without ATR
    }
}

/// Compute direction accuracy across scored predictions.
///
/// Returns `(correct_count, total_scored, accuracy_pct)`.
/// A prediction is "correct" if its outcome score ≥ 0.5.
/// Unscored predictions (outcome_score = None) are excluded.
pub fn compute_direction_accuracy(
    predictions: &[crate::memory::Prediction],
) -> (usize, usize, f64) {
    let scored: Vec<f64> = predictions.iter().filter_map(|p| p.outcome_score).collect();

    let total = scored.len();
    if total == 0 {
        return (0, 0, 0.0);
    }

    let correct = scored.iter().filter(|&&s| s >= 0.5).count();
    let accuracy = correct as f64 / total as f64;

    (correct, total, accuracy)
}

/// Check if recent accuracy is critically low (< threshold over last `window` scored predictions).
///
/// Returns true if there are at least `window` scored predictions and accuracy < threshold.
/// This detects sustained poor performance that may indicate a regime change.
pub fn is_low_accuracy_streak(
    predictions: &[crate::memory::Prediction],
    window: usize,
    threshold: f64,
) -> bool {
    let recent_scored: Vec<_> = predictions
        .iter()
        .rev()
        .filter(|p| p.outcome_score.is_some())
        .take(window)
        .collect();

    if recent_scored.len() < window {
        return false;
    }

    let correct = recent_scored
        .iter()
        .filter(|p| p.outcome_score.unwrap_or(0.0) >= 0.5)
        .count();

    let accuracy = correct as f64 / recent_scored.len() as f64;
    accuracy < threshold
}

/// Reconstruct approximate ATR from indicator snapshot.
///
/// ATR ≈ close × atr_reversion_percent / 100
/// Returns None if either `close` or `atr_reversion_percent` is missing.
pub fn reconstruct_atr(indicators: &HashMap<String, f64>) -> Option<f64> {
    let close = indicators.get("close").copied()?;
    let atr_pct = indicators.get("atr_reversion_percent").copied()?;
    if close <= 0.0 || atr_pct <= 0.0 {
        return None;
    }
    Some(close * atr_pct / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_tier() {
        assert_eq!(classify_tier(85.0, 70.0, 40.0), Tier::Alert);
        assert_eq!(classify_tier(70.0, 70.0, 40.0), Tier::Alert);
        assert_eq!(classify_tier(55.0, 70.0, 40.0), Tier::Watch);
        assert_eq!(classify_tier(40.0, 70.0, 40.0), Tier::Watch);
        assert_eq!(classify_tier(39.9, 70.0, 40.0), Tier::Silent);
        assert_eq!(classify_tier(0.0, 70.0, 40.0), Tier::Silent);
    }

    #[test]
    fn test_symmetric_delta_normal() {
        // 50 → 35: |35-50|/max(50,35,1) = 15/50 = 0.30
        assert!((symmetric_delta(50.0, 35.0) - 0.30).abs() < f64::EPSILON);
    }

    #[test]
    fn test_symmetric_delta_zero_crossing() {
        // +1 → -1: |(-1)-1|/max(1,1,1) = 2/1 = 2.0 (200%)
        assert!((symmetric_delta(1.0, -1.0) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_symmetric_delta_from_zero() {
        // 0 → 0.5: |0.5-0|/max(0,0.5,1) = 0.5/1.0 = 0.5
        assert!((symmetric_delta(0.0, 0.5) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_symmetric_delta_symmetric() {
        // A→B should equal B→A
        assert!((symmetric_delta(50.0, 35.0) - symmetric_delta(35.0, 50.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_detect_significant_change() {
        let old = HashMap::from([("rssi".into(), 50.0), ("structure_power".into(), 0.0)]);
        let new = HashMap::from([("rssi".into(), 35.0), ("structure_power".into(), 1.0)]);
        let keys = vec!["rssi".into(), "structure_power".into()];

        let (changed, max_delta) = detect_significant_change(&old, &new, &keys, 0.20);
        assert!(changed);
        assert!(max_delta >= 0.30); // rssi delta = 30%
    }

    #[test]
    fn test_no_significant_change() {
        let old = HashMap::from([("rssi".into(), 50.0)]);
        let new = HashMap::from([("rssi".into(), 48.0)]);
        let keys = vec!["rssi".into()];

        let (changed, _) = detect_significant_change(&old, &new, &keys, 0.20);
        assert!(!changed); // 4% change < 20% threshold
    }

    #[test]
    fn test_should_notify_tier_change() {
        let past = chrono::Utc::now() - chrono::Duration::hours(2);
        assert!(should_notify(
            Tier::Alert,
            Some("WATCH"),
            false,
            Some(past),
            3600,
            "LONG"
        ));
        assert!(should_notify(
            Tier::Watch,
            Some("SILENT"),
            false,
            Some(past),
            3600,
            "LONG"
        ));
        // Tier change to Silent still doesn't notify (Silent never notifies)
        assert!(!should_notify(
            Tier::Silent,
            Some("ALERT"),
            false,
            Some(past),
            3600,
            "LONG"
        ));
    }

    #[test]
    fn test_should_notify_cold_start() {
        // No previous tier → always notify (unless NONE direction)
        assert!(should_notify(Tier::Watch, None, false, None, 3600, "LONG"));
    }

    #[test]
    fn test_should_notify_significant_change_in_watch() {
        let past = chrono::Utc::now() - chrono::Duration::hours(2);
        let recent = chrono::Utc::now() - chrono::Duration::minutes(5);
        // Significant change + cooldown expired → notify
        assert!(should_notify(
            Tier::Watch,
            Some("WATCH"),
            true,
            Some(past),
            3600,
            "LONG"
        ));
        // Significant change but still in cooldown → suppress
        assert!(!should_notify(
            Tier::Watch,
            Some("WATCH"),
            true,
            Some(recent),
            3600,
            "LONG"
        ));
        // No significant change + cooldown expired → suppress
        assert!(!should_notify(
            Tier::Watch,
            Some("WATCH"),
            false,
            Some(past),
            3600,
            "LONG"
        ));
    }

    #[test]
    fn test_should_notify_alert_with_cooldown() {
        let past = chrono::Utc::now() - chrono::Duration::hours(2);
        let recent = chrono::Utc::now() - chrono::Duration::minutes(5);
        // Alert + cooldown expired → notify
        assert!(should_notify(
            Tier::Alert,
            Some("ALERT"),
            false,
            Some(past),
            3600,
            "LONG"
        ));
        // Alert but in cooldown → suppress
        assert!(!should_notify(
            Tier::Alert,
            Some("ALERT"),
            false,
            Some(recent),
            3600,
            "LONG"
        ));
    }

    #[test]
    fn test_should_notify_silent_suppressed() {
        let past = chrono::Utc::now() - chrono::Duration::hours(2);
        assert!(!should_notify(
            Tier::Silent,
            Some("SILENT"),
            false,
            Some(past),
            3600,
            "LONG"
        ));
        assert!(!should_notify(
            Tier::Silent,
            Some("SILENT"),
            true,
            Some(past),
            3600,
            "LONG"
        ));
    }

    #[test]
    fn test_should_notify_direction_none_suppressed() {
        let past = chrono::Utc::now() - chrono::Duration::hours(2);
        // Watch + NONE direction → always suppress
        assert!(!should_notify(
            Tier::Watch,
            Some("WATCH"),
            true,
            Some(past),
            3600,
            "NONE"
        ));
        assert!(!should_notify(Tier::Watch, None, true, None, 3600, "NONE"));
        // Alert + NONE → still notifies (Alert overrides direction filter)
        assert!(should_notify(
            Tier::Alert,
            Some("WATCH"),
            false,
            Some(past),
            3600,
            "NONE"
        ));
    }

    #[test]
    fn test_parse_indicator_keys() {
        let keys = parse_indicator_keys("rssi,structure_power");
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], "rssi");
    }

    // ─── Outcome Scoring v2 Tests ────────────────────────────────────────────

    #[test]
    fn test_outcome_score_long_correct() {
        // LONG prediction, price went up, ATR = 500
        let score = compute_outcome_score("LONG", 87000.0, 87800.0, Some(500.0));
        // magnitude = min(1.0, 800/500) = 1.0
        // score = 1.0 × (0.6 + 1.0 × 0.4) = 1.0
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_long_correct_small_move() {
        // LONG prediction, price up slightly, ATR = 500
        let score = compute_outcome_score("LONG", 87000.0, 87100.0, Some(500.0));
        // magnitude = min(1.0, 100/500) = 0.2
        // score = 1.0 × (0.6 + 0.2 × 0.4) = 0.68
        assert!((score - 0.68).abs() < 1e-10);
    }

    #[test]
    fn test_outcome_score_long_wrong() {
        // LONG prediction, price went DOWN → 0.0
        let score = compute_outcome_score("LONG", 87000.0, 86500.0, Some(500.0));
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_short_correct() {
        // SHORT prediction, price went down
        let score = compute_outcome_score("SHORT", 87000.0, 86200.0, Some(500.0));
        // magnitude = min(1.0, 800/500) = 1.0
        // score = 1.0 × (0.6 + 1.0 × 0.4) = 1.0
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_short_wrong() {
        // SHORT prediction, price went UP → 0.0
        let score = compute_outcome_score("SHORT", 87000.0, 87500.0, Some(500.0));
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_none_flat_correct() {
        // NONE prediction, market stayed flat (|Δ| < 0.5 × ATR)
        let score = compute_outcome_score("NONE", 87000.0, 87100.0, Some(500.0));
        // |100| < 250 → correct no-trade → 1.0
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_none_missed_move() {
        // NONE prediction, significant move missed (|Δ| > 0.5 × ATR)
        let score = compute_outcome_score("NONE", 87000.0, 87600.0, Some(500.0));
        // |600| > 250 → missed move → 0.0
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_no_atr_correct() {
        // LONG prediction correct, no ATR → binary 1.0
        let score = compute_outcome_score("LONG", 87000.0, 87500.0, None);
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_no_atr_wrong() {
        // LONG prediction wrong, no ATR → 0.0
        let score = compute_outcome_score("LONG", 87000.0, 86500.0, None);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_none_no_atr() {
        // NONE without ATR → 0.0 (can't verify)
        let score = compute_outcome_score("NONE", 87000.0, 87100.0, None);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_direction_accuracy() {
        use chrono::Utc;
        let make_pred = |score: Option<f64>| crate::memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: "LONG".into(),
            summary: "test".into(),
            trade_plans: vec![],
            indicators: HashMap::new(),
            outcome_score: score,
        };

        let preds = vec![
            make_pred(Some(1.0)), // correct
            make_pred(Some(0.0)), // wrong
            make_pred(Some(0.8)), // correct
            make_pred(Some(0.0)), // wrong
            make_pred(Some(1.0)), // correct
            make_pred(Some(0.7)), // correct
            make_pred(Some(0.0)), // wrong
            make_pred(None),      // pending
        ];

        let (correct, total, accuracy) = compute_direction_accuracy(&preds);
        assert_eq!(correct, 4);
        assert_eq!(total, 7);
        assert!((accuracy - 4.0 / 7.0).abs() < 1e-10);
    }

    #[test]
    fn test_direction_accuracy_empty() {
        let (correct, total, accuracy) = compute_direction_accuracy(&[]);
        assert_eq!(correct, 0);
        assert_eq!(total, 0);
        assert!((accuracy - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_reconstruct_atr() {
        let indicators = HashMap::from([
            ("close".to_string(), 87000.0),
            ("atr_reversion_percent".to_string(), 0.575),
        ]);
        let atr = reconstruct_atr(&indicators).unwrap();
        // 87000 × 0.575 / 100 = 500.25
        assert!((atr - 500.25).abs() < 1e-10);
    }

    #[test]
    fn test_reconstruct_atr_missing() {
        let indicators = HashMap::from([("close".to_string(), 87000.0)]);
        assert!(reconstruct_atr(&indicators).is_none());
    }

    // ─── Low Accuracy Streak Tests (Scenario 13) ────────────────────────────

    #[test]
    fn test_is_low_accuracy_streak_insufficient_scored() {
        use chrono::Utc;
        let make_pred = |score: Option<f64>| crate::memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: "LONG".into(),
            summary: "test".into(),
            trade_plans: vec![],
            indicators: HashMap::new(),
            outcome_score: score,
        };

        // Only 3 scored predictions (need 5) — should return false
        let preds = vec![
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
            make_pred(None),
        ];
        assert!(!is_low_accuracy_streak(&preds, 5, 0.4));
    }

    #[test]
    fn test_is_low_accuracy_streak_all_wrong() {
        use chrono::Utc;
        let make_pred = |score: Option<f64>| crate::memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: "LONG".into(),
            summary: "test".into(),
            trade_plans: vec![],
            indicators: HashMap::new(),
            outcome_score: score,
        };

        // 5 scored predictions, all wrong (accuracy = 0%) — should return true
        let preds = vec![
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
        ];
        assert!(is_low_accuracy_streak(&preds, 5, 0.4));
    }

    #[test]
    fn test_is_low_accuracy_streak_above_threshold() {
        use chrono::Utc;
        let make_pred = |score: Option<f64>| crate::memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: "LONG".into(),
            summary: "test".into(),
            trade_plans: vec![],
            indicators: HashMap::new(),
            outcome_score: score,
        };

        // 5 scored predictions, 3 correct (accuracy = 60%) — should return false
        let preds = vec![
            make_pred(Some(1.0)),
            make_pred(Some(0.0)),
            make_pred(Some(0.8)),
            make_pred(Some(0.0)),
            make_pred(Some(0.7)),
        ];
        assert!(!is_low_accuracy_streak(&preds, 5, 0.4));
    }

    #[test]
    fn test_is_low_accuracy_streak_one_correct() {
        use chrono::Utc;
        let make_pred = |score: Option<f64>| crate::memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: "LONG".into(),
            summary: "test".into(),
            trade_plans: vec![],
            indicators: HashMap::new(),
            outcome_score: score,
        };

        // 5 scored predictions, 1 correct (accuracy = 20%) — should return true
        let preds = vec![
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
            make_pred(Some(0.8)),
        ];
        assert!(is_low_accuracy_streak(&preds, 5, 0.4));
    }

    #[test]
    fn test_is_low_accuracy_streak_at_threshold() {
        use chrono::Utc;
        let make_pred = |score: Option<f64>| crate::memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: "LONG".into(),
            summary: "test".into(),
            trade_plans: vec![],
            indicators: HashMap::new(),
            outcome_score: score,
        };

        // 5 scored predictions, 2 correct (accuracy = 40% = threshold) — should return false (< not <=)
        let preds = vec![
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
            make_pred(Some(0.0)),
            make_pred(Some(0.8)),
            make_pred(Some(0.7)),
        ];
        assert!(!is_low_accuracy_streak(&preds, 5, 0.4));
    }

    #[test]
    fn test_is_low_accuracy_streak_interleaved_unscored() {
        use chrono::Utc;
        let make_pred = |score: Option<f64>| crate::memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: "LONG".into(),
            summary: "test".into(),
            trade_plans: vec![],
            indicators: HashMap::new(),
            outcome_score: score,
        };

        // 10 predictions total, but only 5 are scored (all wrong), rest are unscored
        // Should look at the 5 scored ones and return true
        let preds = vec![
            make_pred(Some(0.0)),
            make_pred(None), // unscored
            make_pred(Some(0.1)),
            make_pred(None), // unscored
            make_pred(Some(0.2)),
            make_pred(None), // unscored
            make_pred(Some(0.0)),
            make_pred(None), // unscored
            make_pred(Some(0.1)),
            make_pred(None), // unscored (most recent)
        ];
        assert!(is_low_accuracy_streak(&preds, 5, 0.4));
    }

    #[test]
    fn test_is_low_accuracy_streak_empty() {
        let preds: Vec<crate::memory::Prediction> = vec![];
        assert!(!is_low_accuracy_streak(&preds, 5, 0.4));
    }
}

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
        if let (Some(&old_val), Some(&new_val)) =
            (old_indicators.get(key), new_indicators.get(key))
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

/// Compute outcome score for a prediction by checking how many trade plans
/// matched the actual price movement.
///
/// For each plan: +1 if direction matches reality (price crossed entry in the
/// right direction). Score = matching_plans / total_plans.
/// Returns 0.0 if no plans or no match.
pub fn compute_outcome_score(
    plans: &[crate::memory::TradePlan],
    prediction_price: f64,
    current_price: f64,
) -> f64 {
    if plans.is_empty() {
        return 0.0;
    }

    let actual_direction = if current_price > prediction_price {
        "LONG"
    } else if current_price < prediction_price {
        "SHORT"
    } else {
        return 0.0;
    };

    let matches: usize = plans
        .iter()
        .filter(|plan| {
            let plan_dir = plan.direction.to_uppercase();
            if plan_dir == "WAIT" {
                return false;
            }
            plan_dir == actual_direction
        })
        .count();

    matches as f64 / plans.len() as f64
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
        let old = HashMap::from([("rssi".into(), 50.0), ("climax".into(), 0.0)]);
        let new = HashMap::from([("rssi".into(), 35.0), ("climax".into(), 1.0)]);
        let keys = vec!["rssi".into(), "climax".into()];

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
        assert!(should_notify(Tier::Alert, Some("WATCH"), false, Some(past), 3600, "LONG"));
        assert!(should_notify(Tier::Watch, Some("SILENT"), false, Some(past), 3600, "LONG"));
        // Tier change to Silent still doesn't notify (Silent never notifies)
        assert!(!should_notify(Tier::Silent, Some("ALERT"), false, Some(past), 3600, "LONG"));
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
        assert!(should_notify(Tier::Watch, Some("WATCH"), true, Some(past), 3600, "LONG"));
        // Significant change but still in cooldown → suppress
        assert!(!should_notify(Tier::Watch, Some("WATCH"), true, Some(recent), 3600, "LONG"));
        // No significant change + cooldown expired → suppress
        assert!(!should_notify(Tier::Watch, Some("WATCH"), false, Some(past), 3600, "LONG"));
    }

    #[test]
    fn test_should_notify_alert_with_cooldown() {
        let past = chrono::Utc::now() - chrono::Duration::hours(2);
        let recent = chrono::Utc::now() - chrono::Duration::minutes(5);
        // Alert + cooldown expired → notify
        assert!(should_notify(Tier::Alert, Some("ALERT"), false, Some(past), 3600, "LONG"));
        // Alert but in cooldown → suppress
        assert!(!should_notify(Tier::Alert, Some("ALERT"), false, Some(recent), 3600, "LONG"));
    }

    #[test]
    fn test_should_notify_silent_suppressed() {
        let past = chrono::Utc::now() - chrono::Duration::hours(2);
        assert!(!should_notify(Tier::Silent, Some("SILENT"), false, Some(past), 3600, "LONG"));
        assert!(!should_notify(Tier::Silent, Some("SILENT"), true, Some(past), 3600, "LONG"));
    }

    #[test]
    fn test_should_notify_direction_none_suppressed() {
        let past = chrono::Utc::now() - chrono::Duration::hours(2);
        // Watch + NONE direction → always suppress
        assert!(!should_notify(Tier::Watch, Some("WATCH"), true, Some(past), 3600, "NONE"));
        assert!(!should_notify(Tier::Watch, None, true, None, 3600, "NONE"));
        // Alert + NONE → still notifies (Alert overrides direction filter)
        assert!(should_notify(Tier::Alert, Some("WATCH"), false, Some(past), 3600, "NONE"));
    }

    #[test]
    fn test_parse_indicator_keys() {
        let keys = parse_indicator_keys("rssi,structure_power,climax_signal");
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0], "rssi");
    }

    #[test]
    fn test_outcome_score_all_match() {
        let plans = vec![
            crate::memory::TradePlan {
                label: "A".into(),
                direction: "LONG".into(),
                entry: Some(80000.0),
                target: Some(82000.0),
                stop: Some(79000.0),
                rationale: "bullish".into(),
            },
            crate::memory::TradePlan {
                label: "B".into(),
                direction: "LONG".into(),
                entry: Some(80500.0),
                target: Some(83000.0),
                stop: Some(79500.0),
                rationale: "also bullish".into(),
            },
        ];
        // Price went up → LONG correct
        let score = compute_outcome_score(&plans, 80000.0, 82000.0);
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_no_match() {
        let plans = vec![crate::memory::TradePlan {
            label: "A".into(),
            direction: "SHORT".into(),
            entry: Some(80000.0),
            target: Some(78000.0),
            stop: Some(81000.0),
            rationale: "bearish".into(),
        }];
        // Price went up but plan was SHORT
        let score = compute_outcome_score(&plans, 80000.0, 82000.0);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_empty_plans() {
        let score = compute_outcome_score(&[], 80000.0, 82000.0);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }
}

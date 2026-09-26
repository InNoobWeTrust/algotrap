//! Scoring — tier engine, significant-change detection, and outcome validation.

use std::collections::HashMap;

use algotrap::engine::traits::ComputedFrame;
use algotrap::prelude::{Direction, Timeframe};
use chrono::{DateTime, Utc};

use crate::memory::{TradePlan, TradePlanOutcome, TradePlanOutcomeKind};

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
/// Unavailable values (`None`) are ignored rather than treated as zero.
/// Returns `(has_significant_change, max_delta)`.
pub fn detect_significant_change(
    old_indicators: &HashMap<String, Option<f64>>,
    new_indicators: &HashMap<String, Option<f64>>,
    key_indicators: &[String],
    threshold: f64,
) -> (bool, f64) {
    let mut max_delta: f64 = 0.0;

    for key in key_indicators {
        if let (Some(old_val), Some(new_val)) = (
            old_indicators.get(key).copied().flatten(),
            new_indicators.get(key).copied().flatten(),
        ) {
            let delta = symmetric_delta(old_val, new_val);
            max_delta = max_delta.max(delta);
        }
    }

    (max_delta >= threshold, max_delta)
}

/// Determine whether a notification should be sent based on tier change,
/// significant-change detection, time-based cooldown, and direction.
pub fn should_notify(
    current_tier: Tier,
    previous_tier: Option<&str>,
    has_significant_change: bool,
    last_notified_at: Option<chrono::DateTime<chrono::Utc>>,
    cooldown_secs: u64,
    direction: Direction,
) -> bool {
    // NONE direction is not actionable — suppress across all tiers.
    if direction.is_none() {
        return false;
    }
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
    let tier_changed = previous_tier.is_none_or(|prev| prev != tier_str);
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
    direction: Direction,
    prediction_price: f64,
    current_price: f64,
    atr: Option<f64>,
) -> f64 {
    let delta = current_price - prediction_price;
    let abs_delta = delta.abs();

    // NONE direction: scored conditionally against ATR
    if direction.is_none() {
        return match atr {
            Some(atr_val) if atr_val > 0.0 && abs_delta < 0.5 * atr_val => {
                1.0 // Correctly identified no-trade
            }
            Some(_) => 0.0, // Missed a significant move
            _ => 0.0,       // Can't verify without ATR
        };
    }

    // Determine if the prediction direction was correct
    let direction_correct = match direction {
        Direction::Long => delta > 0.0,
        Direction::Short => delta < 0.0,
        Direction::None => false, // already handled above
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
/// Prefers `atr_percent` (= ATR / open, always populated) for reliable results.
/// Falls back to stored `atr_reversion_percent` for previous memory files.
/// Returns None if neither key is present or values are non-positive.
pub fn reconstruct_atr(indicators: &HashMap<String, Option<f64>>) -> Option<f64> {
    let close = indicators.get("close").copied().flatten()?;
    if close <= 0.0 {
        return None;
    }

    // Prefer atr_percent (ratio, always populated): ATR = atr_percent × close
    if let Some(atr_ratio) = indicators.get("atr_percent").copied().flatten()
        && atr_ratio > 0.0
    {
        return Some(close * atr_ratio);
    }

    // Stored-data fallback: atr_reversion_percent (often zero, less reliable)
    if let Some(atr_pct) = indicators.get("atr_reversion_percent").copied().flatten()
        && atr_pct > 0.0
    {
        return Some(close * atr_pct / 100.0);
    }

    None
}

// ─── Trade-Plan Evaluation (U3) ────────────────────────────────────────────
//
// Deterministic candle-path evaluation of adaptive trade plans.
//
// Rules (locked by architect plan U3):
// - Eligible plans only: LONG/SHORT direction, timeframe >= 4h, finite positive
//   entry/target/stop, LONG `stop < entry < target`, SHORT
//   `target < entry < stop`, reward distance >= risk distance.
//   WAIT/NONE, missing timeframe, sub-4h, malformed levels, and already-terminal
//   plans are ignored (`None`).
// - Use only frames at or below the plan timeframe; never use a higher timeframe
//   to infer finer chronology.
// - Require retained history to cover the signal boundary: if the earliest
//   coarse candle starts after `signal_at`, return `None` (expired history).
// - Require continuous coarse-frame coverage from the candle
//   containing/covering `signal_at` (`start <= signal_at < end`, with
//   `end == start + timeframe duration`) through each evaluated candle:
//   if the signal interval is missing, or a later coarse interval has a gap
//   (`next.start != prev.end`) before a decisive outcome, return `None`
//   (unresolved). Do not fabricate decisive/ambiguous outcomes across gaps;
//   same-candle ambiguity already proven within the continuous prefix is
//   still returned because evaluation stops before the gap.
// - Read candles chronologically using `time`/`high`/`low` only; a level is
//   touched inclusively when `low <= level <= high`.
// - State machine: `SeekingEntry` (exits before entry ignored) -> `Open`
//   (first chronologically proven SL/TP settles).
// - Start at the plan timeframe where available (else highest available <= plan).
//   Refine a coarse candle with progressively lower available timeframes
//   whenever (a) entry + any exit touch the same candle, (b) an open trade's SL
//   and TP both touch, or (c) the candle overlaps the exact signal timestamp
//   (`start < signal_at < end`) and contains a relevant hit.
// - Finer candles must cover the exact coarse interval without gaps
//   (`first.start == coarse.start`, `last.end == coarse.end`, contiguous).
// - If the finest available candle still contains both SL and TP, or coverage /
//   order remains indeterminate, record terminal `Ambiguous`.
// - Never infer ordering from candle color, close location, distance from open,
//   or LONG/SHORT bias.
// - `None` means pending/unscored (entry never reached, or open but unresolved)
//   so the plan can be re-evaluated next cycle.

#[derive(Debug, Clone, Copy, PartialEq)]
struct Candle {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    high: f64,
    low: f64,
}

fn timeframe_duration(tf: Timeframe) -> chrono::Duration {
    chrono::Duration::minutes(tf.weight() as i64)
}

fn level_touched(low: f64, high: f64, level: f64) -> bool {
    low <= level && level <= high
}

/// Structurally valid LONG/SHORT levels, ignoring stored outcome.
///
/// Returns `(timeframe, entry, target, stop, is_long)`.
fn structural_levels(plan: &TradePlan) -> Option<(Timeframe, f64, f64, f64, bool)> {
    let direction: Direction = plan.direction.parse().ok()?;
    let is_long = match direction {
        Direction::Long => true,
        Direction::Short => false,
        Direction::None => return None,
    };
    let tf = plan.timeframe?;
    if tf.weight() < Timeframe::H4.weight() {
        return None;
    }
    let (entry, target, stop) = (plan.entry?, plan.target?, plan.stop?);
    for v in [entry, target, stop] {
        if !v.is_finite() || v <= 0.0 {
            return None;
        }
    }
    if is_long {
        if !(stop < entry && entry < target) {
            return None;
        }
        if (target - entry) < (entry - stop) {
            return None;
        }
    } else {
        if !(target < entry && entry < stop) {
            return None;
        }
        if (entry - target) < (stop - entry) {
            return None;
        }
    }
    Some((tf, entry, target, stop, is_long))
}

fn read_candles(frame: &dyn ComputedFrame, tf: Timeframe) -> Option<Vec<Candle>> {
    if frame.is_empty() {
        return None;
    }
    if !(frame.has_column("time") && frame.has_column("high") && frame.has_column("low")) {
        return None;
    }
    let duration = timeframe_duration(tf);
    let mut out = Vec::with_capacity(frame.len());
    for row in 0..frame.len() {
        let time_f = match frame.f64_at("time", row) {
            Ok(Some(v)) if v.is_finite() => v,
            _ => continue,
        };
        let high = match frame.f64_at("high", row) {
            Ok(Some(v)) if v.is_finite() => v,
            _ => continue,
        };
        let low = match frame.f64_at("low", row) {
            Ok(Some(v)) if v.is_finite() => v,
            _ => continue,
        };
        let time_ms = time_f.round() as i64;
        let Some(start) = DateTime::from_timestamp_millis(time_ms) else {
            continue;
        };
        let Some(end) = start.checked_add_signed(duration) else {
            continue;
        };
        out.push(Candle {
            start,
            end,
            high,
            low,
        });
    }
    if out.is_empty() {
        return None;
    }
    out.sort_by_key(|c| c.start);
    Some(out)
}

/// Finer candles covering exactly `[coarse.start, coarse.end)` without gaps.
fn covering_candles(coarse: &Candle, finer: &[Candle]) -> Option<Vec<Candle>> {
    let mut covering: Vec<Candle> = finer
        .iter()
        .copied()
        .filter(|c| c.start >= coarse.start && c.start < coarse.end)
        .collect();
    if covering.is_empty() {
        return None;
    }
    covering.sort_by_key(|c| c.start);
    if covering.first().map(|c| c.start) != Some(coarse.start) {
        return None;
    }
    if covering.last().map(|c| c.end) != Some(coarse.end) {
        return None;
    }
    for pair in covering.windows(2) {
        if pair[0].end != pair[1].start {
            return None;
        }
    }
    Some(covering)
}

enum SeekingRefine {
    Terminal {
        kind: TradePlanOutcomeKind,
        entry_at: DateTime<Utc>,
        resolved_at: DateTime<Utc>,
        tf: Timeframe,
    },
    EntryOnly {
        entry_at: DateTime<Utc>,
    },
    NoHit,
    Indeterminate,
}

enum OpenRefine {
    Terminal {
        kind: TradePlanOutcomeKind,
        resolved_at: DateTime<Utc>,
        tf: Timeframe,
    },
    NoHit,
    Indeterminate,
}

#[allow(clippy::too_many_arguments)]
fn refine_seeking(
    coarse: &Candle,
    entry: f64,
    target: f64,
    stop: f64,
    signal_at: DateTime<Utc>,
    finer_tfs: &[Timeframe],
    by_tf: &HashMap<Timeframe, Vec<Candle>>,
    coarse_is_overlap: bool,
) -> SeekingRefine {
    for tf in finer_tfs {
        let Some(finer_all) = by_tf.get(tf) else {
            continue;
        };
        let Some(covering) = covering_candles(coarse, finer_all) else {
            continue;
        };
        let mut local_entry: Option<DateTime<Utc>> = None;
        let mut sub_ambiguous = false;
        let mut terminal: Option<(TradePlanOutcomeKind, DateTime<Utc>, DateTime<Utc>)> = None;
        for finer in &covering {
            if finer.end <= signal_at {
                continue;
            }
            let is_overlap = finer.start < signal_at && signal_at < finer.end;
            let e = level_touched(finer.low, finer.high, entry);
            let t = level_touched(finer.low, finer.high, target);
            let s = level_touched(finer.low, finer.high, stop);
            if is_overlap {
                if local_entry.is_none() {
                    if e {
                        sub_ambiguous = true;
                        break;
                    }
                    // Exits before entry are ignored, even inside overlap.
                    continue;
                }
                if t || s {
                    sub_ambiguous = true;
                    break;
                }
                continue;
            }
            if let Some(entry_at) = local_entry {
                if t && s {
                    sub_ambiguous = true;
                    break;
                }
                if t {
                    terminal = Some((TradePlanOutcomeKind::TakeProfit, entry_at, finer.start));
                    break;
                }
                if s {
                    terminal = Some((TradePlanOutcomeKind::StopLoss, entry_at, finer.start));
                    break;
                }
            } else {
                if e && (t || s) {
                    sub_ambiguous = true;
                    break;
                }
                if e {
                    local_entry = Some(finer.start);
                }
                // Single exit without entry is ignored (exits before entry).
            }
        }
        if sub_ambiguous {
            continue;
        }
        if let Some((kind, entry_at, resolved_at)) = terminal {
            return SeekingRefine::Terminal {
                kind,
                entry_at,
                resolved_at,
                tf: *tf,
            };
        }
        if let Some(entry_at) = local_entry {
            return SeekingRefine::EntryOnly { entry_at };
        }
        // No entry proven post-signal in this finer view.
        if coarse_is_overlap {
            return SeekingRefine::NoHit;
        }
        // Non-overlap entry+exit with no finer entry is a data mismatch:
        // try the next finer timeframe before giving up.
        continue;
    }
    SeekingRefine::Indeterminate
}

fn refine_open(
    coarse: &Candle,
    target: f64,
    stop: f64,
    signal_at: DateTime<Utc>,
    finer_tfs: &[Timeframe],
    by_tf: &HashMap<Timeframe, Vec<Candle>>,
    coarse_is_overlap: bool,
) -> OpenRefine {
    for tf in finer_tfs {
        let Some(finer_all) = by_tf.get(tf) else {
            continue;
        };
        let Some(covering) = covering_candles(coarse, finer_all) else {
            continue;
        };
        let mut sub_ambiguous = false;
        let mut terminal: Option<(TradePlanOutcomeKind, DateTime<Utc>)> = None;
        for finer in &covering {
            if finer.end <= signal_at {
                continue;
            }
            let is_overlap = finer.start < signal_at && signal_at < finer.end;
            let t = level_touched(finer.low, finer.high, target);
            let s = level_touched(finer.low, finer.high, stop);
            if is_overlap {
                if t || s {
                    sub_ambiguous = true;
                    break;
                }
                continue;
            }
            if t && s {
                sub_ambiguous = true;
                break;
            }
            if t {
                terminal = Some((TradePlanOutcomeKind::TakeProfit, finer.start));
                break;
            }
            if s {
                terminal = Some((TradePlanOutcomeKind::StopLoss, finer.start));
                break;
            }
        }
        if sub_ambiguous {
            continue;
        }
        if let Some((kind, resolved_at)) = terminal {
            return OpenRefine::Terminal {
                kind,
                resolved_at,
                tf: *tf,
            };
        }
        if coarse_is_overlap {
            return OpenRefine::NoHit;
        }
        // Non-overlap open with no finer hit contradicts the coarse touch:
        // try the next finer timeframe.
        continue;
    }
    OpenRefine::Indeterminate
}

/// Evaluate one trade plan against retained candle history.
///
/// Returns `Some(outcome)` only for terminal states (`TakeProfit`, `StopLoss`,
/// `Ambiguous`). Returns `None` for ineligible plans (WAIT, missing/sub-4h
/// timeframe, malformed levels, already terminal) and for pending plans (entry
/// never reached, or open but unresolved) so callers can retry next cycle.
pub fn evaluate_trade_plan(
    plan: &TradePlan,
    signal_at: DateTime<Utc>,
    all_dfs: &HashMap<Timeframe, Box<dyn ComputedFrame>>,
) -> Option<TradePlanOutcome> {
    if plan.outcome.is_some() {
        return None;
    }
    let (plan_tf, entry, target, stop, _is_long) = structural_levels(plan)?;

    // Usable frames: at or below the plan timeframe only.
    let mut by_tf: HashMap<Timeframe, Vec<Candle>> = HashMap::new();
    for (tf, frame) in all_dfs {
        if tf.weight() > plan_tf.weight() {
            continue;
        }
        if let Some(candles) = read_candles(frame.as_ref(), *tf) {
            by_tf.insert(*tf, candles);
        }
    }
    if by_tf.is_empty() {
        return None;
    }

    // Coarse driver: plan timeframe where available, else highest <= plan.
    let coarse_tf = if by_tf.contains_key(&plan_tf) {
        plan_tf
    } else {
        *by_tf.keys().max_by_key(|tf| tf.weight())?
    };
    let coarse_all = by_tf.get(&coarse_tf)?.clone();
    if coarse_all.is_empty() {
        return None;
    }

    // History must cover the signal boundary.
    let earliest = coarse_all.iter().map(|c| c.start).min()?;
    if earliest > signal_at {
        return None;
    }

    // Candles from the signal onward (overlapping + later).
    let mut considered: Vec<Candle> = coarse_all
        .into_iter()
        .filter(|c| c.end > signal_at)
        .collect();
    if considered.is_empty() {
        return None;
    }
    considered.sort_by_key(|c| c.start);

    // Continuous coarse-frame coverage from the signal interval onward.
    // Signal interval uses exact timestamp semantics: `start <= signal_at < end`
    // where `end == start + timeframe duration`. A missing signal interval
    // means expired/gapped history: unresolved.
    let covers_signal = considered
        .iter()
        .any(|c| c.start <= signal_at && signal_at < c.end);
    if !covers_signal {
        return None;
    }

    // Finer timeframes for refinement, coarsest-finer first.
    let mut finer_tfs: Vec<Timeframe> = by_tf
        .keys()
        .copied()
        .filter(|tf| tf.weight() < coarse_tf.weight())
        .collect();
    finer_tfs.sort_by_key(|tf| std::cmp::Reverse(tf.weight()));

    // Finest usable timeframe anchors ambiguous resolution metadata.
    let finest_tf = by_tf
        .keys()
        .copied()
        .min_by_key(|tf| tf.weight())
        .unwrap_or(coarse_tf);

    let mut lowest = f64::INFINITY;
    let mut highest = f64::NEG_INFINITY;
    let mut entry_hit_at: Option<DateTime<Utc>> = None;

    let ambiguous =
        |entry_at: Option<DateTime<Utc>>, resolved_at: DateTime<Utc>, lowest: f64, highest: f64| {
            TradePlanOutcome {
                kind: TradePlanOutcomeKind::Ambiguous,
                entry_hit_at: entry_at,
                resolved_at,
                resolution_timeframe: finest_tf,
                lowest_reached: lowest,
                highest_reached: highest,
            }
        };

    for (idx, coarse) in considered.iter().enumerate() {
        // Gap check: each evaluated candle must be exactly adjacent to the
        // previous (`next.start == prev.end`, with `end == start + timeframe
        // duration`). A gap before any decisive outcome leaves the path
        // unproven: return unresolved `None` rather than fabricating a
        // decisive or ambiguous outcome. Same-candle ambiguity proven within
        // the continuous prefix is preserved via early return before the gap.
        if idx > 0 {
            let prev = &considered[idx - 1];
            if coarse.start != prev.end {
                return None;
            }
        }
        if !coarse.low.is_finite() || !coarse.high.is_finite() {
            continue;
        }
        lowest = lowest.min(coarse.low);
        highest = highest.max(coarse.high);

        let entry_hit = level_touched(coarse.low, coarse.high, entry);
        let tp_hit = level_touched(coarse.low, coarse.high, target);
        let sl_hit = level_touched(coarse.low, coarse.high, stop);
        let is_overlap = coarse.start < signal_at && signal_at < coarse.end;

        if entry_hit_at.is_none() {
            // SeekingEntry: exits before entry are ignored.
            if is_overlap {
                if !entry_hit {
                    continue;
                }
                if finer_tfs.is_empty() {
                    return Some(ambiguous(None, coarse.start, lowest, highest));
                }
                match refine_seeking(
                    coarse, entry, target, stop, signal_at, &finer_tfs, &by_tf, true,
                ) {
                    SeekingRefine::Terminal {
                        kind,
                        entry_at,
                        resolved_at,
                        tf,
                    } => {
                        return Some(TradePlanOutcome {
                            kind,
                            entry_hit_at: Some(entry_at),
                            resolved_at,
                            resolution_timeframe: tf,
                            lowest_reached: lowest,
                            highest_reached: highest,
                        });
                    }
                    SeekingRefine::EntryOnly { entry_at } => {
                        entry_hit_at = Some(entry_at);
                    }
                    SeekingRefine::NoHit => {}
                    SeekingRefine::Indeterminate => {
                        return Some(ambiguous(None, coarse.start, lowest, highest));
                    }
                }
            } else if entry_hit && (tp_hit || sl_hit) {
                if finer_tfs.is_empty() {
                    return Some(ambiguous(None, coarse.start, lowest, highest));
                }
                match refine_seeking(
                    coarse, entry, target, stop, signal_at, &finer_tfs, &by_tf, false,
                ) {
                    SeekingRefine::Terminal {
                        kind,
                        entry_at,
                        resolved_at,
                        tf,
                    } => {
                        return Some(TradePlanOutcome {
                            kind,
                            entry_hit_at: Some(entry_at),
                            resolved_at,
                            resolution_timeframe: tf,
                            lowest_reached: lowest,
                            highest_reached: highest,
                        });
                    }
                    SeekingRefine::EntryOnly { entry_at } => {
                        entry_hit_at = Some(entry_at);
                    }
                    SeekingRefine::NoHit | SeekingRefine::Indeterminate => {
                        return Some(ambiguous(None, coarse.start, lowest, highest));
                    }
                }
            } else if entry_hit {
                entry_hit_at = Some(coarse.start);
            } else {
                // No entry: ignore any exit touches in this candle.
            }
        } else {
            // Open: first proven exit settles.
            if is_overlap {
                if !(tp_hit || sl_hit) {
                    continue;
                }
                if finer_tfs.is_empty() {
                    return Some(ambiguous(entry_hit_at, coarse.start, lowest, highest));
                }
                match refine_open(coarse, target, stop, signal_at, &finer_tfs, &by_tf, true) {
                    OpenRefine::Terminal {
                        kind,
                        resolved_at,
                        tf,
                    } => {
                        return Some(TradePlanOutcome {
                            kind,
                            entry_hit_at,
                            resolved_at,
                            resolution_timeframe: tf,
                            lowest_reached: lowest,
                            highest_reached: highest,
                        });
                    }
                    OpenRefine::NoHit => {}
                    OpenRefine::Indeterminate => {
                        return Some(ambiguous(entry_hit_at, coarse.start, lowest, highest));
                    }
                }
            } else if tp_hit && sl_hit {
                if finer_tfs.is_empty() {
                    return Some(ambiguous(entry_hit_at, coarse.start, lowest, highest));
                }
                match refine_open(coarse, target, stop, signal_at, &finer_tfs, &by_tf, false) {
                    OpenRefine::Terminal {
                        kind,
                        resolved_at,
                        tf,
                    } => {
                        return Some(TradePlanOutcome {
                            kind,
                            entry_hit_at,
                            resolved_at,
                            resolution_timeframe: tf,
                            lowest_reached: lowest,
                            highest_reached: highest,
                        });
                    }
                    OpenRefine::NoHit | OpenRefine::Indeterminate => {
                        return Some(ambiguous(entry_hit_at, coarse.start, lowest, highest));
                    }
                }
            } else if tp_hit {
                return Some(TradePlanOutcome {
                    kind: TradePlanOutcomeKind::TakeProfit,
                    entry_hit_at,
                    resolved_at: coarse.start,
                    resolution_timeframe: coarse_tf,
                    lowest_reached: lowest,
                    highest_reached: highest,
                });
            } else if sl_hit {
                return Some(TradePlanOutcome {
                    kind: TradePlanOutcomeKind::StopLoss,
                    entry_hit_at,
                    resolved_at: coarse.start,
                    resolution_timeframe: coarse_tf,
                    lowest_reached: lowest,
                    highest_reached: highest,
                });
            }
        }
    }

    // Entry never reached, or open but no exit proven: pending, retry next cycle.
    None
}

/// Average settled plan outcomes: TP = 1.0, SL = 0.0.
///
/// Considers only structurally valid LONG/SHORT plans. Returns `None` when
/// there are no eligible plans, when any eligible plan remains unresolved
/// (`outcome: None`), or when every terminal plan is `Ambiguous` (ambiguous
/// plans are excluded from numerator and denominator).
pub fn trade_plan_outcome_score(plans: &[TradePlan]) -> Option<f64> {
    let eligible: Vec<&TradePlan> = plans
        .iter()
        .filter(|p| structural_levels(p).is_some())
        .collect();
    if eligible.is_empty() {
        return None;
    }
    if eligible.iter().any(|p| p.outcome.is_none()) {
        return None;
    }
    let mut wins = 0usize;
    let mut losses = 0usize;
    for plan in &eligible {
        match plan.outcome.as_ref().map(|o| o.kind) {
            Some(TradePlanOutcomeKind::TakeProfit) => wins += 1,
            Some(TradePlanOutcomeKind::StopLoss) => losses += 1,
            Some(TradePlanOutcomeKind::Ambiguous) | None => {}
        }
    }
    let decisive = wins + losses;
    if decisive == 0 {
        return None;
    }
    Some(wins as f64 / decisive as f64)
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
        let old = HashMap::from([
            ("rssi".into(), Some(50.0)),
            ("structure_power".into(), Some(0.0)),
        ]);
        let new = HashMap::from([
            ("rssi".into(), Some(35.0)),
            ("structure_power".into(), Some(1.0)),
        ]);
        let keys = vec!["rssi".into(), "structure_power".into()];

        let (changed, max_delta) = detect_significant_change(&old, &new, &keys, 0.20);
        assert!(changed);
        assert!(max_delta >= 0.30); // rssi delta = 30%
    }

    #[test]
    fn test_no_significant_change() {
        let old = HashMap::from([("rssi".into(), Some(50.0))]);
        let new = HashMap::from([("rssi".into(), Some(48.0))]);
        let keys = vec!["rssi".into()];

        let (changed, _) = detect_significant_change(&old, &new, &keys, 0.20);
        assert!(!changed); // 4% change < 20% threshold
    }

    #[test]
    fn test_detect_significant_change_ignores_unavailable_values() {
        let old = HashMap::from([("rssi".into(), Some(50.0))]);
        let new = HashMap::from([("rssi".into(), None)]);
        let keys = vec!["rssi".into()];

        let (changed, max_delta) = detect_significant_change(&old, &new, &keys, 0.01);

        assert!(!changed);
        assert_eq!(max_delta, 0.0);
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
            Direction::Long
        ));
        assert!(should_notify(
            Tier::Watch,
            Some("SILENT"),
            false,
            Some(past),
            3600,
            Direction::Long
        ));
        // Tier change to Silent still doesn't notify (Silent never notifies)
        assert!(!should_notify(
            Tier::Silent,
            Some("ALERT"),
            false,
            Some(past),
            3600,
            Direction::Long
        ));
    }

    #[test]
    fn test_should_notify_cold_start() {
        // No previous tier → always notify (unless NONE direction)
        assert!(should_notify(
            Tier::Watch,
            None,
            false,
            None,
            3600,
            Direction::Long
        ));
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
            Direction::Long
        ));
        // Significant change but still in cooldown → suppress
        assert!(!should_notify(
            Tier::Watch,
            Some("WATCH"),
            true,
            Some(recent),
            3600,
            Direction::Long
        ));
        // No significant change + cooldown expired → suppress
        assert!(!should_notify(
            Tier::Watch,
            Some("WATCH"),
            false,
            Some(past),
            3600,
            Direction::Long
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
            Direction::Long
        ));
        // Alert but in cooldown → suppress
        assert!(!should_notify(
            Tier::Alert,
            Some("ALERT"),
            false,
            Some(recent),
            3600,
            Direction::Long
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
            Direction::Long
        ));
        assert!(!should_notify(
            Tier::Silent,
            Some("SILENT"),
            true,
            Some(past),
            3600,
            Direction::Long
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
            Direction::None
        ));
        assert!(!should_notify(
            Tier::Watch,
            None,
            true,
            None,
            3600,
            Direction::None
        ));
        // Alert + NONE → also suppressed (NONE means no actionable trade)
        assert!(!should_notify(
            Tier::Alert,
            Some("WATCH"),
            false,
            Some(past),
            3600,
            Direction::None
        ));
        // WAIT is the display string for NONE — same suppression applies
        assert!(!should_notify(
            Tier::Watch,
            None,
            true,
            None,
            3600,
            Direction::None
        ));
        assert!(!should_notify(
            Tier::Alert,
            Some("WATCH"),
            false,
            Some(past),
            3600,
            Direction::None
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
        let score = compute_outcome_score(Direction::Long, 87000.0, 87800.0, Some(500.0));
        // magnitude = min(1.0, 800/500) = 1.0
        // score = 1.0 × (0.6 + 1.0 × 0.4) = 1.0
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_long_correct_small_move() {
        // LONG prediction, price up slightly, ATR = 500
        let score = compute_outcome_score(Direction::Long, 87000.0, 87100.0, Some(500.0));
        // magnitude = min(1.0, 100/500) = 0.2
        // score = 1.0 × (0.6 + 0.2 × 0.4) = 0.68
        assert!((score - 0.68).abs() < 1e-10);
    }

    #[test]
    fn test_outcome_score_long_wrong() {
        // LONG prediction, price went DOWN → 0.0
        let score = compute_outcome_score(Direction::Long, 87000.0, 86500.0, Some(500.0));
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_short_correct() {
        // SHORT prediction, price went down
        let score = compute_outcome_score(Direction::Short, 87000.0, 86200.0, Some(500.0));
        // magnitude = min(1.0, 800/500) = 1.0
        // score = 1.0 × (0.6 + 1.0 × 0.4) = 1.0
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_short_wrong() {
        // SHORT prediction, price went UP → 0.0
        let score = compute_outcome_score(Direction::Short, 87000.0, 87500.0, Some(500.0));
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_none_flat_correct() {
        // NONE prediction, market stayed flat (|Δ| < 0.5 × ATR)
        let score = compute_outcome_score(Direction::None, 87000.0, 87100.0, Some(500.0));
        // |100| < 250 → correct no-trade → 1.0
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_none_missed_move() {
        // NONE prediction, significant move missed (|Δ| > 0.5 × ATR)
        let score = compute_outcome_score(Direction::None, 87000.0, 87600.0, Some(500.0));
        // |600| > 250 → missed move → 0.0
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_no_atr_correct() {
        // LONG prediction correct, no ATR → binary 1.0
        let score = compute_outcome_score(Direction::Long, 87000.0, 87500.0, None);
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_no_atr_wrong() {
        // LONG prediction wrong, no ATR → 0.0
        let score = compute_outcome_score(Direction::Long, 87000.0, 86500.0, None);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_outcome_score_none_no_atr() {
        // NONE without ATR → 0.0 (can't verify)
        let score = compute_outcome_score(Direction::None, 87000.0, 87100.0, None);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_direction_accuracy() {
        use chrono::Utc;
        let make_pred = |score: Option<f64>| crate::memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: Direction::Long,
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
    fn test_reconstruct_atr_from_atr_percent() {
        // Primary path: atr_percent = ATR / open ≈ ratio
        let indicators = HashMap::from([
            ("close".to_string(), Some(87000.0)),
            ("atr_percent".to_string(), Some(0.00575)), // ATR ≈ 500.25
        ]);
        let atr = reconstruct_atr(&indicators).unwrap();
        assert!((atr - 500.25).abs() < 1.0);
    }

    #[test]
    fn test_reconstruct_atr_stored_data_fallback() {
        // Stored-data path: atr_reversion_percent (when atr_percent is absent)
        let indicators = HashMap::from([
            ("close".to_string(), Some(87000.0)),
            ("atr_reversion_percent".to_string(), Some(0.575)),
        ]);
        let atr = reconstruct_atr(&indicators).unwrap();
        // 87000 × 0.575 / 100 = 500.25
        assert!((atr - 500.25).abs() < 1e-10);
    }

    #[test]
    fn test_reconstruct_atr_prefers_atr_percent() {
        // When both keys are present, atr_percent wins
        let indicators = HashMap::from([
            ("close".to_string(), Some(87000.0)),
            ("atr_percent".to_string(), Some(0.00575)),
            ("atr_reversion_percent".to_string(), Some(0.0)), // zero = would not work
        ]);
        let atr = reconstruct_atr(&indicators).unwrap();
        assert!((atr - 500.25).abs() < 1.0);
    }

    #[test]
    fn test_reconstruct_atr_missing() {
        let indicators = HashMap::from([("close".to_string(), Some(87000.0))]);
        assert!(reconstruct_atr(&indicators).is_none());
    }

    #[test]
    fn test_reconstruct_atr_handles_none_values() {
        let indicators = HashMap::from([
            ("close".to_string(), Some(87000.0)),
            ("atr_percent".to_string(), None),
            ("atr_reversion_percent".to_string(), Some(0.575)),
        ]);

        let atr = reconstruct_atr(&indicators).unwrap();
        assert!((atr - 500.25).abs() < 1e-10);
    }

    // ─── Low Accuracy Streak Tests (Scenario 13) ────────────────────────────

    #[test]
    fn test_is_low_accuracy_streak_insufficient_scored() {
        use chrono::Utc;
        let make_pred = |score: Option<f64>| crate::memory::Prediction {
            timestamp: Utc::now(),
            confidence: 60.0,
            direction: Direction::Long,
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
            direction: Direction::Long,
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
            direction: Direction::Long,
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
            direction: Direction::Long,
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
            direction: Direction::Long,
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
            direction: Direction::Long,
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

    // ─── Trade-Plan Evaluation (U3) Tests ────────────────────────────────────
    //
    // Synthetic in-file `ComputedFrame` fixtures. Only `time`/`high`/`low` are
    // read; `open`/`close`/color/direction are never consulted.

    struct FixtureCandle {
        time_ms: i64,
        high: f64,
        low: f64,
    }

    struct FixtureFrame {
        rows: Vec<FixtureCandle>,
    }

    impl FixtureFrame {
        fn new(rows: Vec<(i64, f64, f64)>) -> Self {
            Self {
                rows: rows
                    .into_iter()
                    .map(|(time_ms, high, low)| FixtureCandle { time_ms, high, low })
                    .collect(),
            }
        }
    }

    impl algotrap::engine::traits::ComputedFrame for FixtureFrame {
        fn len(&self) -> usize {
            self.rows.len()
        }

        fn columns(&self) -> Vec<String> {
            vec!["time".into(), "high".into(), "low".into()]
        }

        fn column_dtypes(&self) -> Vec<(String, algotrap::engine::traits::ColumnDType)> {
            use algotrap::engine::traits::ColumnDType;

            vec![
                ("time".into(), ColumnDType::Number),
                ("high".into(), ColumnDType::Number),
                ("low".into(), ColumnDType::Number),
            ]
        }

        fn slice_last(
            &self,
            count: usize,
        ) -> Result<
            Box<dyn algotrap::engine::traits::ComputedFrame>,
            algotrap::engine::error::MarketError,
        > {
            let start = self.rows.len().saturating_sub(count);
            Ok(Box::new(Self {
                rows: self.rows[start..]
                    .iter()
                    .map(|r| FixtureCandle {
                        time_ms: r.time_ms,
                        high: r.high,
                        low: r.low,
                    })
                    .collect(),
            }))
        }

        fn f64_at(
            &self,
            column: &str,
            row: usize,
        ) -> Result<Option<f64>, algotrap::engine::error::MarketError> {
            if row >= self.rows.len() {
                return Err(algotrap::engine::error::MarketError::data_access(format!(
                    "row {row} out of bounds"
                )));
            }
            match column {
                "time" => Ok(Some(self.rows[row].time_ms as f64)),
                "high" => Ok(Some(self.rows[row].high)),
                "low" => Ok(Some(self.rows[row].low)),
                _ => Err(algotrap::engine::error::MarketError::data_access(format!(
                    "column {column} not found"
                ))),
            }
        }

        fn string_at(
            &self,
            column: &str,
            row: usize,
        ) -> Result<Option<String>, algotrap::engine::error::MarketError> {
            let _ = (column, row);
            Err(algotrap::engine::error::MarketError::data_access(
                "fixture has no string columns",
            ))
        }

        fn to_json_records(
            &self,
        ) -> Result<
            Vec<serde_json::Map<String, serde_json::Value>>,
            algotrap::engine::error::MarketError,
        > {
            let mut out = Vec::with_capacity(self.rows.len());
            for r in &self.rows {
                let mut m = serde_json::Map::new();
                m.insert(
                    "time".into(),
                    serde_json::Number::from_f64(r.time_ms as f64)
                        .map(serde_json::Value::Number)
                        .ok_or_else(|| {
                            algotrap::engine::error::MarketError::computation("non-finite time")
                        })?,
                );
                m.insert(
                    "high".into(),
                    serde_json::Number::from_f64(r.high)
                        .map(serde_json::Value::Number)
                        .ok_or_else(|| {
                            algotrap::engine::error::MarketError::computation("non-finite high")
                        })?,
                );
                m.insert(
                    "low".into(),
                    serde_json::Number::from_f64(r.low)
                        .map(serde_json::Value::Number)
                        .ok_or_else(|| {
                            algotrap::engine::error::MarketError::computation("non-finite low")
                        })?,
                );
                out.push(m);
            }
            Ok(out)
        }

        fn has_column(&self, column: &str) -> bool {
            matches!(column, "time" | "high" | "low")
        }
    }

    fn base_ms() -> i64 {
        chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .timestamp_millis()
    }

    fn dt_at(ms: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(ms).expect("valid test timestamp")
    }

    const H4_MS: i64 = 4 * 60 * 60 * 1000;
    const H1_MS: i64 = 60 * 60 * 1000;

    fn h4_frame(rows: Vec<(i64, f64, f64)>) -> Box<dyn algotrap::engine::traits::ComputedFrame> {
        Box::new(FixtureFrame::new(rows))
    }

    #[allow(clippy::type_complexity)]
    fn dfs(
        frames: Vec<(algotrap::prelude::Timeframe, Vec<(i64, f64, f64)>)>,
    ) -> HashMap<algotrap::prelude::Timeframe, Box<dyn algotrap::engine::traits::ComputedFrame>>
    {
        frames
            .into_iter()
            .map(|(tf, rows)| (tf, h4_frame(rows)))
            .collect()
    }

    fn long_plan(
        entry: f64,
        target: f64,
        stop: f64,
        tf: Option<algotrap::prelude::Timeframe>,
    ) -> crate::memory::TradePlan {
        crate::memory::TradePlan {
            label: "A".into(),
            direction: "LONG".into(),
            entry: Some(entry),
            target: Some(target),
            stop: Some(stop),
            rationale: "test".into(),
            timeframe: tf,
            outcome: None,
        }
    }

    fn short_plan(
        entry: f64,
        target: f64,
        stop: f64,
        tf: Option<algotrap::prelude::Timeframe>,
    ) -> crate::memory::TradePlan {
        crate::memory::TradePlan {
            label: "A".into(),
            direction: "SHORT".into(),
            entry: Some(entry),
            target: Some(target),
            stop: Some(stop),
            rationale: "test".into(),
            timeframe: tf,
            outcome: None,
        }
    }

    fn settled_plan(
        mut plan: crate::memory::TradePlan,
        kind: crate::memory::TradePlanOutcomeKind,
    ) -> crate::memory::TradePlan {
        plan.outcome = Some(crate::memory::TradePlanOutcome {
            kind,
            entry_hit_at: None,
            resolved_at: dt_at(base_ms()),
            resolution_timeframe: algotrap::prelude::Timeframe::H4,
            lowest_reached: 90.0,
            highest_reached: 110.0,
        });
        plan
    }

    #[test]
    fn test_u3_long_entry_before_tp() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + H4_MS, 112.0, 105.0),
            ],
        )]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must settle TP");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::TakeProfit);
        assert_eq!(out.entry_hit_at, Some(dt_at(t0)));
        assert_eq!(out.resolved_at, dt_at(t0 + H4_MS));
        assert_eq!(out.resolution_timeframe, Timeframe::H4);
    }

    #[test]
    fn test_u3_short_entry_before_tp() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = short_plan(100.0, 90.0, 105.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + H4_MS, 92.0, 88.0),
            ],
        )]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must settle TP");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::TakeProfit);
        assert_eq!(out.entry_hit_at, Some(dt_at(t0)));
        assert_eq!(out.resolved_at, dt_at(t0 + H4_MS));
    }

    #[test]
    fn test_u3_long_entry_before_sl() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + H4_MS, 96.0, 93.0),
            ],
        )]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must settle SL");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::StopLoss);
        assert_eq!(out.entry_hit_at, Some(dt_at(t0)));
        assert_eq!(out.resolved_at, dt_at(t0 + H4_MS));
    }

    #[test]
    fn test_u3_short_entry_before_sl() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = short_plan(100.0, 90.0, 105.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + H4_MS, 106.0, 104.0),
            ],
        )]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must settle SL");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::StopLoss);
    }

    #[test]
    fn test_u3_exit_before_entry_ignored() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 111.0, 109.0),
                (t0 + H4_MS, 101.0, 99.0),
                (t0 + 2 * H4_MS, 112.0, 109.0),
            ],
        )]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must settle TP");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::TakeProfit);
        assert_eq!(out.entry_hit_at, Some(dt_at(t0 + H4_MS)));
        assert_eq!(out.resolved_at, dt_at(t0 + 2 * H4_MS));
    }

    #[test]
    fn test_u3_entry_never_touched() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 102.0, 101.0),
                (t0 + H4_MS, 104.0, 103.0),
            ],
        )]);
        assert!(evaluate_trade_plan(&plan, signal, &all).is_none());
    }

    #[test]
    fn test_u3_open_but_unresolved() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + H4_MS, 105.0, 102.0),
                (t0 + 2 * H4_MS, 104.0, 101.0),
            ],
        )]);
        assert!(evaluate_trade_plan(&plan, signal, &all).is_none());
    }

    #[test]
    fn test_u3_inclusive_level_boundaries() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 100.0, 100.0),
                (t0 + H4_MS, 110.0, 110.0),
            ],
        )]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("inclusive touches settle");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::TakeProfit);
    }

    #[test]
    fn test_u3_coarse_both_exits_finer_tp_first() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let c1 = t0 + H4_MS;
        let all = dfs(vec![
            (
                Timeframe::H4,
                vec![
                    (t0 - H4_MS, 81.0, 80.0),
                    (t0, 101.0, 99.0),
                    (c1, 112.0, 94.0),
                ],
            ),
            (
                Timeframe::H1,
                vec![
                    (c1, 111.0, 109.0),
                    (c1 + H1_MS, 96.0, 93.0),
                    (c1 + 2 * H1_MS, 102.0, 100.0),
                    (c1 + 3 * H1_MS, 103.0, 101.0),
                ],
            ),
        ]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("finer TP first");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::TakeProfit);
        assert_eq!(out.resolution_timeframe, Timeframe::H1);
        assert_eq!(out.resolved_at, dt_at(c1));
        assert_eq!(out.entry_hit_at, Some(dt_at(t0)));
    }

    #[test]
    fn test_u3_coarse_both_exits_finer_sl_first() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let c1 = t0 + H4_MS;
        let all = dfs(vec![
            (
                Timeframe::H4,
                vec![
                    (t0 - H4_MS, 81.0, 80.0),
                    (t0, 101.0, 99.0),
                    (c1, 112.0, 94.0),
                ],
            ),
            (
                Timeframe::H1,
                vec![
                    (c1, 96.0, 93.0),
                    (c1 + H1_MS, 111.0, 109.0),
                    (c1 + 2 * H1_MS, 102.0, 100.0),
                    (c1 + 3 * H1_MS, 103.0, 101.0),
                ],
            ),
        ]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("finer SL first");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::StopLoss);
        assert_eq!(out.resolution_timeframe, Timeframe::H1);
        assert_eq!(out.resolved_at, dt_at(c1));
    }

    #[test]
    fn test_u3_entry_exit_same_coarse_finer_tp_first() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![
            (
                Timeframe::H4,
                vec![(t0 - H4_MS, 81.0, 80.0), (t0, 111.0, 99.0)],
            ),
            (
                Timeframe::H1,
                vec![
                    (t0, 101.0, 99.0),
                    (t0 + H1_MS, 111.0, 109.0),
                    (t0 + 2 * H1_MS, 103.0, 101.0),
                    (t0 + 3 * H1_MS, 104.0, 102.0),
                ],
            ),
        ]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("entry then TP");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::TakeProfit);
        assert_eq!(out.entry_hit_at, Some(dt_at(t0)));
        assert_eq!(out.resolved_at, dt_at(t0 + H1_MS));
        assert_eq!(out.resolution_timeframe, Timeframe::H1);
    }

    #[test]
    fn test_u3_finest_candle_touching_both_is_ambiguous() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let c1 = t0 + H4_MS;
        let all = dfs(vec![
            (
                Timeframe::H4,
                vec![
                    (t0 - H4_MS, 81.0, 80.0),
                    (t0, 101.0, 99.0),
                    (c1, 112.0, 94.0),
                ],
            ),
            (
                Timeframe::H1,
                vec![
                    (c1, 112.0, 94.0),
                    (c1 + H1_MS, 102.0, 100.0),
                    (c1 + 2 * H1_MS, 103.0, 101.0),
                    (c1 + 3 * H1_MS, 104.0, 102.0),
                ],
            ),
        ]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must be terminal");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::Ambiguous);
        assert_eq!(out.entry_hit_at, Some(dt_at(t0)));
    }

    #[test]
    fn test_u3_entry_exit_finest_both_is_ambiguous() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![
            (
                Timeframe::H4,
                vec![(t0 - H4_MS, 81.0, 80.0), (t0, 111.0, 99.0)],
            ),
            (
                Timeframe::H1,
                vec![
                    (t0, 111.0, 99.0),
                    (t0 + H1_MS, 103.0, 101.0),
                    (t0 + 2 * H1_MS, 104.0, 102.0),
                    (t0 + 3 * H1_MS, 105.0, 103.0),
                ],
            ),
        ]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must be terminal");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::Ambiguous);
        assert_eq!(out.entry_hit_at, None);
    }

    #[test]
    fn test_u3_incomplete_finer_coverage_is_ambiguous() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let c1 = t0 + H4_MS;
        let all = dfs(vec![
            (
                Timeframe::H4,
                vec![
                    (t0 - H4_MS, 81.0, 80.0),
                    (t0, 101.0, 99.0),
                    (c1, 112.0, 94.0),
                ],
            ),
            (
                Timeframe::H1,
                vec![
                    (c1, 111.0, 109.0),
                    (c1 + H1_MS, 96.0, 93.0),
                    (c1 + 2 * H1_MS, 103.0, 101.0),
                ],
            ),
        ]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must be terminal");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::Ambiguous);
    }

    #[test]
    fn test_u3_signal_overlap_uncertainty_is_ambiguous() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0 + H1_MS);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![(t0, 101.0, 99.0), (t0 + H4_MS, 112.0, 105.0)],
        )]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must be terminal");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::Ambiguous);
        assert_eq!(out.entry_hit_at, None);
    }

    #[test]
    fn test_u3_signal_overlap_finer_proves_entry_then_tp() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0 + H1_MS);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![
            (
                Timeframe::H4,
                vec![(t0, 101.0, 99.0), (t0 + H4_MS, 112.0, 105.0)],
            ),
            (
                Timeframe::H1,
                vec![
                    (t0, 81.0, 80.0),
                    (t0 + H1_MS, 82.0, 81.0),
                    (t0 + 2 * H1_MS, 101.0, 99.0),
                    (t0 + 3 * H1_MS, 102.0, 100.0),
                ],
            ),
            (
                Timeframe::M15,
                vec![
                    (t0 + H4_MS, 106.0, 105.0),
                    (t0 + H4_MS + 900_000, 107.0, 106.0),
                    (t0 + H4_MS + 2 * 900_000, 108.0, 107.0),
                    (t0 + H4_MS + 3 * 900_000, 109.0, 108.0),
                    (t0 + H4_MS + 4 * 900_000, 110.5, 109.5),
                    (t0 + H4_MS + 5 * 900_000, 106.0, 105.0),
                    (t0 + H4_MS + 6 * 900_000, 106.0, 105.0),
                    (t0 + H4_MS + 7 * 900_000, 106.0, 105.0),
                    (t0 + H4_MS + 8 * 900_000, 106.0, 105.0),
                    (t0 + H4_MS + 9 * 900_000, 106.0, 105.0),
                    (t0 + H4_MS + 10 * 900_000, 106.0, 105.0),
                    (t0 + H4_MS + 11 * 900_000, 106.0, 105.0),
                    (t0 + H4_MS + 12 * 900_000, 106.0, 105.0),
                    (t0 + H4_MS + 13 * 900_000, 106.0, 105.0),
                    (t0 + H4_MS + 14 * 900_000, 106.0, 105.0),
                    (t0 + H4_MS + 15 * 900_000, 106.0, 105.0),
                ],
            ),
        ]);
        // Overlapping H4 entry is proven post-signal by H1, then TP on the next
        // H4 resolves without further ambiguity.
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must settle TP");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::TakeProfit);
        assert_eq!(out.entry_hit_at, Some(dt_at(t0 + 2 * H1_MS)));
    }

    #[test]
    fn test_u3_expired_history_returns_none() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![(t0 + H4_MS, 101.0, 99.0), (t0 + 2 * H4_MS, 112.0, 105.0)],
        )]);
        assert!(evaluate_trade_plan(&plan, signal, &all).is_none());
    }

    #[test]
    fn test_u3_extrema_metadata_tracks_signal_onward_range() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 50.0, 40.0),
                (t0, 102.0, 98.0),
                (t0 + H4_MS, 103.0, 97.0),
                (t0 + 2 * H4_MS, 115.0, 105.0),
            ],
        )]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("must settle TP");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::TakeProfit);
        assert!((out.lowest_reached - 97.0).abs() < f64::EPSILON);
        assert!((out.highest_reached - 115.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_u3_ignores_higher_timeframes() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        let all = dfs(vec![
            (
                Timeframe::H4,
                vec![
                    (t0 - H4_MS, 81.0, 80.0),
                    (t0, 101.0, 99.0),
                    (t0 + H4_MS, 104.0, 102.0),
                ],
            ),
            (Timeframe::D1, vec![(t0 - 86_400_000, 200.0, 50.0)]),
        ]);
        // D1 touches both exits but must never be consulted for an H4 plan.
        assert!(evaluate_trade_plan(&plan, signal, &all).is_none());
    }

    #[test]
    fn test_u3_ineligible_plans_return_none() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + H4_MS, 112.0, 105.0),
            ],
        )]);
        // WAIT direction.
        let wait = crate::memory::TradePlan {
            label: "A".into(),
            direction: "WAIT".into(),
            entry: None,
            target: None,
            stop: None,
            rationale: "test".into(),
            timeframe: None,
            outcome: None,
        };
        assert!(evaluate_trade_plan(&wait, signal, &all).is_none());
        // Missing timeframe.
        assert!(evaluate_trade_plan(&long_plan(100.0, 110.0, 95.0, None), signal, &all).is_none());
        // Sub-4h timeframe.
        assert!(
            evaluate_trade_plan(
                &long_plan(100.0, 110.0, 95.0, Some(Timeframe::H1)),
                signal,
                &all
            )
            .is_none()
        );
        // Malformed ordering (LONG stop above entry).
        assert!(
            evaluate_trade_plan(
                &long_plan(100.0, 90.0, 105.0, Some(Timeframe::H4)),
                signal,
                &all
            )
            .is_none()
        );
        // Already terminal.
        let terminal = settled_plan(
            long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4)),
            crate::memory::TradePlanOutcomeKind::TakeProfit,
        );
        assert!(evaluate_trade_plan(&terminal, signal, &all).is_none());
    }

    #[test]
    fn test_u3_mixed_tp_sl_average() {
        use algotrap::prelude::Timeframe;
        let tp = settled_plan(
            long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4)),
            crate::memory::TradePlanOutcomeKind::TakeProfit,
        );
        let sl = settled_plan(
            short_plan(100.0, 90.0, 105.0, Some(Timeframe::H4)),
            crate::memory::TradePlanOutcomeKind::StopLoss,
        );
        let score = trade_plan_outcome_score(&[tp, sl]).expect("must score");
        assert!((score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_u3_ambiguous_excluded_from_score() {
        use algotrap::prelude::Timeframe;
        let tp = settled_plan(
            long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4)),
            crate::memory::TradePlanOutcomeKind::TakeProfit,
        );
        let amb = settled_plan(
            long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4)),
            crate::memory::TradePlanOutcomeKind::Ambiguous,
        );
        let score = trade_plan_outcome_score(&[tp, amb]).expect("must score");
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_u3_no_score_until_all_directional_terminal() {
        use algotrap::prelude::Timeframe;
        let tp = settled_plan(
            long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4)),
            crate::memory::TradePlanOutcomeKind::TakeProfit,
        );
        let pending = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        assert!(trade_plan_outcome_score(&[tp, pending]).is_none());
    }

    #[test]
    fn test_u3_all_ambiguous_returns_none() {
        use algotrap::prelude::Timeframe;
        let a = settled_plan(
            long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4)),
            crate::memory::TradePlanOutcomeKind::Ambiguous,
        );
        let b = settled_plan(
            short_plan(100.0, 90.0, 105.0, Some(Timeframe::H4)),
            crate::memory::TradePlanOutcomeKind::Ambiguous,
        );
        assert!(trade_plan_outcome_score(&[a, b]).is_none());
    }

    #[test]
    fn test_u3_score_ignores_wait_and_invalid_plans() {
        use algotrap::prelude::Timeframe;
        let tp = settled_plan(
            long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4)),
            crate::memory::TradePlanOutcomeKind::TakeProfit,
        );
        let wait = crate::memory::TradePlan {
            label: "B".into(),
            direction: "WAIT".into(),
            entry: None,
            target: None,
            stop: None,
            rationale: "test".into(),
            timeframe: None,
            outcome: None,
        };
        let score = trade_plan_outcome_score(&[tp, wait.clone()]).expect("WAIT excluded");
        assert!((score - 1.0).abs() < f64::EPSILON);
        assert!(trade_plan_outcome_score(&[wait]).is_none());
    }

    #[test]
    fn test_u3_missing_signal_interval_returns_none() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        // Old pre-signal candle exists, but the candle covering `signal_at`
        // ([t0, t0+4h)) is missing; later candles show apparent entry then TP.
        // Without continuous coverage the path is unproven: unresolved None.
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0 + H4_MS, 101.0, 99.0),
                (t0 + 2 * H4_MS, 112.0, 105.0),
            ],
        )]);
        assert!(evaluate_trade_plan(&plan, signal, &all).is_none());
    }

    #[test]
    fn test_u3_mid_path_gap_returns_none() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        // Signal candle proves entry, the next coarse interval is missing,
        // and a later candle shows apparent TP. The gap breaks continuity.
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + 2 * H4_MS, 112.0, 105.0),
            ],
        )]);
        assert!(evaluate_trade_plan(&plan, signal, &all).is_none());
    }

    #[test]
    fn test_u3_continuous_coverage_control_settles_tp() {
        use algotrap::prelude::Timeframe;
        let t0 = base_ms();
        let signal = dt_at(t0);
        let plan = long_plan(100.0, 110.0, 95.0, Some(Timeframe::H4));
        // Same shape as the mid-path gap test but with every coarse interval
        // present: entry then TP across a continuous path still settles.
        let all = dfs(vec![(
            Timeframe::H4,
            vec![
                (t0 - H4_MS, 81.0, 80.0),
                (t0, 101.0, 99.0),
                (t0 + H4_MS, 104.0, 102.0),
                (t0 + 2 * H4_MS, 112.0, 105.0),
            ],
        )]);
        let out = evaluate_trade_plan(&plan, signal, &all).expect("continuous path settles");
        assert_eq!(out.kind, crate::memory::TradePlanOutcomeKind::TakeProfit);
        assert_eq!(out.entry_hit_at, Some(dt_at(t0)));
        assert_eq!(out.resolved_at, dt_at(t0 + 2 * H4_MS));
    }
}
